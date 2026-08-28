/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cross-cell HTTP/1 reuse for bounded origins.
//!
//! [`H1Reuse`] tracks the latest availability reported by each cell and pairs
//! the oldest origin-wide demand with a different cell's connection. Eligible
//! demand borrows the connection's request sender. Ineligible demand reclaims
//! the connection's capacity instead.
//!
//! Admission owns the operation record. The connection-owning cell retains a
//! local `H1ReuseReservation` serialized with sender return. Values crossing
//! between those lock domains own their fallback: an uninstalled operation is
//! cancelled, and an uncommitted candidate returns its sender before demand
//! becomes schedulable again.
//!
//! Admission advances each operation through these phases:
//!
//! ```text
//! Installing -- reserve future return ------------------------> Installed
//! Installing -- extract idle sender --------------------------> Resolving
//! Installing(cancelled) -- acknowledge reservation ----------> Cancelling
//! Installed -- intercept sender return -----------------------> Resolving
//! Installed -- demand cancelled ------------------------------> Cancelling
//! Resolving -- borrow or reclaim completes -------------------> removed
//! Cancelling -- connection-owning cell clears reservation ----> removed
//! ```

use super::{
    AdmissionAction, DeliveryAckResult, DeliveryGuard, DeliveryId, DemandId, DemandSchedule,
    OriginAdmission, ProtocolRequirement,
};
use crate::client::pool::cell::h1::{H1Selection, ProvisionalH1};
use crate::client::pool::cell::{CellId, OriginCell};
use crate::client::pool::partition::EligibilityGroup;
use crate::sync::Arc;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;

/// Identity of one cross-cell HTTP/1 reuse operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::client::pool) struct ReuseId(u64);

impl ReuseId {
    /// Creates a deterministic identity for focused transition tests.
    #[cfg(test)]
    pub(in crate::client::pool) const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// How an HTTP/1 reuse operation satisfies waiting demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) enum ReuseMode {
    /// Move the selected sender to the requesting cell for dispatch.
    Borrow,
    /// Close the selected connection and deliver its capacity instead.
    Reclaim,
}

/// Origin-locked HTTP/1 availability order and active reuse operations.
#[derive(Debug, Default)]
pub(super) struct H1Reuse {
    /// Latest availability for every cell that has advertised an HTTP/1 record.
    connection_cells: HashMap<CellId, H1AvailabilityRecord>,
    /// FIFO view across every cell with a connection available for reclaim.
    origin_order: H1AvailabilityOrder,
    /// FIFO views of connections eligible for borrowing.
    group_orders: HashMap<EligibilityGroup, H1AvailabilityOrder>,
    /// Nonterminal operations indexed by their never-reused identity.
    operations: HashMap<ReuseId, ReuseRecord>,
    /// Requesting cells that already have a reuse operation in progress.
    requesting_cells: HashMap<CellId, ReuseId>,
    /// Installed operations whose requesting cell became stale.
    cancellations: VecDeque<ReuseId>,
    /// Next reuse operation identity.
    next_reuse: u64,
}

/// Complete HTTP/1 availability reported by a connection-owning cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct H1Availability {
    /// Whether at least one reusable or active HTTP/1 record remains.
    pub(in crate::client::pool) advertised: bool,
    /// Whether local H1 work temporarily excludes peer operations.
    pub(in crate::client::pool) blocked: bool,
}

/// Versioned HTTP/1 availability crossing from a cell to admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct H1AvailabilitySnapshot {
    /// Monotonic revision assigned under the cell lock.
    revision: u64,
    /// Complete availability at this revision.
    availability: H1Availability,
}

impl H1AvailabilitySnapshot {
    /// Creates one complete connection-owning cell report.
    pub(in crate::client::pool) fn new(revision: u64, availability: H1Availability) -> Self {
        Self {
            revision,
            availability,
        }
    }
}

/// Terminal HTTP/1 availability observed at a connection-owning cell.
pub(in crate::client::pool) enum H1AvailabilityOutcome {
    /// The connection-owning cell remains live and reports its complete availability.
    Reported {
        /// Cell described by `snapshot`.
        connection_cell: CellId,
        /// Complete availability after the terminal transition.
        snapshot: H1AvailabilitySnapshot,
    },
    /// The connection-owning cell disappeared before the crossing completed.
    Expired {
        /// Cell no longer retained by the partition registry.
        connection_cell: CellId,
    },
}

impl H1AvailabilityOutcome {
    /// Wraps a complete report with the connection-owning cell identity it describes.
    pub(in crate::client::pool) fn reported(
        connection_cell: CellId,
        snapshot: H1AvailabilitySnapshot,
    ) -> Self {
        Self::Reported {
            connection_cell,
            snapshot,
        }
    }

    /// Records that a connection-owning cell no longer exists.
    pub(in crate::client::pool) fn expired(connection_cell: CellId) -> Self {
        Self::Expired { connection_cell }
    }

    /// Returns the connection-owning cell whose admission view must change.
    fn connection_cell(&self) -> &CellId {
        match self {
            Self::Reported {
                connection_cell, ..
            }
            | Self::Expired { connection_cell } => connection_cell,
        }
    }
}

/// Result of installing a reuse reservation at the connection-owning cell.
pub(in crate::client::pool) enum ReuseInstallResult {
    /// A future reusable return will satisfy the reuse operation.
    Installed,
    /// An idle sender was extracted immediately.
    Candidate(ReuseCandidate),
    /// The connection-owning cell could not retain the reuse operation.
    Rejected(H1AvailabilitySnapshot),
}

/// Admission's current view of one HTTP/1 connection-owning cell.
#[derive(Debug)]
struct H1AvailabilityRecord {
    /// Reuse group whose peers may borrow this connection-owning cell's sender.
    group: EligibilityGroup,
    /// Newest connection-owning cell report accepted by admission.
    revision: u64,
    /// Whether the cell has an H1 record that can return or be reclaimed.
    advertised: bool,
    /// Reuse operation currently reserving this cell's connection.
    reuse_id: Option<ReuseId>,
    /// Whether connection-owning cell-local work temporarily excludes peers.
    blocked: bool,
    /// Linked scheduling residence while this connection-owning cell is selectable.
    residence: H1AvailabilityResidence,
}

impl H1AvailabilityRecord {
    /// Returns whether this connection-owning cell must occupy both scheduling views.
    fn is_schedulable(&self) -> bool {
        self.advertised && !self.blocked && self.reuse_id.is_none()
    }
}

/// Whether a connection-owning cell is linked in both scheduling views.
#[derive(Debug, Default)]
enum H1AvailabilityResidence {
    /// The connection-owning cell is absent from connection-owning cell-selection order.
    #[default]
    Unavailable,
    /// The connection-owning cell is linked once in origin and group order.
    Available {
        /// Links in origin-wide reclaim order.
        origin: H1AvailabilityLinks,
        /// Links in eligibility-group borrow order.
        group: H1AvailabilityLinks,
    },
}

impl H1AvailabilityResidence {
    /// Returns whether the connection-owning cell is linked in both views.
    fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Returns this residence's links for one scheduling view.
    fn links(&self, view: H1AvailabilityView) -> Option<&H1AvailabilityLinks> {
        match (self, view) {
            (Self::Available { origin, .. }, H1AvailabilityView::Origin) => Some(origin),
            (Self::Available { group, .. }, H1AvailabilityView::Group) => Some(group),
            (Self::Unavailable, _) => None,
        }
    }

    /// Returns mutable links while repairing one scheduling view.
    fn links_mut(&mut self, view: H1AvailabilityView) -> &mut H1AvailabilityLinks {
        match (self, view) {
            (Self::Available { origin, .. }, H1AvailabilityView::Origin) => origin,
            (Self::Available { group, .. }, H1AvailabilityView::Group) => group,
            (Self::Unavailable, _) => {
                panic!("unavailable connection-owning cell has no order links")
            }
        }
    }
}

/// Selects one of the two intrusive connection-owning cell-order link sets.
#[derive(Clone, Copy)]
enum H1AvailabilityView {
    /// Origin-wide connection-reclaim order.
    Origin,
    /// Eligibility-group connection-borrow order.
    Group,
}

/// Intrusive links for one connection-owning cell-selection view.
#[derive(Debug)]
struct H1AvailabilityLinks {
    /// Previous available connection-owning cell in this view.
    previous: Option<CellId>,
    /// Next available connection-owning cell in this view.
    next: Option<CellId>,
}

/// Endpoints and length of one connection-owning cell-selection FIFO.
#[derive(Debug, Default)]
enum H1AvailabilityOrder {
    /// No connection-owning cell is schedulable.
    #[default]
    Empty,
    /// At least one connection-owning cell is linked in this view.
    Active {
        /// Oldest available connection-owning cell.
        head: CellId,
        /// Newest available connection-owning cell.
        tail: CellId,
        /// Number of linked connection-owning cells.
        len: NonZeroUsize,
    },
}

impl H1AvailabilityOrder {
    /// Returns the oldest available connection-owning cell.
    fn head(&self) -> Option<&CellId> {
        match self {
            Self::Empty => None,
            Self::Active { head, .. } => Some(head),
        }
    }

    /// Returns the newest available connection-owning cell.
    fn tail(&self) -> Option<&CellId> {
        match self {
            Self::Empty => None,
            Self::Active { tail, .. } => Some(tail),
        }
    }

    /// Returns the number of linked connection-owning cells.
    fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Active { len, .. } => len.get(),
        }
    }

    /// Appends one newly available connection-owning cell.
    fn push_back(&mut self, connection_cell: CellId) {
        match self {
            order @ Self::Empty => {
                *order = Self::Active {
                    head: connection_cell.clone(),
                    tail: connection_cell,
                    len: NonZeroUsize::MIN,
                };
            }
            Self::Active { tail, len, .. } => {
                *tail = connection_cell;
                *len = len
                    .checked_add(1)
                    .expect("HTTP/1 connection-owning cell order length exhausted");
            }
        }
    }

    /// Removes one connection-owning cell after its neighboring links were repaired.
    fn remove(&mut self, connection_cell: &CellId, links: &H1AvailabilityLinks) {
        let order = std::mem::take(self);
        let Self::Active { head, tail, len } = order else {
            unreachable!("removed a connection-owning cell from an empty order");
        };
        debug_assert_eq!(head == *connection_cell, links.previous.is_none());
        debug_assert_eq!(tail == *connection_cell, links.next.is_none());
        if len == NonZeroUsize::MIN {
            return;
        }
        *self = Self::Active {
            head: if head == *connection_cell {
                links
                    .next
                    .clone()
                    .expect("removed connection-owning cell head had no successor")
            } else {
                head
            },
            tail: if tail == *connection_cell {
                links
                    .previous
                    .clone()
                    .expect("removed connection-owning cell tail had no predecessor")
            } else {
                tail
            },
            len: NonZeroUsize::new(
                len.get()
                    .checked_sub(1)
                    .expect("HTTP/1 connection-owning cell order length underflowed"),
            )
            .expect("nonempty connection-owning cell order lost its length"),
        };
    }
}

/// Admission-owned state for one nonterminal reuse operation.
#[derive(Clone, Debug)]
struct ReuseRecord {
    /// Cell whose connection is reserved by this operation.
    connection_cell: CellId,
    /// Cell whose demand caused this operation.
    requesting_cell: CellId,
    /// Exact requesting cell demand generation fenced by the reuse operation.
    demand: DemandId,
    /// Whether resolution borrows a sender or reclaims its capacity.
    mode: ReuseMode,
    /// Admission-side progress of the reuse operation.
    phase: ReusePhase,
    /// Whether requesting cell demand became stale before connection-owning-cell
    /// resolution.
    cancelled: bool,
}

/// Origin-side progress of one reuse operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReusePhase {
    /// Admission selected a connection, but the cell has not reserved it.
    Installing,
    /// The cell will intercept the connection's next reusable return.
    Installed,
    /// A provisional sender is resolving outside admission.
    Resolving,
    /// Request cancellation must clear the installed reservation.
    Cancelling,
}

/// Work required to install a reuse reservation outside the admission lock.
#[derive(Debug)]
pub(in crate::client::pool) struct PreparedReuseInstall {
    /// Operation being installed.
    pub(in crate::client::pool) id: ReuseId,
    /// Cell whose connection was selected while admission was locked.
    pub(in crate::client::pool) connection_cell: CellId,
}

/// Work required to cancel a reservation outside the admission lock.
pub(super) struct PreparedReuseCancellation {
    /// Operation whose reservation must be cleared.
    id: ReuseId,
    /// Cell that owns the reservation.
    connection_cell: CellId,
}

impl H1Reuse {
    /// Publishes the connection-owning cell's complete current availability.
    pub(super) fn update_availability(
        &mut self,
        connection_cell: CellId,
        group: EligibilityGroup,
        snapshot: H1AvailabilitySnapshot,
    ) {
        if self
            .connection_cells
            .get(&connection_cell)
            .is_some_and(|record| record.revision >= snapshot.revision)
        {
            return;
        }
        self.unlink_cell(&connection_cell);
        let record = self
            .connection_cells
            .entry(connection_cell.clone())
            .or_insert_with(|| H1AvailabilityRecord {
                group: group.clone(),
                revision: snapshot.revision,
                advertised: snapshot.availability.advertised,
                reuse_id: None,
                blocked: snapshot.availability.blocked,
                residence: H1AvailabilityResidence::Unavailable,
            });
        record.group = group;
        record.revision = snapshot.revision;
        record.advertised = snapshot.availability.advertised;
        record.blocked = snapshot.availability.blocked;
        self.enqueue_cell_if_available(&connection_cell);
        self.assert_consistent();
    }

    /// Marks a requesting cell reuse operation stale after demand publication or delivery.
    pub(super) fn reconcile_requesting_cell(
        &mut self,
        requesting_cell: &CellId,
        schedule: &DemandSchedule,
    ) {
        let Some(reuse_id) = self.requesting_cells.get(requesting_cell).copied() else {
            return;
        };
        let Some(record) = self.operations.get_mut(&reuse_id) else {
            self.requesting_cells.remove(requesting_cell);
            self.assert_consistent();
            return;
        };
        if schedule.is_current_queued(&record.requesting_cell, record.demand) {
            return;
        }
        record.cancelled = true;
        if record.phase == ReusePhase::Installed {
            record.phase = ReusePhase::Cancelling;
            self.cancellations.push_back(reuse_id);
        }
        self.assert_consistent();
    }

    /// Selects another cell's connection for the oldest origin-wide demand.
    ///
    /// An eligible cell lends its sender. Otherwise, a connection is reclaimed
    /// so its capacity can satisfy the same demand. A cell is never selected for
    /// its own demand; local idle and returning senders are handled under the
    /// cell lock.
    pub(super) fn prepare_reuse(
        &mut self,
        schedule: &DemandSchedule,
    ) -> Option<PreparedReuseInstall> {
        let requesting_cell = schedule.queued_head()?;
        if self
            .requesting_cells
            .contains_key(&requesting_cell.requesting_cell)
        {
            return None;
        }

        let (connection_cell, mode) =
            if requesting_cell.requirement == ProtocolRequirement::H1Compatible {
                match self.take_group_peer(
                    &requesting_cell.eligibility_group,
                    &requesting_cell.requesting_cell,
                ) {
                    Some(connection_cell) => (connection_cell, ReuseMode::Borrow),
                    None => (
                        self.take_origin_peer(&requesting_cell.requesting_cell)?,
                        ReuseMode::Reclaim,
                    ),
                }
            } else {
                (
                    self.take_origin_peer(&requesting_cell.requesting_cell)?,
                    ReuseMode::Reclaim,
                )
            };

        let id = self.take_reuse_id();
        let availability_record = self
            .connection_cells
            .get_mut(&connection_cell)
            .expect("selected HTTP/1 connection-owning cell disappeared");
        debug_assert!(availability_record.reuse_id.is_none());
        availability_record.reuse_id = Some(id);
        self.requesting_cells
            .insert(requesting_cell.requesting_cell.clone(), id);
        self.operations.insert(
            id,
            ReuseRecord {
                connection_cell: connection_cell.clone(),
                requesting_cell: requesting_cell.requesting_cell.clone(),
                demand: requesting_cell.demand,
                mode,
                phase: ReusePhase::Installing,
                cancelled: false,
            },
        );
        self.assert_consistent();
        Some(PreparedReuseInstall {
            id,
            connection_cell,
        })
    }

    /// Extracts cancellation work, discarding entries for completed operations.
    pub(super) fn prepare_cancellation(&mut self) -> Option<PreparedReuseCancellation> {
        while let Some(id) = self.cancellations.pop_front() {
            let Some(record) = self.operations.get(&id) else {
                continue;
            };
            if record.phase != ReusePhase::Cancelling {
                continue;
            }
            return Some(PreparedReuseCancellation {
                id,
                connection_cell: record.connection_cell.clone(),
            });
        }
        None
    }

    /// Applies an install acknowledgement and returns the retained reuse operation.
    fn finish_install(&mut self, id: ReuseId, resolved: bool) -> Option<ReuseRecord> {
        let record = self.operations.get_mut(&id)?;
        if record.phase != ReusePhase::Installing {
            return None;
        }
        record.phase = if resolved {
            ReusePhase::Resolving
        } else if record.cancelled {
            ReusePhase::Cancelling
        } else {
            ReusePhase::Installed
        };
        let record = record.clone();
        self.assert_consistent();
        Some(record)
    }

    /// Moves an installed reuse operation to provisional-candidate resolution.
    fn begin_resolution(&mut self, id: ReuseId) -> Option<ReuseRecord> {
        let record = self.operations.get_mut(&id)?;
        if !matches!(
            record.phase,
            ReusePhase::Installed | ReusePhase::Resolving | ReusePhase::Cancelling
        ) {
            return None;
        }
        record.phase = ReusePhase::Resolving;
        let record = record.clone();
        self.assert_consistent();
        Some(record)
    }

    /// Removes one reuse operation and applies the connection-owning cell's terminal report.
    ///
    /// The outcome names its connection-owning cell explicitly so an expired cell cannot be
    /// mistaken for an unchanged connection-owning cell and a duplicate terminal report still
    /// refreshes admission's connection-owning cell view.
    fn finish_reuse(&mut self, id: ReuseId, outcome: H1AvailabilityOutcome) -> Option<ReuseRecord> {
        let outcome_cell = outcome.connection_cell().clone();
        self.apply_h1_availability_outcome(outcome);

        let record = self.operations.remove(&id);
        if let Some(record) = record.as_ref() {
            debug_assert_eq!(record.connection_cell, outcome_cell);
            if self.requesting_cells.get(&record.requesting_cell) == Some(&id) {
                self.requesting_cells.remove(&record.requesting_cell);
            }
            if let Some(connection_cell) = self.connection_cells.get_mut(&record.connection_cell) {
                if connection_cell.reuse_id == Some(id) {
                    connection_cell.reuse_id = None;
                }
            }
        }

        self.enqueue_cell_if_available(&outcome_cell);
        self.assert_consistent();
        record
    }

    /// Applies a report without assuming its reuse operation record still exists.
    fn apply_h1_availability_outcome(&mut self, outcome: H1AvailabilityOutcome) {
        match outcome {
            H1AvailabilityOutcome::Reported {
                connection_cell,
                snapshot,
            } => {
                if self
                    .connection_cells
                    .get(&connection_cell)
                    .is_some_and(|record| record.revision >= snapshot.revision)
                {
                    return;
                }
                self.unlink_cell(&connection_cell);
                if let Some(record) = self.connection_cells.get_mut(&connection_cell) {
                    record.revision = snapshot.revision;
                    record.advertised = snapshot.availability.advertised;
                    record.blocked = snapshot.availability.blocked;
                }
            }
            H1AvailabilityOutcome::Expired { connection_cell } => {
                self.unlink_cell(&connection_cell);
                self.connection_cells.remove(&connection_cell);
            }
        }
    }

    /// Removes and returns the first eligible peer in one reuse group.
    fn take_group_peer(
        &mut self,
        group: &EligibilityGroup,
        requesting_cell: &CellId,
    ) -> Option<CellId> {
        let connection_cell = {
            let order = self.group_orders.get(group)?;
            self.first_peer(order, requesting_cell, H1AvailabilityView::Group)?
        };
        self.unlink_cell(&connection_cell);
        Some(connection_cell)
    }

    /// Removes and returns the first origin-wide peer.
    fn take_origin_peer(&mut self, requesting_cell: &CellId) -> Option<CellId> {
        let connection_cell = self.first_peer(
            &self.origin_order,
            requesting_cell,
            H1AvailabilityView::Origin,
        )?;
        self.unlink_cell(&connection_cell);
        Some(connection_cell)
    }

    /// Returns the head, or its successor when the head is the requesting cell itself.
    fn first_peer(
        &self,
        order: &H1AvailabilityOrder,
        requesting_cell: &CellId,
        view: H1AvailabilityView,
    ) -> Option<CellId> {
        let head = order.head()?.clone();
        if head != *requesting_cell {
            return Some(head);
        }
        let record = self
            .connection_cells
            .get(&head)
            .expect("connection-owning cell order head disappeared");
        record
            .residence
            .links(view)
            .and_then(|links| links.next.clone())
    }

    /// Appends an available, unreserved cell to both scheduling views.
    fn enqueue_cell_if_available(&mut self, connection_cell: &CellId) {
        let Some(record) = self.connection_cells.get(connection_cell) else {
            return;
        };
        if !record.is_schedulable() || record.residence.is_available() {
            return;
        }
        let group = record.group.clone();
        let origin_previous = self.origin_order.tail().cloned();
        let group_previous = self
            .group_orders
            .get(&group)
            .and_then(H1AvailabilityOrder::tail)
            .cloned();

        if let Some(previous) = origin_previous.as_ref() {
            self.connection_cells
                .get_mut(previous)
                .expect("origin connection-owning cell order tail disappeared")
                .residence
                .links_mut(H1AvailabilityView::Origin)
                .next = Some(connection_cell.clone());
        }
        if let Some(previous) = group_previous.as_ref() {
            self.connection_cells
                .get_mut(previous)
                .expect("group connection-owning cell order tail disappeared")
                .residence
                .links_mut(H1AvailabilityView::Group)
                .next = Some(connection_cell.clone());
        }

        self.connection_cells
            .get_mut(connection_cell)
            .expect("enqueued connection-owning cell disappeared")
            .residence = H1AvailabilityResidence::Available {
            origin: H1AvailabilityLinks {
                previous: origin_previous,
                next: None,
            },
            group: H1AvailabilityLinks {
                previous: group_previous,
                next: None,
            },
        };
        self.origin_order.push_back(connection_cell.clone());
        self.group_orders
            .entry(group)
            .or_default()
            .push_back(connection_cell.clone());
    }

    /// Unlinks a connection-owning cell eagerly from both scheduling views.
    fn unlink_cell(&mut self, connection_cell: &CellId) {
        let Some(record) = self.connection_cells.get_mut(connection_cell) else {
            return;
        };
        let residence = std::mem::take(&mut record.residence);
        let H1AvailabilityResidence::Available { origin, group } = residence else {
            return;
        };
        let group_key = record.group.clone();

        Self::repair_links(
            &mut self.connection_cells,
            connection_cell,
            &origin,
            H1AvailabilityView::Origin,
        );
        self.origin_order.remove(connection_cell, &origin);

        Self::repair_links(
            &mut self.connection_cells,
            connection_cell,
            &group,
            H1AvailabilityView::Group,
        );
        self.group_orders
            .get_mut(&group_key)
            .expect("available connection-owning cell lost its group order")
            .remove(connection_cell, &group);
    }

    /// Repairs neighboring links after one connection-owning cell leaves an order.
    fn repair_links(
        connection_cells: &mut HashMap<CellId, H1AvailabilityRecord>,
        connection_cell: &CellId,
        links: &H1AvailabilityLinks,
        view: H1AvailabilityView,
    ) {
        if let Some(previous) = links.previous.as_ref() {
            connection_cells
                .get_mut(previous)
                .expect("previous connection-owning cell disappeared")
                .residence
                .links_mut(view)
                .next = links.next.clone();
        }
        if let Some(next) = links.next.as_ref() {
            connection_cells
                .get_mut(next)
                .expect("next connection-owning cell disappeared")
                .residence
                .links_mut(view)
                .previous = links.previous.clone();
        }
        debug_assert_ne!(links.previous.as_ref(), Some(connection_cell));
        debug_assert_ne!(links.next.as_ref(), Some(connection_cell));
    }

    /// Allocates a reuse operation identity.
    fn take_reuse_id(&mut self) -> ReuseId {
        let value = self.next_reuse;
        self.next_reuse = value
            .checked_add(1)
            .expect("HTTP/1 reuse operation identity exhausted");
        ReuseId(value)
    }

    /// Checks reuse-operation indexes and both availability orders after every mutation.
    fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        self.assert_consistent_debug();
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        for (id, operation) in &self.operations {
            assert_eq!(
                self.requesting_cells.get(&operation.requesting_cell),
                Some(id),
                "reuse operation's requesting-cell index did not name the operation"
            );
            assert_eq!(
                self.connection_cells
                    .get(&operation.connection_cell)
                    .and_then(|connection_cell| connection_cell.reuse_id),
                Some(*id),
                "reuse operation's connection-cell index did not name the operation"
            );
        }
        for (requesting_cell, id) in &self.requesting_cells {
            assert_eq!(
                self.operations
                    .get(id)
                    .map(|operation| &operation.requesting_cell),
                Some(requesting_cell),
                "requesting cell index named a missing reuse operation"
            );
        }
        for (connection_cell, record) in &self.connection_cells {
            if let Some(id) = record.reuse_id {
                assert_eq!(
                    self.operations
                        .get(&id)
                        .map(|operation| &operation.connection_cell),
                    Some(connection_cell),
                    "connection-owning cell index named a missing reuse operation"
                );
            }
            assert_eq!(
                record.residence.is_available(),
                record.is_schedulable(),
                "connection-owning cell scheduling residence did not match availability"
            );
        }
        self.assert_order(&self.origin_order, None, H1AvailabilityView::Origin);
        for (group, order) in &self.group_orders {
            self.assert_order(order, Some(group), H1AvailabilityView::Group);
        }
    }

    #[cfg(debug_assertions)]
    fn assert_order(
        &self,
        order: &H1AvailabilityOrder,
        expected_group: Option<&EligibilityGroup>,
        view: H1AvailabilityView,
    ) {
        let expected = self
            .connection_cells
            .values()
            .filter(|record| {
                record.residence.is_available()
                    && expected_group.is_none_or(|group| &record.group == group)
            })
            .count();
        let mut current = order.head().cloned();
        let mut previous = None;
        let mut traversed = 0;
        while let Some(connection_cell) = current {
            assert!(
                traversed < self.connection_cells.len(),
                "HTTP/1 connection-owning cell order contains a cycle"
            );
            let record = self
                .connection_cells
                .get(&connection_cell)
                .expect("ordered HTTP/1 connection-owning cell disappeared");
            if let Some(group) = expected_group {
                assert_eq!(
                    &record.group, group,
                    "connection-owning cell appeared in the wrong eligibility order"
                );
            }
            let links = record
                .residence
                .links(view)
                .expect("ordered connection-owning cell lost its links");
            assert_eq!(
                links.previous, previous,
                "connection-owning cell order contains inconsistent backward links"
            );
            previous = Some(connection_cell);
            current = links.next.clone();
            traversed += 1;
        }
        assert_eq!(
            expected, traversed,
            "connection-owning cell order omitted available records"
        );
        assert_eq!(
            order.len(),
            traversed,
            "connection-owning cell order length was incorrect"
        );
        assert_eq!(
            order.tail().cloned(),
            previous,
            "connection-owning cell order tail was not reachable"
        );
    }
}

/// One unlocked HTTP/1 coordination step.
pub(in crate::client::pool) enum H1ReuseAction {
    /// Install a prepared reservation in its connection-owning cell.
    Install(ReuseInstallAction),
    /// Cancel an installed reservation.
    Cancel(ReuseCancelAction),
    /// Close a selected connection and release its capacity.
    Reclaim(ReclaimAction),
    /// Complete the connection-owning cell after a sender transfer.
    CompleteConnectionCell(ConnectionCellCompletion),
}

impl H1ReuseAction {
    /// Creates an unlocked connection-owning cell installation crossing.
    pub(super) fn install(origin: Arc<OriginAdmission>, prepared: PreparedReuseInstall) -> Self {
        Self::Install(ReuseInstallAction {
            origin,
            install: Some(prepared),
        })
    }

    /// Creates an unlocked connection-owning cell-cancellation crossing.
    pub(super) fn cancel(
        origin: Arc<OriginAdmission>,
        cancellation: PreparedReuseCancellation,
    ) -> Self {
        Self::Cancel(ReuseCancelAction {
            origin,
            cancellation: Some(cancellation),
        })
    }

    /// Executes one crossing and returns the next admission action.
    pub(super) fn drive_once(self) -> Option<AdmissionAction> {
        match self {
            Self::Install(action) => action.drive_once(),
            Self::Cancel(action) => action.drive_once(),
            Self::Reclaim(action) => action.drive_once(),
            Self::CompleteConnectionCell(action) => action.drive_once(),
        }
    }
}

/// Reservation installation crossing from admission to a connection-owning cell.
pub(in crate::client::pool) struct ReuseInstallAction {
    /// Admission authority that prepared and owns the reuse operation record.
    origin: Arc<OriginAdmission>,
    /// Prepared reservation still owned by this cancellation fallback.
    install: Option<PreparedReuseInstall>,
}

impl ReuseInstallAction {
    /// Installs the prepared reservation without holding the admission lock.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let prepared = self
            .install
            .take()
            .expect("HTTP/1 reuse operation install consumed more than once");
        let Some(connection_cell) = self.origin.cell(&prepared.connection_cell) else {
            return OriginAdmission::finish_h1_reuse(
                &self.origin,
                prepared.id,
                H1AvailabilityOutcome::expired(prepared.connection_cell),
            );
        };
        OriginCell::install_h1_reuse(&connection_cell, self.origin.clone(), prepared)
    }
}

impl Drop for ReuseInstallAction {
    fn drop(&mut self) {
        if let Some(prepared) = self.install.take() {
            let outcome = h1_availability_outcome(
                &self.origin,
                &prepared.connection_cell,
                |connection_cell| connection_cell.cancel_h1_reuse(prepared.id),
            );
            let next = OriginAdmission::finish_h1_reuse(&self.origin, prepared.id, outcome);
            OriginAdmission::drive(next);
        }
    }
}

/// Installed reservation cancellation crossing to its connection-owning cell.
pub(in crate::client::pool) struct ReuseCancelAction {
    /// Admission authority that owns the reuse operation record.
    origin: Arc<OriginAdmission>,
    /// Cancellation work still owned by this guard.
    cancellation: Option<PreparedReuseCancellation>,
}

impl ReuseCancelAction {
    /// Clears the cell reservation and completes the admission operation.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let cancellation = self
            .cancellation
            .take()
            .expect("HTTP/1 reuse operation cancellation consumed more than once");
        let outcome = h1_availability_outcome(
            &self.origin,
            &cancellation.connection_cell,
            |connection_cell| connection_cell.cancel_h1_reuse(cancellation.id),
        );
        OriginAdmission::finish_h1_reuse(&self.origin, cancellation.id, outcome)
    }
}

impl Drop for ReuseCancelAction {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            let outcome = h1_availability_outcome(
                &self.origin,
                &cancellation.connection_cell,
                |connection_cell| connection_cell.cancel_h1_reuse(cancellation.id),
            );
            let next = OriginAdmission::finish_h1_reuse(&self.origin, cancellation.id, outcome);
            OriginAdmission::drive(next);
        }
    }
}

/// Provisional sender owned by one resolving reuse operation.
pub(in crate::client::pool) struct ReuseCandidate {
    /// Admission authority that owns the operation record.
    origin: Arc<OriginAdmission>,
    /// Operation that selected this sender.
    reuse_id: ReuseId,
    /// Cell that must revalidate or receive the sender back.
    connection_cell: CellId,
    /// Sender fallback while connection-owning cell resolution is incomplete.
    provisional: Option<ProvisionalH1>,
}

impl ReuseCandidate {
    /// Takes provisional sender ownership for one resolving connection-owning cell reuse operation.
    pub(in crate::client::pool) fn new(
        origin: Arc<OriginAdmission>,
        reuse_id: ReuseId,
        connection_cell: CellId,
        provisional: ProvisionalH1,
    ) -> Self {
        Self {
            origin,
            reuse_id,
            connection_cell,
            provisional: Some(provisional),
        }
    }

    /// Revalidates the cell reservation and turns the sender into a selection.
    ///
    /// Failure returns this guard intact so dropping it restores the sender to
    /// its owning cell and completes the reuse operation exactly once.
    pub(in crate::client::pool) fn commit(mut self) -> Result<H1Selection, Self> {
        let Some(connection_cell) = self.origin.cell(&self.connection_cell) else {
            return Err(self);
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 reuse operation candidate consumed more than once");
        match OriginCell::commit_h1_reuse(&connection_cell, self.reuse_id, provisional) {
            Ok(selection) => Ok(selection),
            Err(provisional) => {
                self.provisional = Some(provisional);
                Err(self)
            }
        }
    }

    /// Attempts reclaim and returns the connection-owning cell's explicit terminal outcome.
    fn reclaim(mut self) -> H1AvailabilityOutcome {
        let Some(connection_cell) = self.origin.cell(&self.connection_cell) else {
            drop(self.provisional.take());
            return H1AvailabilityOutcome::expired(self.connection_cell.clone());
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 reuse operation candidate consumed more than once");
        let availability =
            match OriginCell::reclaim_h1_reuse(&connection_cell, self.reuse_id, provisional) {
                Ok(availability) => availability,
                Err(provisional) => OriginCell::reject_h1_reuse_candidate(
                    &connection_cell,
                    self.reuse_id,
                    provisional,
                ),
            };
        H1AvailabilityOutcome::reported(self.connection_cell.clone(), availability)
    }
}

impl fmt::Debug for ReuseCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReuseCandidate")
            .field("reuse_id", &self.reuse_id)
            .field("connection_cell", &self.connection_cell)
            .field("provisional", &self.provisional)
            .finish_non_exhaustive()
    }
}

impl Drop for ReuseCandidate {
    fn drop(&mut self) {
        let Some(provisional) = self.provisional.take() else {
            return;
        };
        let outcome = match self.origin.cell(&self.connection_cell) {
            Some(connection_cell) => H1AvailabilityOutcome::reported(
                self.connection_cell.clone(),
                OriginCell::reject_h1_reuse_candidate(&connection_cell, self.reuse_id, provisional),
            ),
            None => {
                drop(provisional);
                H1AvailabilityOutcome::expired(self.connection_cell.clone())
            }
        };
        let next = OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome);
        OriginAdmission::drive(next);
    }
}

/// Reclaim decision carrying its selected provisional sender.
pub(in crate::client::pool) struct ReclaimAction {
    /// Admission authority that selected reclaim.
    origin: Arc<OriginAdmission>,
    /// Reuse operation completed by the reclaim attempt.
    reuse_id: ReuseId,
    /// Candidate whose `Drop` is the fallback before execution.
    candidate: Option<ReuseCandidate>,
}

impl ReclaimAction {
    /// Attempts logical close outside admission and reports the connection-owning cell result.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let candidate = self
            .candidate
            .take()
            .expect("HTTP/1 reclaim action consumed more than once");
        let outcome = candidate.reclaim();
        OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome)
    }
}

/// Connection-owning-cell completion after an irreversible sender transfer.
pub(in crate::client::pool) struct ConnectionCellCompletion {
    /// Admission authority that owns the remaining operation record.
    origin: Arc<OriginAdmission>,
    /// Operation completed at the connection-owning cell.
    reuse_id: ReuseId,
    /// Cell whose local reservation must be released.
    connection_cell: CellId,
    /// Whether the requesting cell accepted and owns the sender.
    transferred: bool,
    /// Whether `Drop` still owns cell completion.
    active: bool,
}

impl ConnectionCellCompletion {
    /// Completes the connection-owning cell outside admission.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let outcome =
            h1_availability_outcome(&self.origin, &self.connection_cell, |connection_cell| {
                connection_cell.complete_h1_reuse(self.reuse_id, self.transferred)
            });
        let action = OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome);
        self.active = false;
        action
    }
}

impl Drop for ConnectionCellCompletion {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let outcome =
            h1_availability_outcome(&self.origin, &self.connection_cell, |connection_cell| {
                connection_cell.complete_h1_reuse(self.reuse_id, self.transferred)
            });
        let next = OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome);
        OriginAdmission::drive(next);
    }
}

/// Produces a complete connection-owning cell outcome without overloading absence as no-op.
fn h1_availability_outcome(
    origin: &OriginAdmission,
    connection_cell: &CellId,
    report: impl FnOnce(&Arc<OriginCell>) -> H1AvailabilitySnapshot,
) -> H1AvailabilityOutcome {
    match origin.cell(connection_cell) {
        Some(cell) => H1AvailabilityOutcome::reported(connection_cell.clone(), report(&cell)),
        None => H1AvailabilityOutcome::expired(connection_cell.clone()),
    }
}

impl OriginAdmission {
    /// Publishes a connection-owning cell availability change and drives bounded progress.
    pub(in crate::client::pool) fn update_h1_availability(
        origin: &Arc<Self>,
        connection_cell: CellId,
        group: EligibilityGroup,
        snapshot: H1AvailabilitySnapshot,
    ) {
        let action = {
            let mut state = origin.state.lock();
            state
                .h1
                .update_availability(connection_cell, group, snapshot);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Completes a reuse operation whose returning sender was no longer reusable.
    pub(in crate::client::pool) fn reject_returned_h1_reuse(
        origin: &Arc<Self>,
        id: ReuseId,
        connection_cell: CellId,
        snapshot: H1AvailabilitySnapshot,
    ) {
        let action = Self::finish_h1_reuse(
            origin,
            id,
            H1AvailabilityOutcome::reported(connection_cell, snapshot),
        );
        Self::drive(action);
    }

    /// Applies a connection-owning cell reuse operation installation result.
    pub(in crate::client::pool) fn finish_h1_reuse_install(
        origin: &Arc<Self>,
        id: ReuseId,
        connection_cell: CellId,
        result: ReuseInstallResult,
    ) -> Option<AdmissionAction> {
        match result {
            ReuseInstallResult::Rejected(availability) => Self::finish_h1_reuse(
                origin,
                id,
                H1AvailabilityOutcome::reported(connection_cell, availability),
            ),
            ReuseInstallResult::Installed => {
                let mut state = origin.state.lock();
                let record = state.h1.finish_install(id, false);
                if record.as_ref().is_some_and(|record| record.cancelled) {
                    state.h1.cancellations.push_back(id);
                }
                Self::prepare_action(origin, &mut state)
            }
            ReuseInstallResult::Candidate(candidate) => {
                {
                    let mut state = origin.state.lock();
                    let record = state.h1.finish_install(id, true);
                    if record.is_none() {
                        drop(state);
                        drop(candidate);
                        return None;
                    }
                }
                Self::resolve_h1_reuse(origin, id, candidate)
            }
        }
    }

    /// Resolves a provisional sender through borrow or reclaim policy.
    pub(in crate::client::pool) fn resolve_h1_reuse(
        origin: &Arc<Self>,
        id: ReuseId,
        candidate: ReuseCandidate,
    ) -> Option<AdmissionAction> {
        let action = {
            let mut state = origin.state.lock();
            let Some(record) = state.h1.begin_resolution(id) else {
                drop(state);
                drop(candidate);
                return None;
            };
            if record.cancelled
                || !state
                    .demand_schedule
                    .is_current_queued(&record.requesting_cell, record.demand)
            {
                drop(state);
                drop(candidate);
                return None;
            }

            match record.mode {
                ReuseMode::Borrow => {
                    let delivery = state.take_delivery_id();
                    let Some(scheduled) = state.demand_schedule.reserve_reuse_demand(
                        &record.requesting_cell,
                        record.demand,
                        delivery,
                    ) else {
                        drop(state);
                        drop(candidate);
                        return None;
                    };
                    Some(AdmissionAction::Delivery(DeliveryGuard::borrowed_h1(
                        origin.clone(),
                        delivery,
                        scheduled.requesting_cell,
                        scheduled.demand,
                        id,
                        record.connection_cell,
                        candidate,
                    )))
                }
                ReuseMode::Reclaim => {
                    Some(AdmissionAction::H1(H1ReuseAction::Reclaim(ReclaimAction {
                        origin: origin.clone(),
                        reuse_id: id,
                        candidate: Some(candidate),
                    })))
                }
            }
        };
        action
    }

    /// Applies a terminal connection-owning cell outcome and schedules the next admission action.
    fn finish_h1_reuse(
        origin: &Arc<Self>,
        id: ReuseId,
        outcome: H1AvailabilityOutcome,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.h1.finish_reuse(id, outcome);
        Self::prepare_action(origin, &mut state)
    }

    /// Closes a borrow delivery fence and schedules connection-owning cell completion.
    pub(super) fn finish_borrow_delivery(
        origin: &Arc<Self>,
        reuse_id: ReuseId,
        delivery: DeliveryId,
        requesting_cell: &CellId,
        result: DeliveryAckResult,
        transferred_connection_cell: Option<CellId>,
        rejected_outcome: Option<H1AvailabilityOutcome>,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.finish_delivery(delivery, requesting_cell, result);
        if let Some(connection_cell) = transferred_connection_cell {
            return Some(AdmissionAction::H1(H1ReuseAction::CompleteConnectionCell(
                ConnectionCellCompletion {
                    origin: origin.clone(),
                    reuse_id,
                    connection_cell,
                    transferred: true,
                    active: true,
                },
            )));
        }
        state.h1.finish_reuse(
            reuse_id,
            rejected_outcome.expect("rejected borrow had no connection-owning cell outcome"),
        );
        Self::prepare_action(origin, &mut state)
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::{DemandSnapshot, SnapshotVersion};
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::PartitionId;
    use http_1x::uri::Scheme;

    fn cell(index: usize) -> CellId {
        CellId::new(
            PartitionId::from_index(index),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        )
    }

    fn schedule(requesting_cell: CellId, group: EligibilityGroup) -> DemandSchedule {
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            requesting_cell,
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H1Compatible,
                group,
            ),
        );
        schedule
    }

    fn availability_snapshot(
        revision: u64,
        advertised: bool,
        blocked: bool,
    ) -> H1AvailabilitySnapshot {
        H1AvailabilitySnapshot::new(
            revision,
            H1Availability {
                advertised,
                blocked,
            },
        )
    }

    #[test]
    fn availability_publication_keeps_one_bounded_residence() {
        let connection_cell = cell(1);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Reuse::default();

        for revision in 1..=500 {
            coordination.update_availability(
                connection_cell.clone(),
                group.clone(),
                availability_snapshot(revision, true, false),
            );
        }

        assert_eq!(1, coordination.connection_cells.len());
        assert_eq!(1, coordination.origin_order.len());
        assert_eq!(
            1,
            coordination
                .group_orders
                .get(&group)
                .expect("connection-owning cell group was not published")
                .len()
        );

        coordination.update_availability(
            connection_cell.clone(),
            group.clone(),
            availability_snapshot(501, false, false),
        );
        assert_eq!(0, coordination.origin_order.len());
        assert_eq!(
            0,
            coordination
                .group_orders
                .get(&group)
                .expect("connection-owning cell group disappeared")
                .len()
        );
    }

    #[test]
    fn reuse_selection_skips_the_requesting_cell() {
        let requesting_cell = cell(1);
        let peer = cell(2);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            requesting_cell.clone(),
            group.clone(),
            availability_snapshot(1, true, false),
        );
        coordination.update_availability(
            peer.clone(),
            group.clone(),
            availability_snapshot(1, true, false),
        );

        let reuse_id = coordination
            .prepare_reuse(&schedule(requesting_cell.clone(), group))
            .expect("peer connection-owning cell was not selected");
        assert_eq!(peer, reuse_id.connection_cell);
        assert_ne!(requesting_cell, reuse_id.connection_cell);
    }

    #[test]
    fn expired_connection_cell_terminates_its_reuse_without_republication() {
        let connection_cell = cell(1);
        let requesting_cell = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_cell, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_cell.clone(),
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection-owning cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::expired(connection_cell.clone()),
        );

        assert!(!coordination.connection_cells.contains_key(&connection_cell));
        assert!(coordination.prepare_reuse(&schedule).is_none());
        assert!(coordination.operations.is_empty());
    }

    #[test]
    fn stale_terminal_report_cannot_hide_newer_availability() {
        let connection_cell = cell(1);
        let requesting_cell = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_cell, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_cell.clone(),
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection-owning cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_cell.clone(),
                availability_snapshot(3, true, false),
            ),
        );
        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_cell.clone(),
                availability_snapshot(2, false, false),
            ),
        );

        let connection_cell = coordination
            .connection_cells
            .get(&connection_cell)
            .expect("stale report removed the connection-owning cell");
        assert!(connection_cell.advertised);
        assert!(connection_cell.residence.is_available());
        assert_eq!(3, connection_cell.revision);
    }

    #[test]
    fn duplicate_terminal_report_still_refreshes_availability() {
        let connection_cell = cell(1);
        let requesting_cell = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_cell, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_cell.clone(),
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection-owning cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_cell.clone(),
                availability_snapshot(2, true, false),
            ),
        );
        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_cell.clone(),
                availability_snapshot(3, false, false),
            ),
        );

        let connection_cell = coordination
            .connection_cells
            .get(&connection_cell)
            .expect("live availability report removed the connection-owning cell");
        assert!(!connection_cell.advertised);
        assert!(!connection_cell.residence.is_available());
    }
}

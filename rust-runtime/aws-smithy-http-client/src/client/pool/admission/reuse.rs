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
//! Admission owns the operation record. The connection cell retains a
//! local `H1ReuseReservation` serialized with sender return. Values crossing
//! between those lock domains own their fallback: an uninstalled operation is
//! cancelled, and an uncommitted candidate returns its sender before demand
//! becomes schedulable again.
//!
//! Admission advances each operation through these phases:
//!
//! ```text
//! Installing -- reservation installed ----------------------> Installed
//! Installing -- sender extracted ---------------------------> Resolving
//! Installing(cancelled) -- reservation installed -----------> Cancelling
//! Installing(cancelled) -- sender extracted ----------------> Resolving
//! Installing -- reservation rejected or cell expired -------> removed
//! Installed -- sender return intercepted -------------------> Resolving
//! Installed -- demand cancelled ----------------------------> Cancelling
//! Installed -- terminal cell outcome ------------------------> removed
//! Cancelling -- sender return intercepted ------------------> Resolving
//! Cancelling -- reservation cleared -------------------------> removed
//! Resolving -- repeated candidate handoff ------------------> Resolving
//! Resolving -- borrow or reclaim completes ------------------> removed
//! ```
//!
//! A sender extracted during installation takes the `Resolving` path even if
//! cancellation raced with the install acknowledgement. The extracted sender
//! must complete its terminal transition before cancellation can finish.

use super::{
    AdmissionAction, DeliveryAckResult, DeliveryGuard, DeliveryId, DemandId, DemandSchedule,
    IntrusiveLinks, IntrusiveOrder, OriginAdmission,
};
use crate::client::pool::cell::h1::{H1Selection, ProvisionalH1};
use crate::client::pool::cell::OriginCell;
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use crate::sync::Arc;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::collections::{HashMap, VecDeque};
use std::fmt;

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
    connection_partitions: HashMap<PartitionId, H1AvailabilityRecord>,
    /// FIFO view across every cell with a connection available for reclaim.
    origin_order: IntrusiveOrder<PartitionId>,
    /// FIFO views of connections eligible for borrowing.
    group_orders: HashMap<EligibilityGroup, IntrusiveOrder<PartitionId>>,
    /// Nonterminal operations indexed by their never-reused identity.
    operations: HashMap<ReuseId, ReuseOperation>,
    /// Requesting cells that already have a reuse operation in progress.
    requesting_partitions: HashMap<PartitionId, ReuseId>,
    /// Installed operations whose requesting cell became stale.
    cancellations: VecDeque<ReuseId>,
    /// Next reuse operation identity.
    next_reuse: u64,
}

/// Complete HTTP/1 availability reported by a connection cell.
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
    /// Creates one complete connection cell report.
    pub(in crate::client::pool) fn new(revision: u64, availability: H1Availability) -> Self {
        Self {
            revision,
            availability,
        }
    }
}

/// Terminal HTTP/1 availability observed at a connection cell.
pub(in crate::client::pool) enum H1AvailabilityOutcome {
    /// The connection cell remains live and reports its complete availability.
    Reported {
        /// Cell described by `snapshot`.
        connection_partition: PartitionId,
        /// Complete availability after the terminal transition.
        snapshot: H1AvailabilitySnapshot,
    },
    /// The connection cell disappeared before the crossing completed.
    Expired {
        /// Cell no longer retained by the partition registry.
        connection_partition: PartitionId,
    },
}

impl H1AvailabilityOutcome {
    /// Wraps a complete report with the connection cell identity it describes.
    pub(in crate::client::pool) fn reported(
        connection_partition: PartitionId,
        snapshot: H1AvailabilitySnapshot,
    ) -> Self {
        Self::Reported {
            connection_partition,
            snapshot,
        }
    }

    /// Records that a connection cell no longer exists.
    pub(in crate::client::pool) fn expired(connection_partition: PartitionId) -> Self {
        Self::Expired {
            connection_partition,
        }
    }

    /// Returns the connection cell whose admission view must change.
    fn connection_partition(&self) -> &PartitionId {
        match self {
            Self::Reported {
                connection_partition,
                ..
            }
            | Self::Expired {
                connection_partition,
            } => connection_partition,
        }
    }
}

/// Result of installing a reuse reservation at the connection cell.
pub(in crate::client::pool) enum ReuseInstallResult {
    /// A future reusable return will satisfy the reuse operation.
    Installed,
    /// An idle sender was extracted immediately.
    Candidate(ReuseCandidate),
    /// The connection cell could not retain the reuse operation.
    Rejected(H1AvailabilitySnapshot),
}

/// Admission's current view of one HTTP/1 connection cell.
#[derive(Debug)]
struct H1AvailabilityRecord {
    /// Reuse group whose peers may borrow this connection cell's sender.
    group: EligibilityGroup,
    /// Newest connection cell report accepted by admission.
    revision: u64,
    /// Whether the cell has an H1 record that can return or be reclaimed.
    advertised: bool,
    /// Reuse operation currently reserving this cell's connection.
    reuse_id: Option<ReuseId>,
    /// Whether connection cell-local work temporarily excludes peers.
    blocked: bool,
    /// Linked scheduling residence while this connection cell is selectable.
    residence: H1AvailabilityResidence,
}

impl H1AvailabilityRecord {
    /// Returns whether this connection cell must occupy both scheduling views.
    fn is_schedulable(&self) -> bool {
        self.advertised && !self.blocked && self.reuse_id.is_none()
    }
}

/// Whether a connection cell is linked in both scheduling views.
#[derive(Debug, Default)]
enum H1AvailabilityResidence {
    /// The connection cell is absent from connection cell-selection order.
    #[default]
    Unavailable,
    /// The connection cell is linked once in origin and group order.
    Available {
        /// Links in origin-wide reclaim order.
        origin: IntrusiveLinks<PartitionId>,
        /// Links in eligibility-group borrow order.
        group: IntrusiveLinks<PartitionId>,
    },
}

impl H1AvailabilityResidence {
    /// Returns whether the connection cell is linked in both views.
    fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Returns this residence's links for one scheduling view.
    fn links(&self, view: AvailabilityOrderView) -> Option<&IntrusiveLinks<PartitionId>> {
        match (self, view) {
            (Self::Available { origin, .. }, AvailabilityOrderView::Reclaim) => Some(origin),
            (Self::Available { group, .. }, AvailabilityOrderView::Borrow) => Some(group),
            (Self::Unavailable, _) => None,
        }
    }

    /// Returns mutable links while repairing one scheduling view.
    fn links_mut(&mut self, view: AvailabilityOrderView) -> &mut IntrusiveLinks<PartitionId> {
        match (self, view) {
            (Self::Available { origin, .. }, AvailabilityOrderView::Reclaim) => origin,
            (Self::Available { group, .. }, AvailabilityOrderView::Borrow) => group,
            (Self::Unavailable, _) => {
                panic!("unavailable connection cell has no order links")
            }
        }
    }
}

/// Selects one of the two intrusive connection cell-order link sets.
#[derive(Clone, Copy)]
enum AvailabilityOrderView {
    /// Origin-wide connection-reclaim order.
    Reclaim,
    /// Eligibility-group connection-borrow order.
    Borrow,
}

/// Admission-owned state for one nonterminal reuse operation.
#[derive(Clone, Debug)]
struct ReuseOperation {
    /// Cell whose connection is reserved by this operation.
    connection_partition: PartitionId,
    /// Cell whose demand caused this operation.
    requesting_partition: PartitionId,
    /// Exact requesting cell demand generation fenced by the reuse operation.
    demand: DemandId,
    /// Whether resolution borrows a sender or reclaims its capacity.
    mode: ReuseMode,
    /// Admission-side progress of the reuse operation.
    phase: ReusePhase,
    /// Whether requesting cell demand became stale before connection-cell
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
    pub(in crate::client::pool) connection_partition: PartitionId,
}

/// Work required to cancel a reservation outside the admission lock.
pub(super) struct PreparedReuseCancellation {
    /// Operation whose reservation must be cleared.
    id: ReuseId,
    /// Cell that owns the reservation.
    connection_partition: PartitionId,
}

impl H1Reuse {
    /// Publishes the connection cell's complete current availability.
    pub(super) fn update_availability(
        &mut self,
        connection_partition: PartitionId,
        group: EligibilityGroup,
        snapshot: H1AvailabilitySnapshot,
    ) {
        if self
            .connection_partitions
            .get(&connection_partition)
            .is_some_and(|record| record.revision >= snapshot.revision)
        {
            return;
        }
        self.unlink_cell(&connection_partition);
        let record = self
            .connection_partitions
            .entry(connection_partition)
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
        self.enqueue_cell_if_available(&connection_partition);
        self.assert_consistent();
    }

    /// Marks a requesting cell reuse operation stale after demand publication or delivery.
    pub(super) fn reconcile_requesting_cell(
        &mut self,
        requesting_partition: &PartitionId,
        schedule: &DemandSchedule,
    ) {
        let Some(reuse_id) = self
            .requesting_partitions
            .get(requesting_partition)
            .copied()
        else {
            return;
        };
        let Some(record) = self.operations.get_mut(&reuse_id) else {
            self.requesting_partitions.remove(requesting_partition);
            self.assert_consistent();
            return;
        };
        if schedule.is_current_queued(&record.requesting_partition, record.demand) {
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
    /// so its capacity can satisfy the same demand. HTTP/1-compatible demand
    /// does not select its own cell because local senders are handled under the
    /// cell lock. HTTP/2-only demand may reclaim same-cell idle HTTP/1 capacity.
    pub(super) fn prepare_reuse(
        &mut self,
        schedule: &DemandSchedule,
    ) -> Option<PreparedReuseInstall> {
        let requesting_partition = schedule.queued_head()?;
        if self
            .requesting_partitions
            .contains_key(&requesting_partition.requesting_partition)
        {
            return None;
        }

        let (connection_partition, mode) = if requesting_partition.requirement.accepts_h1() {
            match self.take_group_peer(
                &requesting_partition.eligibility_group,
                &requesting_partition.requesting_partition,
            ) {
                Some(connection_partition) => (connection_partition, ReuseMode::Borrow),
                None => (
                    self.take_origin_peer(&requesting_partition.requesting_partition)?,
                    ReuseMode::Reclaim,
                ),
            }
        } else {
            (self.take_origin_head()?, ReuseMode::Reclaim)
        };

        let id = self.take_reuse_id();
        let availability_record = self
            .connection_partitions
            .get_mut(&connection_partition)
            .expect("selected HTTP/1 connection cell disappeared");
        debug_assert!(availability_record.reuse_id.is_none());
        availability_record.reuse_id = Some(id);
        self.requesting_partitions
            .insert(requesting_partition.requesting_partition, id);
        self.operations.insert(
            id,
            ReuseOperation {
                connection_partition,
                requesting_partition: requesting_partition.requesting_partition,
                demand: requesting_partition.demand,
                mode,
                phase: ReusePhase::Installing,
                cancelled: false,
            },
        );
        self.assert_consistent();
        Some(PreparedReuseInstall {
            id,
            connection_partition,
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
                connection_partition: record.connection_partition,
            });
        }
        None
    }

    /// Applies an install acknowledgement and returns the retained reuse operation.
    fn finish_install(&mut self, id: ReuseId, resolved: bool) -> Option<ReuseOperation> {
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
    fn begin_resolution(&mut self, id: ReuseId) -> Option<ReuseOperation> {
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

    /// Removes one reuse operation and applies the connection cell's terminal report.
    ///
    /// The outcome names its connection cell explicitly so an expired cell cannot be
    /// mistaken for an unchanged connection cell and a duplicate terminal report still
    /// refreshes admission's connection cell view.
    fn finish_reuse(
        &mut self,
        id: ReuseId,
        outcome: H1AvailabilityOutcome,
    ) -> Option<ReuseOperation> {
        let outcome_cell = *outcome.connection_partition();
        self.apply_h1_availability_outcome(outcome);

        let record = self.operations.remove(&id);
        if let Some(record) = record.as_ref() {
            debug_assert_eq!(record.connection_partition, outcome_cell);
            if self.requesting_partitions.get(&record.requesting_partition) == Some(&id) {
                self.requesting_partitions
                    .remove(&record.requesting_partition);
            }
            if let Some(connection_partition) = self
                .connection_partitions
                .get_mut(&record.connection_partition)
            {
                if connection_partition.reuse_id == Some(id) {
                    connection_partition.reuse_id = None;
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
                connection_partition,
                snapshot,
            } => {
                if self
                    .connection_partitions
                    .get(&connection_partition)
                    .is_some_and(|record| record.revision >= snapshot.revision)
                {
                    return;
                }
                self.unlink_cell(&connection_partition);
                if let Some(record) = self.connection_partitions.get_mut(&connection_partition) {
                    record.revision = snapshot.revision;
                    record.advertised = snapshot.availability.advertised;
                    record.blocked = snapshot.availability.blocked;
                }
            }
            H1AvailabilityOutcome::Expired {
                connection_partition,
            } => {
                self.unlink_cell(&connection_partition);
                self.connection_partitions.remove(&connection_partition);
            }
        }
    }

    /// Removes and returns the first eligible peer in one reuse group.
    fn take_group_peer(
        &mut self,
        group: &EligibilityGroup,
        requesting_partition: &PartitionId,
    ) -> Option<PartitionId> {
        let connection_partition = {
            let order = self.group_orders.get(group)?;
            self.first_peer(order, requesting_partition, AvailabilityOrderView::Borrow)?
        };
        self.unlink_cell(&connection_partition);
        Some(connection_partition)
    }

    /// Removes and returns the first origin-wide peer.
    fn take_origin_peer(&mut self, requesting_partition: &PartitionId) -> Option<PartitionId> {
        let connection_partition = self.first_peer(
            &self.origin_order,
            requesting_partition,
            AvailabilityOrderView::Reclaim,
        )?;
        self.unlink_cell(&connection_partition);
        Some(connection_partition)
    }

    /// Removes and returns the oldest origin-wide connection, including local.
    fn take_origin_head(&mut self) -> Option<PartitionId> {
        let connection_partition = self.origin_order.head()?;
        self.unlink_cell(&connection_partition);
        Some(connection_partition)
    }

    /// Returns the head, or its successor when the head is the requesting cell itself.
    fn first_peer(
        &self,
        order: &IntrusiveOrder<PartitionId>,
        requesting_partition: &PartitionId,
        view: AvailabilityOrderView,
    ) -> Option<PartitionId> {
        let head = order.head()?;
        if head != *requesting_partition {
            return Some(head);
        }
        let record = self
            .connection_partitions
            .get(&head)
            .expect("connection cell order head disappeared");
        record.residence.links(view).and_then(|links| links.next)
    }

    /// Appends an available, unreserved cell to both scheduling views.
    fn enqueue_cell_if_available(&mut self, connection_partition: &PartitionId) {
        let Some(record) = self.connection_partitions.get(connection_partition) else {
            return;
        };
        if !record.is_schedulable() || record.residence.is_available() {
            return;
        }
        let group = record.group.clone();
        let origin_links = self.origin_order.push_back(*connection_partition);
        let group_links = self
            .group_orders
            .entry(group.clone())
            .or_default()
            .push_back(*connection_partition);

        if let Some(previous) = origin_links.previous {
            self.connection_partitions
                .get_mut(&previous)
                .expect("origin connection cell order tail disappeared")
                .residence
                .links_mut(AvailabilityOrderView::Reclaim)
                .next = Some(*connection_partition);
        }
        if let Some(previous) = group_links.previous {
            self.connection_partitions
                .get_mut(&previous)
                .expect("group connection cell order tail disappeared")
                .residence
                .links_mut(AvailabilityOrderView::Borrow)
                .next = Some(*connection_partition);
        }

        self.connection_partitions
            .get_mut(connection_partition)
            .expect("enqueued connection cell disappeared")
            .residence = H1AvailabilityResidence::Available {
            origin: origin_links,
            group: group_links,
        };
    }

    /// Unlinks a connection cell eagerly from both scheduling views.
    fn unlink_cell(&mut self, connection_partition: &PartitionId) {
        let Some(record) = self.connection_partitions.get_mut(connection_partition) else {
            return;
        };
        let residence = std::mem::take(&mut record.residence);
        let H1AvailabilityResidence::Available { origin, group } = residence else {
            return;
        };
        let group_key = record.group.clone();

        Self::repair_links(
            &mut self.connection_partitions,
            connection_partition,
            &origin,
            AvailabilityOrderView::Reclaim,
        );
        self.origin_order.remove(*connection_partition, origin);

        Self::repair_links(
            &mut self.connection_partitions,
            connection_partition,
            &group,
            AvailabilityOrderView::Borrow,
        );
        self.group_orders
            .get_mut(&group_key)
            .expect("available connection cell lost its group order")
            .remove(*connection_partition, group);
    }

    /// Repairs neighboring links after one connection cell leaves an order.
    fn repair_links(
        connection_partitions: &mut HashMap<PartitionId, H1AvailabilityRecord>,
        connection_partition: &PartitionId,
        links: &IntrusiveLinks<PartitionId>,
        view: AvailabilityOrderView,
    ) {
        if let Some(previous) = links.previous {
            connection_partitions
                .get_mut(&previous)
                .expect("previous connection cell disappeared")
                .residence
                .links_mut(view)
                .next = links.next;
        }
        if let Some(next) = links.next {
            connection_partitions
                .get_mut(&next)
                .expect("next connection cell disappeared")
                .residence
                .links_mut(view)
                .previous = links.previous;
        }
        debug_assert_ne!(links.previous.as_ref(), Some(connection_partition));
        debug_assert_ne!(links.next.as_ref(), Some(connection_partition));
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
        {
            if std::thread::panicking() {
                return;
            }
            self.assert_consistent_debug();
        }
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        for (id, operation) in &self.operations {
            assert_eq!(
                self.requesting_partitions
                    .get(&operation.requesting_partition),
                Some(id),
                "reuse operation's requesting-cell index did not name the operation"
            );
            assert_eq!(
                self.connection_partitions
                    .get(&operation.connection_partition)
                    .and_then(|connection_partition| connection_partition.reuse_id),
                Some(*id),
                "reuse operation's connection-cell index did not name the operation"
            );
        }
        for (requesting_partition, id) in &self.requesting_partitions {
            assert_eq!(
                self.operations
                    .get(id)
                    .map(|operation| &operation.requesting_partition),
                Some(requesting_partition),
                "requesting cell index named a missing reuse operation"
            );
        }
        for (connection_partition, record) in &self.connection_partitions {
            if let Some(id) = record.reuse_id {
                assert_eq!(
                    self.operations
                        .get(&id)
                        .map(|operation| &operation.connection_partition),
                    Some(connection_partition),
                    "connection cell index named a missing reuse operation"
                );
            }
            assert_eq!(
                record.residence.is_available(),
                record.is_schedulable(),
                "connection cell scheduling residence did not match availability"
            );
        }
        self.assert_order(&self.origin_order, None, AvailabilityOrderView::Reclaim);
        for (group, order) in &self.group_orders {
            self.assert_order(order, Some(group), AvailabilityOrderView::Borrow);
        }
    }

    #[cfg(debug_assertions)]
    fn assert_order(
        &self,
        order: &IntrusiveOrder<PartitionId>,
        expected_group: Option<&EligibilityGroup>,
        view: AvailabilityOrderView,
    ) {
        let expected = self
            .connection_partitions
            .values()
            .filter(|record| {
                record.residence.is_available()
                    && expected_group.is_none_or(|group| &record.group == group)
            })
            .count();
        order.assert_consistent(
            expected,
            self.connection_partitions.len(),
            "HTTP/1 connection cell order",
            |connection_partition| {
                let record = self
                    .connection_partitions
                    .get(&connection_partition)
                    .expect("ordered HTTP/1 connection cell disappeared");
                if let Some(group) = expected_group {
                    assert_eq!(
                        &record.group, group,
                        "connection cell appeared in the wrong eligibility order"
                    );
                }
                *record
                    .residence
                    .links(view)
                    .expect("ordered connection cell lost its links")
            },
        );
    }
}

/// One unlocked HTTP/1 coordination step.
pub(in crate::client::pool) enum H1ReuseAction {
    /// Install a prepared reservation in its connection cell.
    Install(ReuseInstallAction),
    /// Cancel an installed reservation.
    Cancel(ReuseCancelAction),
    /// Close a selected connection and release its capacity.
    Reclaim(ReclaimAction),
    /// Complete the connection cell after a sender transfer.
    CompleteConnectionCell(ConnectionCellCompletion),
}

impl H1ReuseAction {
    /// Creates an unlocked connection cell installation crossing.
    pub(super) fn install(origin: Arc<OriginAdmission>, prepared: PreparedReuseInstall) -> Self {
        Self::Install(ReuseInstallAction {
            origin,
            install: Some(prepared),
        })
    }

    /// Creates an unlocked connection cell-cancellation crossing.
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

/// Reservation installation crossing from admission to a connection cell.
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
        let Some(connection_partition) = self.origin.cell(&prepared.connection_partition) else {
            return OriginAdmission::finish_h1_reuse(
                &self.origin,
                prepared.id,
                H1AvailabilityOutcome::expired(prepared.connection_partition),
            );
        };
        OriginCell::install_h1_reuse(&connection_partition, self.origin.clone(), prepared)
    }
}

impl Drop for ReuseInstallAction {
    fn drop(&mut self) {
        if let Some(prepared) = self.install.take() {
            let outcome = h1_availability_outcome(
                &self.origin,
                &prepared.connection_partition,
                |connection_partition| connection_partition.cancel_h1_reuse(prepared.id),
            );
            let next = OriginAdmission::finish_h1_reuse(&self.origin, prepared.id, outcome);
            OriginAdmission::drive(next);
        }
    }
}

/// Installed reservation cancellation crossing to its connection cell.
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
            &cancellation.connection_partition,
            |connection_partition| connection_partition.cancel_h1_reuse(cancellation.id),
        );
        OriginAdmission::finish_h1_reuse(&self.origin, cancellation.id, outcome)
    }
}

impl Drop for ReuseCancelAction {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            let outcome = h1_availability_outcome(
                &self.origin,
                &cancellation.connection_partition,
                |connection_partition| connection_partition.cancel_h1_reuse(cancellation.id),
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
    connection_partition: PartitionId,
    /// Sender fallback while connection cell resolution is incomplete.
    provisional: Option<ProvisionalH1>,
}

impl ReuseCandidate {
    /// Takes provisional sender ownership for one resolving connection cell reuse operation.
    pub(in crate::client::pool) fn new(
        origin: Arc<OriginAdmission>,
        reuse_id: ReuseId,
        connection_partition: PartitionId,
        provisional: ProvisionalH1,
    ) -> Self {
        Self {
            origin,
            reuse_id,
            connection_partition,
            provisional: Some(provisional),
        }
    }

    /// Returns the connection selected by this reuse operation.
    fn connection_id(&self) -> ConnectionId {
        self.provisional
            .as_ref()
            .expect("HTTP/1 reuse operation candidate consumed more than once")
            .connection_id()
    }

    /// Revalidates the cell reservation and turns the sender into a selection.
    ///
    /// Failure returns this guard intact so dropping it restores the sender to
    /// its owning cell and completes the reuse operation exactly once.
    pub(in crate::client::pool) fn commit(mut self) -> Result<H1Selection, Self> {
        let Some(connection_partition) = self.origin.cell(&self.connection_partition) else {
            return Err(self);
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 reuse operation candidate consumed more than once");
        match OriginCell::commit_h1_reuse(&connection_partition, self.reuse_id, provisional) {
            Ok(selection) => Ok(selection),
            Err(provisional) => {
                self.provisional = Some(provisional);
                Err(self)
            }
        }
    }

    /// Attempts reclaim and reports its terminal cell outcome and close result.
    fn reclaim(mut self) -> (H1AvailabilityOutcome, bool) {
        let Some(connection_partition) = self.origin.cell(&self.connection_partition) else {
            drop(self.provisional.take());
            return (
                H1AvailabilityOutcome::expired(self.connection_partition),
                false,
            );
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 reuse operation candidate consumed more than once");
        let (availability, reclaimed) =
            match OriginCell::reclaim_h1_reuse(&connection_partition, self.reuse_id, provisional) {
                Ok(result) => result,
                Err(provisional) => (
                    OriginCell::reject_h1_reuse_candidate(
                        &connection_partition,
                        self.reuse_id,
                        provisional,
                    ),
                    false,
                ),
            };
        (
            H1AvailabilityOutcome::reported(self.connection_partition, availability),
            reclaimed,
        )
    }
}

impl fmt::Debug for ReuseCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReuseCandidate")
            .field("reuse_id", &self.reuse_id)
            .field("connection_partition", &self.connection_partition)
            .field("provisional", &self.provisional)
            .finish_non_exhaustive()
    }
}

impl Drop for ReuseCandidate {
    fn drop(&mut self) {
        let Some(provisional) = self.provisional.take() else {
            return;
        };
        let outcome = match self.origin.cell(&self.connection_partition) {
            Some(connection_partition) => H1AvailabilityOutcome::reported(
                self.connection_partition,
                OriginCell::reject_h1_reuse_candidate(
                    &connection_partition,
                    self.reuse_id,
                    provisional,
                ),
            ),
            None => {
                drop(provisional);
                H1AvailabilityOutcome::expired(self.connection_partition)
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
    /// Cell whose demand receives capacity released by reclaim.
    requesting_partition: PartitionId,
    /// Reuse operation completed by the reclaim attempt.
    reuse_id: ReuseId,
    /// Candidate whose `Drop` is the fallback before execution.
    candidate: Option<ReuseCandidate>,
}

impl ReclaimAction {
    /// Attempts logical close outside admission and reports the connection cell result.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let candidate = self
            .candidate
            .take()
            .expect("HTTP/1 reclaim action consumed more than once");
        let connection_id = candidate.connection_id();
        let connection_partition = candidate.connection_partition;
        let (outcome, reclaimed) = candidate.reclaim();
        if reclaimed {
            tracing::trace!(
                connection_id = %connection_id,
                request_partition = ?self.requesting_partition,
                connection_partition = ?connection_partition,
                origin_scheme = %self.origin.origin().scheme(),
                origin_host = self.origin.origin().host(),
                origin_port = ?self.origin.origin().port(),
                "HTTP/1 connection reclaimed for peer demand"
            );
        }
        OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome)
    }
}

/// Connection-owning-cell completion after an irreversible sender transfer.
pub(in crate::client::pool) struct ConnectionCellCompletion {
    /// Admission authority that owns the remaining operation record.
    origin: Arc<OriginAdmission>,
    /// Operation completed at the connection cell.
    reuse_id: ReuseId,
    /// Cell whose local reservation must be released.
    connection_partition: PartitionId,
    /// Whether the requesting cell accepted and owns the sender.
    transferred: bool,
    /// Whether `Drop` still owns cell completion.
    active: bool,
}

impl ConnectionCellCompletion {
    /// Completes the connection cell outside admission.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let outcome = h1_availability_outcome(
            &self.origin,
            &self.connection_partition,
            |connection_partition| {
                connection_partition.complete_h1_reuse(self.reuse_id, self.transferred)
            },
        );
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
        let outcome = h1_availability_outcome(
            &self.origin,
            &self.connection_partition,
            |connection_partition| {
                connection_partition.complete_h1_reuse(self.reuse_id, self.transferred)
            },
        );
        let next = OriginAdmission::finish_h1_reuse(&self.origin, self.reuse_id, outcome);
        OriginAdmission::drive(next);
    }
}

/// Produces a complete connection cell outcome without overloading absence as no-op.
fn h1_availability_outcome(
    origin: &OriginAdmission,
    connection_partition: &PartitionId,
    report: impl FnOnce(&Arc<OriginCell>) -> H1AvailabilitySnapshot,
) -> H1AvailabilityOutcome {
    match origin.cell(connection_partition) {
        Some(cell) => H1AvailabilityOutcome::reported(*connection_partition, report(&cell)),
        None => H1AvailabilityOutcome::expired(*connection_partition),
    }
}

impl OriginAdmission {
    /// Publishes a connection cell availability change and drives bounded progress.
    pub(in crate::client::pool) fn update_h1_availability(
        origin: &Arc<Self>,
        connection_partition: PartitionId,
        group: EligibilityGroup,
        snapshot: H1AvailabilitySnapshot,
    ) {
        let action = {
            let mut state = origin.state.lock();
            state
                .h1
                .update_availability(connection_partition, group, snapshot);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Completes a reuse operation whose returning sender was no longer reusable.
    pub(in crate::client::pool) fn reject_returned_h1_reuse(
        origin: &Arc<Self>,
        id: ReuseId,
        connection_partition: PartitionId,
        snapshot: H1AvailabilitySnapshot,
    ) {
        let action = Self::finish_h1_reuse(
            origin,
            id,
            H1AvailabilityOutcome::reported(connection_partition, snapshot),
        );
        Self::drive(action);
    }

    /// Applies a connection cell reuse operation installation result.
    pub(in crate::client::pool) fn finish_h1_reuse_install(
        origin: &Arc<Self>,
        id: ReuseId,
        connection_partition: PartitionId,
        result: ReuseInstallResult,
    ) -> Option<AdmissionAction> {
        match result {
            ReuseInstallResult::Rejected(availability) => Self::finish_h1_reuse(
                origin,
                id,
                H1AvailabilityOutcome::reported(connection_partition, availability),
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
                    .is_current_queued(&record.requesting_partition, record.demand)
            {
                drop(state);
                drop(candidate);
                return None;
            }

            match record.mode {
                ReuseMode::Borrow => {
                    let delivery = state.take_delivery_id();
                    let old_group = state
                        .demand_schedule
                        .group_for(&record.requesting_partition);
                    let Some(scheduled) = state.demand_schedule.reserve_reuse_demand(
                        &record.requesting_partition,
                        record.demand,
                        delivery,
                    ) else {
                        drop(state);
                        drop(candidate);
                        return None;
                    };
                    state.reconcile_demand_indexes(&scheduled.requesting_partition, old_group);
                    Some(AdmissionAction::Delivery(DeliveryGuard::borrowed_h1(
                        origin.clone(),
                        delivery,
                        scheduled.requesting_partition,
                        scheduled.demand,
                        id,
                        record.connection_partition,
                        candidate,
                    )))
                }
                ReuseMode::Reclaim => {
                    Some(AdmissionAction::H1(H1ReuseAction::Reclaim(ReclaimAction {
                        origin: origin.clone(),
                        requesting_partition: record.requesting_partition,
                        reuse_id: id,
                        candidate: Some(candidate),
                    })))
                }
            }
        };
        action
    }

    /// Applies a terminal connection cell outcome and schedules the next admission action.
    fn finish_h1_reuse(
        origin: &Arc<Self>,
        id: ReuseId,
        outcome: H1AvailabilityOutcome,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.h1.finish_reuse(id, outcome);
        Self::prepare_action(origin, &mut state)
    }

    /// Closes a borrow delivery fence and schedules connection cell completion.
    pub(super) fn finish_borrow_delivery(
        origin: &Arc<Self>,
        reuse_id: ReuseId,
        delivery: DeliveryId,
        requesting_partition: &PartitionId,
        result: DeliveryAckResult,
        transferred_connection_cell: Option<PartitionId>,
        rejected_outcome: Option<H1AvailabilityOutcome>,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.finish_delivery(delivery, requesting_partition, result);
        if let Some(connection_partition) = transferred_connection_cell {
            return Some(AdmissionAction::H1(H1ReuseAction::CompleteConnectionCell(
                ConnectionCellCompletion {
                    origin: origin.clone(),
                    reuse_id,
                    connection_partition,
                    transferred: true,
                    active: true,
                },
            )));
        }
        state.h1.finish_reuse(
            reuse_id,
            rejected_outcome.expect("rejected borrow had no connection cell outcome"),
        );
        Self::prepare_action(origin, &mut state)
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::{DemandSnapshot, ProtocolRequirement, SnapshotVersion};
    use crate::client::pool::partition::PartitionId;

    fn cell(index: usize) -> PartitionId {
        PartitionId::from_index(index)
    }

    fn schedule(requesting_partition: PartitionId, group: EligibilityGroup) -> DemandSchedule {
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            requesting_partition,
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
        let connection_partition = cell(1);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Reuse::default();

        for revision in 1..=500 {
            coordination.update_availability(
                connection_partition,
                group.clone(),
                availability_snapshot(revision, true, false),
            );
        }

        assert_eq!(1, coordination.connection_partitions.len());
        assert_eq!(1, coordination.origin_order.len());
        assert_eq!(
            1,
            coordination
                .group_orders
                .get(&group)
                .expect("connection cell group was not published")
                .len()
        );

        coordination.update_availability(
            connection_partition,
            group.clone(),
            availability_snapshot(501, false, false),
        );
        assert_eq!(0, coordination.origin_order.len());
        assert_eq!(
            0,
            coordination
                .group_orders
                .get(&group)
                .expect("connection cell group disappeared")
                .len()
        );
    }

    #[test]
    fn h2_required_demand_reclaims_local_h1_capacity() {
        let requesting_partition = cell(1);
        let group = EligibilityGroup::Pool;
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            requesting_partition,
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H2Required,
                group.clone(),
            ),
        );
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            requesting_partition,
            group,
            availability_snapshot(1, true, false),
        );

        let prepared = coordination
            .prepare_reuse(&schedule)
            .expect("local HTTP/1 capacity was not selected for reclaim");

        assert_eq!(requesting_partition, prepared.connection_partition);
        assert_eq!(
            ReuseMode::Reclaim,
            coordination.operations[&prepared.id].mode
        );
    }

    #[test]
    fn reuse_selection_skips_the_requesting_cell() {
        let requesting_partition = cell(1);
        let peer = cell(2);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            requesting_partition,
            group.clone(),
            availability_snapshot(1, true, false),
        );
        coordination.update_availability(
            peer,
            group.clone(),
            availability_snapshot(1, true, false),
        );

        let reuse_id = coordination
            .prepare_reuse(&schedule(requesting_partition, group))
            .expect("peer connection cell was not selected");
        assert_eq!(peer, reuse_id.connection_partition);
        assert_ne!(requesting_partition, reuse_id.connection_partition);
    }

    #[test]
    fn expired_connection_cell_terminates_its_reuse_without_republication() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_partition,
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::expired(connection_partition),
        );

        assert!(!coordination
            .connection_partitions
            .contains_key(&connection_partition));
        assert!(coordination.prepare_reuse(&schedule).is_none());
        assert!(coordination.operations.is_empty());
    }

    #[test]
    fn stale_terminal_report_cannot_hide_newer_availability() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_partition,
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_partition,
                availability_snapshot(3, true, false),
            ),
        );
        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_partition,
                availability_snapshot(2, false, false),
            ),
        );

        let connection_partition = coordination
            .connection_partitions
            .get(&connection_partition)
            .expect("stale report removed the connection cell");
        assert!(connection_partition.advertised);
        assert!(connection_partition.residence.is_available());
        assert_eq!(3, connection_partition.revision);
    }

    #[test]
    fn duplicate_terminal_report_still_refreshes_availability() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut coordination = H1Reuse::default();
        coordination.update_availability(
            connection_partition,
            group,
            availability_snapshot(1, true, false),
        );
        let reuse_id = coordination
            .prepare_reuse(&schedule)
            .expect("connection cell did not produce a reuse operation");

        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_partition,
                availability_snapshot(2, true, false),
            ),
        );
        coordination.finish_reuse(
            reuse_id.id,
            H1AvailabilityOutcome::reported(
                connection_partition,
                availability_snapshot(3, false, false),
            ),
        );

        let connection_partition = coordination
            .connection_partitions
            .get(&connection_partition)
            .expect("live availability report removed the connection cell");
        assert!(!connection_partition.advertised);
        assert!(!connection_partition.residence.is_available());
    }
}

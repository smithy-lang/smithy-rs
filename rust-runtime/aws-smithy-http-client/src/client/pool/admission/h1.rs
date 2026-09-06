/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Admission's view of HTTP/1 connection supply for one bounded origin.
//!
//! [`H1Supply`] indexes cells with selectable HTTP/1 senders and pairs the oldest
//! origin demand with one supplier. Compatible demand borrows the sender.
//! Incompatible demand closes the selected connection to recover capacity.
//!
//! Admission retains an [`H1Match`] while supplier reservation crosses locks or
//! waits for a sender return. The match owns identities and phase flags only.
//! Detached values own resources and their drop fallback.
//!
//! Admission advances each match through these phases:
//!
//! ```text
//! Reserving -- reservation installed -----------------------> WaitingForSender
//! Reserving -- sender extracted ----------------------------> Resolving
//! Reserving(cancelled) -- reservation installed ------------> Cancelling
//! Reserving(cancelled) -- sender extracted -----------------> Resolving
//! Reserving -- reservation rejected or cell expired --------> removed
//! WaitingForSender -- sender return intercepted ------------> Resolving
//! WaitingForSender -- demand cancelled ---------------------> Cancelling
//! WaitingForSender -- terminal supplier outcome ------------> removed
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
    AdmissionAction, DeliveryGuard, DemandAssignmentOutcome, DemandId, DemandSchedule,
    IntrusiveLinks, IntrusiveOrder, OriginAdmission, SupplyRevision,
};
use crate::client::pool::cell::h1::{H1Selection, ProvisionalH1};
use crate::client::pool::cell::OriginCell;
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use crate::sync::Arc;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::collections::{HashMap, VecDeque};
use std::fmt;

/// Identity of one retained HTTP/1 demand-to-supplier match.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::client::pool) struct H1MatchId(u64);

impl H1MatchId {
    /// Creates a deterministic identity for focused transition tests.
    #[cfg(test)]
    pub(in crate::client::pool) const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// How one retained HTTP/1 match satisfies waiting demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) enum H1MatchKind {
    /// Move the selected sender to the requesting cell for dispatch.
    BorrowSender,
    /// Close the selected connection and deliver its capacity instead.
    ReclaimCapacity,
}

/// Origin-locked HTTP/1 supply indexes and retained cross-lock matches.
#[derive(Debug, Default)]
pub(super) struct H1Supply {
    index: H1SupplyIndex,
    matches: HashMap<H1MatchId, H1Match>,
    by_requester: HashMap<PartitionId, H1MatchId>,
    /// Lazy cancellation queue; resolution may leave a tombstone to discard.
    cancellations: VecDeque<H1MatchId>,
    next_match_id: u64,
}

/// Admission-facing HTTP/1 status derived under one cell lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct H1SupplyStatus {
    pub(in crate::client::pool) has_returnable_connection: bool,
    pub(in crate::client::pool) peer_use_blocked: bool,
}

/// Terminal HTTP/1 supply observed at a supplier cell.
pub(in crate::client::pool) enum H1SupplyOutcome {
    SupplierLive {
        supplier: PartitionId,
        revision: SupplyRevision<H1SupplyStatus>,
    },
    SupplierExpired {
        supplier: PartitionId,
    },
}

impl H1SupplyOutcome {
    pub(in crate::client::pool) fn supplier_live(
        supplier: PartitionId,
        revision: SupplyRevision<H1SupplyStatus>,
    ) -> Self {
        Self::SupplierLive { supplier, revision }
    }

    pub(in crate::client::pool) fn supplier_expired(supplier: PartitionId) -> Self {
        Self::SupplierExpired { supplier }
    }

    fn supplier(&self) -> &PartitionId {
        match self {
            Self::SupplierLive { supplier, .. } | Self::SupplierExpired { supplier } => supplier,
        }
    }
}

/// Supplier-cell decision for one H1 reservation attempt.
pub(in crate::client::pool) enum H1ReservationDecision<C> {
    /// A future reusable return will satisfy the retained match.
    Installed,
    /// An idle sender was extracted immediately.
    Candidate(C),
    /// The connection cell could not reserve supply for the retained match.
    Rejected(SupplyRevision<H1SupplyStatus>),
}

/// Admission's indexed view of supplier cells exposing selectable H1 supply.
#[derive(Debug, Default)]
struct H1SupplyIndex {
    records: HashMap<PartitionId, H1SupplyRecord>,
    reclaim_order: IntrusiveOrder<PartitionId>,
    suppliers_by_group: HashMap<EligibilityGroup, IntrusiveOrder<PartitionId>>,
}

/// Admission's current view of one HTTP/1 supplier cell.
#[derive(Debug)]
struct H1SupplyRecord {
    /// Reuse group whose peers may borrow this connection cell's sender.
    eligibility_group: EligibilityGroup,
    /// Newest connection cell report accepted by admission.
    revision: u64,
    /// Whether the cell has an H1 record that can return or be reclaimed.
    status: H1SupplyStatus,
    reserved_by: Option<H1MatchId>,
    index_state: H1SupplyIndexState,
}

impl H1SupplyRecord {
    /// Returns whether this connection cell must occupy both scheduling views.
    fn is_selectable(&self) -> bool {
        self.status.has_returnable_connection
            && !self.status.peer_use_blocked
            && self.reserved_by.is_none()
    }
}

/// Whether a connection cell is linked in both scheduling views.
#[derive(Debug, Default)]
enum H1SupplyIndexState {
    /// The connection cell is absent from connection cell-selection order.
    #[default]
    Unlinked,
    /// The connection cell is linked once in origin and group order.
    Linked {
        /// Links in origin-wide reclaim order.
        reclaim: IntrusiveLinks<PartitionId>,
        /// Links in eligibility-group borrow order.
        group: IntrusiveLinks<PartitionId>,
    },
}

impl H1SupplyIndexState {
    fn is_linked(&self) -> bool {
        matches!(self, Self::Linked { .. })
    }
}

/// Admission-owned state for one nonterminal demand/supplier match.
#[derive(Clone, Debug)]
struct H1Match {
    /// Cell whose connection is reserved by this match.
    supplier: PartitionId,
    /// Cell whose demand caused this match.
    requester: PartitionId,
    /// Exact requesting-cell demand generation that caused this match.
    demand: DemandId,
    /// Whether resolution borrows a sender or reclaims its capacity.
    kind: H1MatchKind,
    /// Admission-side progress of the match.
    state: H1MatchState,
    /// Whether requesting cell demand became stale before connection-cell
    /// resolution.
    cancelled: bool,
}

/// Origin-side progress of one retained H1 match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H1MatchState {
    /// Admission selected a connection, but the cell has not reserved it.
    Reserving,
    /// The cell will intercept the connection's next reusable return.
    WaitingForSender,
    /// A provisional sender is resolving outside admission.
    Resolving,
    /// Request cancellation must clear the installed reservation.
    Cancelling,
}

/// Work required to install a reuse reservation outside the admission lock.
#[derive(Debug)]
pub(in crate::client::pool) struct PreparedH1Reservation {
    /// Operation being installed.
    pub(in crate::client::pool) match_id: H1MatchId,
    /// Cell whose connection was selected while admission was locked.
    pub(in crate::client::pool) supplier: PartitionId,
}

/// Work required to cancel a reservation outside the admission lock.
pub(super) struct PreparedH1Cancellation {
    /// Operation whose reservation must be cleared.
    match_id: H1MatchId,
    /// Cell that owns the reservation.
    supplier: PartitionId,
}

impl H1SupplyIndex {
    fn apply_revision(
        &mut self,
        supplier: PartitionId,
        eligibility_group: EligibilityGroup,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        if self
            .records
            .get(&supplier)
            .is_some_and(|record| record.revision >= revision.revision)
        {
            return;
        }
        self.unlink_supplier(&supplier);
        let record = self
            .records
            .entry(supplier)
            .or_insert_with(|| H1SupplyRecord {
                eligibility_group: eligibility_group.clone(),
                revision: revision.revision,
                status: revision.status,
                reserved_by: None,
                index_state: H1SupplyIndexState::Unlinked,
            });
        record.eligibility_group = eligibility_group;
        record.revision = revision.revision;
        record.status = revision.status;
        self.link_supplier_if_selectable(&supplier);
    }

    fn select_borrow_supplier(
        &mut self,
        eligibility_group: &EligibilityGroup,
        requester: PartitionId,
    ) -> Option<PartitionId> {
        let supplier = {
            let order = self.suppliers_by_group.get(eligibility_group)?;
            self.first_peer_supplier(order, requester, false)?
        };
        self.unlink_supplier(&supplier);
        Some(supplier)
    }

    fn select_peer_reclaim_supplier(&mut self, requester: PartitionId) -> Option<PartitionId> {
        let supplier = self.first_peer_supplier(&self.reclaim_order, requester, true)?;
        self.unlink_supplier(&supplier);
        Some(supplier)
    }

    fn select_oldest_reclaim_supplier(&mut self) -> Option<PartitionId> {
        let supplier = self.reclaim_order.head()?;
        self.unlink_supplier(&supplier);
        Some(supplier)
    }

    fn first_peer_supplier(
        &self,
        order: &IntrusiveOrder<PartitionId>,
        requester: PartitionId,
        reclaim: bool,
    ) -> Option<PartitionId> {
        let head = order.head()?;
        if head != requester {
            return Some(head);
        }
        let record = self
            .records
            .get(&head)
            .expect("H1 supply order head disappeared");
        let H1SupplyIndexState::Linked {
            reclaim: reclaim_links,
            group,
        } = &record.index_state
        else {
            unreachable!("ordered H1 supplier was unlinked");
        };
        if reclaim {
            reclaim_links.next
        } else {
            group.next
        }
    }

    fn link_supplier_if_selectable(&mut self, supplier: &PartitionId) {
        let Some(record) = self.records.get(supplier) else {
            return;
        };
        if !record.is_selectable() || record.index_state.is_linked() {
            return;
        }
        let eligibility_group = record.eligibility_group.clone();
        let reclaim = self.reclaim_order.push_back(*supplier);
        let group = self
            .suppliers_by_group
            .entry(eligibility_group)
            .or_default()
            .push_back(*supplier);

        if let Some(previous) = reclaim.previous {
            let H1SupplyIndexState::Linked { reclaim, .. } = &mut self
                .records
                .get_mut(&previous)
                .expect("reclaim order tail disappeared")
                .index_state
            else {
                unreachable!("reclaim order tail was unlinked");
            };
            reclaim.next = Some(*supplier);
        }
        if let Some(previous) = group.previous {
            let H1SupplyIndexState::Linked { group, .. } = &mut self
                .records
                .get_mut(&previous)
                .expect("group supplier order tail disappeared")
                .index_state
            else {
                unreachable!("group supplier order tail was unlinked");
            };
            group.next = Some(*supplier);
        }
        self.records
            .get_mut(supplier)
            .expect("linked H1 supplier disappeared")
            .index_state = H1SupplyIndexState::Linked { reclaim, group };
    }

    fn unlink_supplier(&mut self, supplier: &PartitionId) {
        let Some(record) = self.records.get_mut(supplier) else {
            return;
        };
        let index_state = std::mem::take(&mut record.index_state);
        let H1SupplyIndexState::Linked { reclaim, group } = index_state else {
            return;
        };
        let eligibility_group = record.eligibility_group.clone();
        self.repair_reclaim_links(supplier, &reclaim);
        self.reclaim_order.remove(*supplier, reclaim);
        self.repair_group_links(supplier, &group);
        let order = self
            .suppliers_by_group
            .get_mut(&eligibility_group)
            .expect("linked H1 supplier lost its group order");
        order.remove(*supplier, group);
    }

    fn repair_reclaim_links(
        &mut self,
        supplier: &PartitionId,
        links: &IntrusiveLinks<PartitionId>,
    ) {
        if let Some(previous) = links.previous {
            let H1SupplyIndexState::Linked { reclaim, .. } = &mut self
                .records
                .get_mut(&previous)
                .expect("previous reclaim supplier disappeared")
                .index_state
            else {
                unreachable!("previous reclaim supplier was unlinked");
            };
            reclaim.next = links.next;
        }
        if let Some(next) = links.next {
            let H1SupplyIndexState::Linked { reclaim, .. } = &mut self
                .records
                .get_mut(&next)
                .expect("next reclaim supplier disappeared")
                .index_state
            else {
                unreachable!("next reclaim supplier was unlinked");
            };
            reclaim.previous = links.previous;
        }
        debug_assert_ne!(links.previous.as_ref(), Some(supplier));
        debug_assert_ne!(links.next.as_ref(), Some(supplier));
    }

    fn repair_group_links(&mut self, supplier: &PartitionId, links: &IntrusiveLinks<PartitionId>) {
        if let Some(previous) = links.previous {
            let H1SupplyIndexState::Linked { group, .. } = &mut self
                .records
                .get_mut(&previous)
                .expect("previous group supplier disappeared")
                .index_state
            else {
                unreachable!("previous group supplier was unlinked");
            };
            group.next = links.next;
        }
        if let Some(next) = links.next {
            let H1SupplyIndexState::Linked { group, .. } = &mut self
                .records
                .get_mut(&next)
                .expect("next group supplier disappeared")
                .index_state
            else {
                unreachable!("next group supplier was unlinked");
            };
            group.previous = links.previous;
        }
        debug_assert_ne!(links.previous.as_ref(), Some(supplier));
        debug_assert_ne!(links.next.as_ref(), Some(supplier));
    }

    #[cfg(any(debug_assertions, test))]
    fn assert_consistent(&self) {
        for record in self.records.values() {
            assert_eq!(
                record.index_state.is_linked(),
                record.is_selectable(),
                "H1 supplier index state did not match selectability"
            );
        }
        self.assert_order(&self.reclaim_order, None, true);
        for (group, order) in &self.suppliers_by_group {
            self.assert_order(order, Some(group), false);
        }
    }

    #[cfg(any(debug_assertions, test))]
    fn assert_order(
        &self,
        order: &IntrusiveOrder<PartitionId>,
        expected_group: Option<&EligibilityGroup>,
        reclaim_order: bool,
    ) {
        let expected = self
            .records
            .values()
            .filter(|record| {
                record.index_state.is_linked()
                    && expected_group.is_none_or(|group| &record.eligibility_group == group)
            })
            .count();
        order.assert_consistent(
            expected,
            self.records.len(),
            "HTTP/1 supplier order",
            |supplier| {
                let record = self
                    .records
                    .get(&supplier)
                    .expect("ordered H1 supplier disappeared");
                if let Some(group) = expected_group {
                    assert_eq!(&record.eligibility_group, group);
                }
                let H1SupplyIndexState::Linked { reclaim, group } = &record.index_state else {
                    unreachable!("ordered H1 supplier was unlinked");
                };
                if reclaim_order {
                    *reclaim
                } else {
                    *group
                }
            },
        );
    }
}

impl H1Supply {
    pub(super) fn apply_revision(
        &mut self,
        supplier: PartitionId,
        eligibility_group: EligibilityGroup,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        self.index
            .apply_revision(supplier, eligibility_group, revision);
        self.assert_consistent();
    }

    pub(super) fn reconcile_requester(&mut self, requester: &PartitionId, demand: &DemandSchedule) {
        let Some(match_id) = self.by_requester.get(requester).copied() else {
            return;
        };
        let Some(retained) = self.matches.get_mut(&match_id) else {
            self.by_requester.remove(requester);
            self.assert_consistent();
            return;
        };
        if demand.is_current_queued(&retained.requester, retained.demand) {
            return;
        }
        retained.cancelled = true;
        if retained.state == H1MatchState::WaitingForSender {
            retained.state = H1MatchState::Cancelling;
            self.cancellations.push_back(match_id);
        }
        self.assert_consistent();
    }

    pub(super) fn prepare_match(
        &mut self,
        demand: &DemandSchedule,
    ) -> Option<PreparedH1Reservation> {
        let queued = demand.queued_head()?;
        if self.by_requester.contains_key(&queued.requester) {
            return None;
        }
        let (supplier, kind) = if queued.requirement.accepts_h1() {
            match self
                .index
                .select_borrow_supplier(&queued.eligibility_group, queued.requester)
            {
                Some(supplier) => (supplier, H1MatchKind::BorrowSender),
                None => (
                    self.index.select_peer_reclaim_supplier(queued.requester)?,
                    H1MatchKind::ReclaimCapacity,
                ),
            }
        } else {
            (
                self.index.select_oldest_reclaim_supplier()?,
                H1MatchKind::ReclaimCapacity,
            )
        };
        let match_id = self.take_match_id();
        let record = self
            .index
            .records
            .get_mut(&supplier)
            .expect("selected H1 supplier disappeared");
        debug_assert!(record.reserved_by.is_none());
        record.reserved_by = Some(match_id);
        self.by_requester.insert(queued.requester, match_id);
        self.matches.insert(
            match_id,
            H1Match {
                supplier,
                requester: queued.requester,
                demand: queued.demand,
                kind,
                state: H1MatchState::Reserving,
                cancelled: false,
            },
        );
        self.assert_consistent();
        Some(PreparedH1Reservation { match_id, supplier })
    }

    pub(super) fn prepare_cancellation(&mut self) -> Option<PreparedH1Cancellation> {
        while let Some(match_id) = self.cancellations.pop_front() {
            let Some(retained) = self.matches.get(&match_id) else {
                continue;
            };
            if retained.state != H1MatchState::Cancelling {
                continue;
            }
            return Some(PreparedH1Cancellation {
                match_id,
                supplier: retained.supplier,
            });
        }
        None
    }

    fn settle_reservation(&mut self, match_id: H1MatchId, resolved: bool) -> Option<H1Match> {
        let retained = self.matches.get_mut(&match_id)?;
        if retained.state != H1MatchState::Reserving {
            return None;
        }
        retained.state = if resolved {
            H1MatchState::Resolving
        } else if retained.cancelled {
            H1MatchState::Cancelling
        } else {
            H1MatchState::WaitingForSender
        };
        if retained.state == H1MatchState::Cancelling {
            self.cancellations.push_back(match_id);
        }
        let retained = retained.clone();
        self.assert_consistent();
        Some(retained)
    }

    fn begin_resolution(&mut self, match_id: H1MatchId) -> Option<H1Match> {
        let retained = self.matches.get_mut(&match_id)?;
        if !matches!(
            retained.state,
            H1MatchState::WaitingForSender | H1MatchState::Resolving | H1MatchState::Cancelling
        ) {
            return None;
        }
        retained.state = H1MatchState::Resolving;
        let retained = retained.clone();
        self.assert_consistent();
        Some(retained)
    }

    fn settle_match(&mut self, match_id: H1MatchId, outcome: H1SupplyOutcome) -> Option<H1Match> {
        let outcome_supplier = *outcome.supplier();
        self.apply_h1_supply_outcome(outcome);
        let retained = self.matches.remove(&match_id);
        if let Some(retained) = retained.as_ref() {
            debug_assert_eq!(retained.supplier, outcome_supplier);
            if self.by_requester.get(&retained.requester) == Some(&match_id) {
                self.by_requester.remove(&retained.requester);
            }
            if let Some(record) = self.index.records.get_mut(&retained.supplier) {
                if record.reserved_by == Some(match_id) {
                    record.reserved_by = None;
                }
            }
        }
        self.index.link_supplier_if_selectable(&outcome_supplier);
        self.assert_consistent();
        retained
    }

    fn apply_h1_supply_outcome(&mut self, outcome: H1SupplyOutcome) {
        match outcome {
            H1SupplyOutcome::SupplierLive { supplier, revision } => {
                let Some(eligibility_group) = self
                    .index
                    .records
                    .get(&supplier)
                    .map(|record| record.eligibility_group.clone())
                else {
                    return;
                };
                self.index
                    .apply_revision(supplier, eligibility_group, revision);
            }
            H1SupplyOutcome::SupplierExpired { supplier } => {
                self.index.unlink_supplier(&supplier);
                self.index.records.remove(&supplier);
            }
        }
    }

    fn take_match_id(&mut self) -> H1MatchId {
        let value = self.next_match_id;
        self.next_match_id = value
            .checked_add(1)
            .expect("HTTP/1 match identity exhausted");
        H1MatchId(value)
    }

    fn assert_consistent(&self) {
        #[cfg(any(debug_assertions, test))]
        {
            if std::thread::panicking() {
                return;
            }
            self.index.assert_consistent();
            for (match_id, retained) in &self.matches {
                assert_eq!(self.by_requester.get(&retained.requester), Some(match_id));
                assert_eq!(
                    self.index
                        .records
                        .get(&retained.supplier)
                        .and_then(|record| record.reserved_by),
                    Some(*match_id),
                    "H1 supplier back-pointer did not name its retained match"
                );
            }
            for (requester, match_id) in &self.by_requester {
                assert_eq!(
                    self.matches
                        .get(match_id)
                        .map(|retained| &retained.requester),
                    Some(requester),
                    "H1 requester index named a missing match"
                );
            }
            for (supplier, record) in &self.index.records {
                if let Some(match_id) = record.reserved_by {
                    assert_eq!(
                        self.matches
                            .get(&match_id)
                            .map(|retained| &retained.supplier),
                        Some(supplier),
                        "H1 supplier record named a missing match"
                    );
                }
            }
            for (index, match_id) in self.cancellations.iter().enumerate() {
                if let Some(retained) = self.matches.get(match_id) {
                    assert!(
                        retained.cancelled
                            && matches!(
                                retained.state,
                                H1MatchState::Cancelling | H1MatchState::Resolving
                            ),
                        "live H1 cancellation named a match outside cancellation or resolution"
                    );
                }
                assert!(
                    !self
                        .cancellations
                        .iter()
                        .skip(index + 1)
                        .any(|id| id == match_id),
                    "H1 cancellation was queued more than once"
                );
            }
        }
    }
}

/// Reservation crossing from admission to one supplier cell.
pub(in crate::client::pool) struct H1ReservationAction {
    admission: Arc<OriginAdmission>,
    prepared: Option<PreparedH1Reservation>,
}

impl H1ReservationAction {
    pub(super) fn new(admission: Arc<OriginAdmission>, prepared: PreparedH1Reservation) -> Self {
        Self {
            admission,
            prepared: Some(prepared),
        }
    }

    pub(super) fn reserve_supplier(mut self) -> Option<AdmissionAction> {
        let prepared = self
            .prepared
            .take()
            .expect("HTTP/1 reservation consumed more than once");
        let Some(supplier) = self.admission.cell(&prepared.supplier) else {
            return OriginAdmission::settle_h1_match(
                &self.admission,
                prepared.match_id,
                H1SupplyOutcome::supplier_expired(prepared.supplier),
            );
        };
        OriginCell::commit_h1_reservation(&supplier, self.admission.clone(), prepared)
    }
}

impl Drop for H1ReservationAction {
    fn drop(&mut self) {
        if let Some(prepared) = self.prepared.take() {
            let outcome = h1_supply_outcome(&self.admission, &prepared.supplier, |supplier| {
                supplier.cancel_h1_reservation(prepared.match_id)
            });
            let next =
                OriginAdmission::settle_h1_match(&self.admission, prepared.match_id, outcome);
            OriginAdmission::run_action_chain(next);
        }
    }
}

/// Cancellation crossing for an installed supplier reservation.
pub(in crate::client::pool) struct H1CancellationAction {
    admission: Arc<OriginAdmission>,
    prepared: Option<PreparedH1Cancellation>,
}

impl H1CancellationAction {
    pub(super) fn new(admission: Arc<OriginAdmission>, prepared: PreparedH1Cancellation) -> Self {
        Self {
            admission,
            prepared: Some(prepared),
        }
    }

    pub(super) fn cancel_reservation(mut self) -> Option<AdmissionAction> {
        let prepared = self
            .prepared
            .take()
            .expect("HTTP/1 cancellation consumed more than once");
        let outcome = h1_supply_outcome(&self.admission, &prepared.supplier, |supplier| {
            supplier.cancel_h1_reservation(prepared.match_id)
        });
        OriginAdmission::settle_h1_match(&self.admission, prepared.match_id, outcome)
    }
}

impl Drop for H1CancellationAction {
    fn drop(&mut self) {
        if let Some(prepared) = self.prepared.take() {
            let outcome = h1_supply_outcome(&self.admission, &prepared.supplier, |supplier| {
                supplier.cancel_h1_reservation(prepared.match_id)
            });
            let next =
                OriginAdmission::settle_h1_match(&self.admission, prepared.match_id, outcome);
            OriginAdmission::run_action_chain(next);
        }
    }
}

/// Provisional sender owned by one resolving H1 match.
pub(in crate::client::pool) struct H1Candidate {
    admission: Arc<OriginAdmission>,
    match_id: H1MatchId,
    supplier: PartitionId,
    provisional: Option<ProvisionalH1>,
}

impl H1Candidate {
    pub(in crate::client::pool) fn new(
        admission: Arc<OriginAdmission>,
        match_id: H1MatchId,
        supplier: PartitionId,
        provisional: ProvisionalH1,
    ) -> Self {
        Self {
            admission,
            match_id,
            supplier,
            provisional: Some(provisional),
        }
    }

    pub(super) fn connection_id(&self) -> ConnectionId {
        self.provisional
            .as_ref()
            .expect("HTTP/1 candidate consumed more than once")
            .connection_id()
    }

    pub(in crate::client::pool) fn commit(mut self) -> Result<H1Selection, Self> {
        let Some(supplier) = self.admission.cell(&self.supplier) else {
            return Err(self);
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 candidate consumed more than once");
        match OriginCell::commit_h1_match(&supplier, self.match_id, provisional) {
            Ok(selection) => Ok(selection),
            Err(provisional) => {
                self.provisional = Some(provisional);
                Err(self)
            }
        }
    }

    /// Returns the provisional sender to its supplier without independently
    /// running the admission action chain.
    pub(super) fn reject(mut self) -> H1SupplyOutcome {
        let Some(provisional) = self.provisional.take() else {
            unreachable!("HTTP/1 candidate consumed more than once");
        };
        match self.admission.cell(&self.supplier) {
            Some(supplier) => H1SupplyOutcome::supplier_live(
                self.supplier,
                OriginCell::reject_h1_match(&supplier, self.match_id, provisional),
            ),
            None => {
                drop(provisional);
                H1SupplyOutcome::supplier_expired(self.supplier)
            }
        }
    }

    fn close_for_capacity(mut self) -> (H1SupplyOutcome, bool) {
        let Some(supplier) = self.admission.cell(&self.supplier) else {
            drop(self.provisional.take());
            return (H1SupplyOutcome::supplier_expired(self.supplier), false);
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 candidate consumed more than once");
        let (revision, reclaimed) =
            match OriginCell::reclaim_h1_candidate(&supplier, self.match_id, provisional) {
                Ok(result) => result,
                Err(provisional) => (
                    OriginCell::reject_h1_match(&supplier, self.match_id, provisional),
                    false,
                ),
            };
        (
            H1SupplyOutcome::supplier_live(self.supplier, revision),
            reclaimed,
        )
    }
}

impl fmt::Debug for H1Candidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("H1Candidate")
            .field("match_id", &self.match_id)
            .field("supplier", &self.supplier)
            .field("provisional", &self.provisional)
            .finish_non_exhaustive()
    }
}

impl Drop for H1Candidate {
    fn drop(&mut self) {
        let Some(provisional) = self.provisional.take() else {
            return;
        };
        let outcome = match self.admission.cell(&self.supplier) {
            Some(supplier) => H1SupplyOutcome::supplier_live(
                self.supplier,
                OriginCell::reject_h1_match(&supplier, self.match_id, provisional),
            ),
            None => {
                drop(provisional);
                H1SupplyOutcome::supplier_expired(self.supplier)
            }
        };
        let next = OriginAdmission::settle_h1_match(&self.admission, self.match_id, outcome);
        OriginAdmission::run_action_chain(next);
    }
}

/// Exact provisional H1 sender selected to close for bounded capacity.
pub(in crate::client::pool) struct H1CapacityReclaim {
    admission: Arc<OriginAdmission>,
    requester: PartitionId,
    match_id: H1MatchId,
    candidate: Option<H1Candidate>,
}

impl H1CapacityReclaim {
    pub(super) fn new(
        admission: Arc<OriginAdmission>,
        requester: PartitionId,
        match_id: H1MatchId,
        candidate: H1Candidate,
    ) -> Self {
        Self {
            admission,
            requester,
            match_id,
            candidate: Some(candidate),
        }
    }

    pub(super) fn reclaim_capacity(mut self) -> Option<AdmissionAction> {
        let candidate = self
            .candidate
            .take()
            .expect("HTTP/1 capacity reclaim consumed more than once");
        let connection_id = candidate.connection_id();
        let supplier = candidate.supplier;
        let (outcome, reclaimed) = candidate.close_for_capacity();
        if reclaimed {
            tracing::trace!(
                connection_id = %connection_id,
                request_partition = ?self.requester,
                connection_partition = ?supplier,
                origin_scheme = %self.admission.origin().scheme(),
                origin_host = self.admission.origin().host(),
                origin_port = ?self.admission.origin().port(),
                "HTTP/1 connection reclaimed for peer demand"
            );
        }
        OriginAdmission::settle_h1_match(&self.admission, self.match_id, outcome)
    }
}

/// Supplier-cell settlement after an irreversible sender transfer.
pub(in crate::client::pool) struct H1SupplierSettlement {
    admission: Arc<OriginAdmission>,
    match_id: H1MatchId,
    supplier: PartitionId,
    transferred: bool,
    active: bool,
}

impl H1SupplierSettlement {
    pub(super) fn new(
        admission: Arc<OriginAdmission>,
        match_id: H1MatchId,
        supplier: PartitionId,
        transferred: bool,
    ) -> Self {
        Self {
            admission,
            match_id,
            supplier,
            transferred,
            active: true,
        }
    }

    pub(super) fn settle_supplier(mut self) -> Option<AdmissionAction> {
        let outcome = h1_supply_outcome(&self.admission, &self.supplier, |supplier| {
            supplier.complete_h1_match(self.match_id, self.transferred)
        });
        let action = OriginAdmission::settle_h1_match(&self.admission, self.match_id, outcome);
        self.active = false;
        action
    }
}

impl Drop for H1SupplierSettlement {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let outcome = h1_supply_outcome(&self.admission, &self.supplier, |supplier| {
            supplier.complete_h1_match(self.match_id, self.transferred)
        });
        let next = OriginAdmission::settle_h1_match(&self.admission, self.match_id, outcome);
        OriginAdmission::run_action_chain(next);
    }
}

fn h1_supply_outcome(
    admission: &OriginAdmission,
    supplier: &PartitionId,
    current: impl FnOnce(&Arc<OriginCell>) -> SupplyRevision<H1SupplyStatus>,
) -> H1SupplyOutcome {
    match admission.cell(supplier) {
        Some(cell) => H1SupplyOutcome::supplier_live(*supplier, current(&cell)),
        None => H1SupplyOutcome::supplier_expired(*supplier),
    }
}

impl OriginAdmission {
    pub(in crate::client::pool) fn apply_h1_supply_revision(
        admission: &Arc<Self>,
        supplier: PartitionId,
        eligibility_group: EligibilityGroup,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        let action = {
            let mut state = admission.state.lock();
            state
                .h1_supply
                .apply_revision(supplier, eligibility_group, revision);
            Self::prepare_action(admission, &mut state)
        };
        Self::run_action_chain(action);
    }

    pub(in crate::client::pool) fn reject_returned_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        supplier: PartitionId,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        let action = Self::settle_h1_match(
            admission,
            match_id,
            H1SupplyOutcome::supplier_live(supplier, revision),
        );
        Self::run_action_chain(action);
    }

    pub(in crate::client::pool) fn settle_h1_reservation(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        supplier: PartitionId,
        decision: H1ReservationDecision<H1Candidate>,
    ) -> Option<AdmissionAction> {
        match decision {
            H1ReservationDecision::Rejected(revision) => Self::settle_h1_match(
                admission,
                match_id,
                H1SupplyOutcome::supplier_live(supplier, revision),
            ),
            H1ReservationDecision::Installed => {
                let mut state = admission.state.lock();
                state.h1_supply.settle_reservation(match_id, false);
                Self::prepare_action(admission, &mut state)
            }
            H1ReservationDecision::Candidate(candidate) => {
                {
                    let mut state = admission.state.lock();
                    if state.h1_supply.settle_reservation(match_id, true).is_none() {
                        drop(state);
                        return Self::reject_h1_candidate(admission, match_id, candidate);
                    }
                }
                Self::resolve_h1_match(admission, match_id, candidate)
            }
        }
    }

    pub(in crate::client::pool) fn resolve_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        candidate: H1Candidate,
    ) -> Option<AdmissionAction> {
        let mut state = admission.state.lock();
        let Some(retained) = state.h1_supply.begin_resolution(match_id) else {
            drop(state);
            return Self::reject_h1_candidate(admission, match_id, candidate);
        };
        if retained.cancelled
            || !state
                .demand
                .is_current_queued(&retained.requester, retained.demand)
        {
            drop(state);
            return Self::reject_h1_candidate(admission, match_id, candidate);
        }
        match retained.kind {
            H1MatchKind::BorrowSender => {
                let assignment_id = state.take_assignment_id();
                let old_group = state.demand.group_for(&retained.requester);
                let Some(assignment) = state.demand.prepare_h1_assignment(
                    &retained.requester,
                    retained.demand,
                    assignment_id,
                ) else {
                    drop(state);
                    return Self::reject_h1_candidate(admission, match_id, candidate);
                };
                state.reconcile_demand_indexes(&assignment.requester, old_group);
                Some(AdmissionAction::Deliver(DeliveryGuard::borrowed_h1(
                    admission.clone(),
                    assignment,
                    match_id,
                    retained.supplier,
                    candidate,
                )))
            }
            H1MatchKind::ReclaimCapacity => Some(AdmissionAction::ReclaimCapacity(
                super::CapacityReclaim::FromH1(H1CapacityReclaim::new(
                    admission.clone(),
                    retained.requester,
                    match_id,
                    candidate,
                )),
            )),
        }
    }

    fn settle_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        outcome: H1SupplyOutcome,
    ) -> Option<AdmissionAction> {
        let mut state = admission.state.lock();
        state.h1_supply.settle_match(match_id, outcome);
        Self::prepare_action(admission, &mut state)
    }

    /// Returns a provisional sender and keeps successor work in the caller's
    /// action chain instead of re-entering it through `H1Candidate::drop`.
    fn reject_h1_candidate(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        candidate: H1Candidate,
    ) -> Option<AdmissionAction> {
        let outcome = candidate.reject();
        Self::settle_h1_match(admission, match_id, outcome)
    }

    pub(super) fn settle_borrow_delivery(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        assignment: &super::DemandAssignment,
        outcome: DemandAssignmentOutcome,
        transferred_supplier: Option<PartitionId>,
        refused_outcome: Option<H1SupplyOutcome>,
    ) -> Option<AdmissionAction> {
        let mut state = admission.state.lock();
        state.settle_assignment(assignment, outcome);
        if let Some(supplier) = transferred_supplier {
            return Some(AdmissionAction::SettleH1Supplier(
                H1SupplierSettlement::new(admission.clone(), match_id, supplier, true),
            ));
        }
        state.h1_supply.settle_match(
            match_id,
            refused_outcome.expect("refused H1 borrow had no supplier outcome"),
        );
        Self::prepare_action(admission, &mut state)
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
        schedule.apply_snapshot(
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

    fn supply_revision(
        revision: u64,
        has_returnable_connection: bool,
        peer_use_blocked: bool,
    ) -> SupplyRevision<H1SupplyStatus> {
        SupplyRevision::new(
            revision,
            H1SupplyStatus {
                has_returnable_connection,
                peer_use_blocked,
            },
        )
    }

    #[test]
    fn supply_revision_keeps_one_bounded_residence() {
        let supplier = cell(1);
        let group = EligibilityGroup::Pool;
        let mut supply = H1Supply::default();

        for revision in 1..=500 {
            supply.apply_revision(
                supplier,
                group.clone(),
                supply_revision(revision, true, false),
            );
        }

        assert_eq!(1, supply.index.records.len());
        assert_eq!(1, supply.index.reclaim_order.len());
        assert_eq!(
            1,
            supply
                .index
                .suppliers_by_group
                .get(&group)
                .expect("supplier group was not indexed")
                .len()
        );

        supply.apply_revision(supplier, group.clone(), supply_revision(501, false, false));
        assert_eq!(0, supply.index.reclaim_order.len());
        assert_eq!(
            0,
            supply
                .index
                .suppliers_by_group
                .get(&group)
                .expect("supplier group disappeared")
                .len()
        );
    }

    #[test]
    fn h2_required_demand_reclaims_local_h1_capacity() {
        let requesting_partition = cell(1);
        let group = EligibilityGroup::Pool;
        let mut schedule = DemandSchedule::default();
        schedule.apply_snapshot(
            requesting_partition,
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H2Required,
                group.clone(),
            ),
        );
        let mut supply = H1Supply::default();
        supply.apply_revision(requesting_partition, group, supply_revision(1, true, false));

        let prepared = supply
            .prepare_match(&schedule)
            .expect("local HTTP/1 capacity was not selected for reclaim");

        assert_eq!(requesting_partition, prepared.supplier);
        assert_eq!(
            H1MatchKind::ReclaimCapacity,
            supply.matches[&prepared.match_id].kind
        );
    }

    #[test]
    fn borrow_supplier_selection_skips_the_requesting_cell() {
        let requesting_partition = cell(1);
        let peer = cell(2);
        let group = EligibilityGroup::Pool;
        let mut supply = H1Supply::default();
        supply.apply_revision(
            requesting_partition,
            group.clone(),
            supply_revision(1, true, false),
        );
        supply.apply_revision(peer, group.clone(), supply_revision(1, true, false));

        let prepared = supply
            .prepare_match(&schedule(requesting_partition, group))
            .expect("peer connection cell was not selected");
        assert_eq!(peer, prepared.supplier);
        assert_ne!(requesting_partition, prepared.supplier);
    }

    #[test]
    fn expired_supplier_cell_terminates_its_match_without_resubmission() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut supply = H1Supply::default();
        supply.apply_revision(connection_partition, group, supply_revision(1, true, false));
        let prepared = supply
            .prepare_match(&schedule)
            .expect("supplier cell did not produce a retained match");

        supply.settle_match(
            prepared.match_id,
            H1SupplyOutcome::supplier_expired(connection_partition),
        );

        assert!(!supply.index.records.contains_key(&connection_partition));
        assert!(supply.prepare_match(&schedule).is_none());
        assert!(supply.matches.is_empty());
    }

    #[test]
    fn stale_supply_outcome_cannot_hide_newer_status() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut supply = H1Supply::default();
        supply.apply_revision(connection_partition, group, supply_revision(1, true, false));
        let prepared = supply
            .prepare_match(&schedule)
            .expect("supplier cell did not produce a retained match");

        supply.settle_match(
            prepared.match_id,
            H1SupplyOutcome::supplier_live(connection_partition, supply_revision(3, true, false)),
        );
        supply.settle_match(
            prepared.match_id,
            H1SupplyOutcome::supplier_live(connection_partition, supply_revision(2, false, false)),
        );

        let supplier = supply
            .index
            .records
            .get(&connection_partition)
            .expect("stale revision removed the supplier cell");
        assert!(supplier.status.has_returnable_connection);
        assert!(supplier.index_state.is_linked());
        assert_eq!(3, supplier.revision);
    }

    #[test]
    fn duplicate_supply_outcome_still_refreshes_status() {
        let connection_partition = cell(1);
        let requesting_partition = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(requesting_partition, group.clone());
        let mut supply = H1Supply::default();
        supply.apply_revision(connection_partition, group, supply_revision(1, true, false));
        let prepared = supply
            .prepare_match(&schedule)
            .expect("supplier cell did not produce a retained match");

        supply.settle_match(
            prepared.match_id,
            H1SupplyOutcome::supplier_live(connection_partition, supply_revision(2, true, false)),
        );
        supply.settle_match(
            prepared.match_id,
            H1SupplyOutcome::supplier_live(connection_partition, supply_revision(3, false, false)),
        );

        let supplier = supply
            .index
            .records
            .get(&connection_partition)
            .expect("live supply revision removed the supplier cell");
        assert!(!supplier.status.has_returnable_connection);
        assert!(!supplier.index_state.is_linked());
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Demand, connection supply, and capacity shared by one bounded origin.
//!
//! [`OriginAdmission`] stores the origin once and keys its partition cells,
//! demand records, and supply indexes by [`PartitionId`]. Its lock is the sole
//! authority for admission-resident capacity, origin-wide demand order, and
//! retained HTTP/1 matching. Cell locks are never held with the admission lock.
//!
//! A bounded acquisition moves through the layer in this order:
//!
//! 1. A cell records complete [`DemandSnapshot`] updates in its cached
//!    [`DemandPublication`] after releasing its lock.
//! 2. One caller holds the [`baton::AdmissionBaton`] and applies pending publications
//!    under the admission lock. [`DemandSchedule`] replaces each partition's
//!    snapshot and links active demand in the origin and eligibility-group orders.
//! 3. Capacity delivery and HTTP/1 matching select from the origin head.
//!    HTTP/2 route matching pairs a group head with peer supply.
//! 4. The selected demand becomes a [`DemandAssignment`] while a resource
//!    crosses lock domains. Both scheduling positions remain attached, so
//!    refusal can restore the demand without changing its order.
//! 5. A [`DeliveryGuard`] resolves its capacity or HTTP/1 payload before
//!    committing the exact assigned waiter.
//! 6. An [`H2RouteGuard`] carries identities through the same unlocked
//!    handoff. It revalidates the connection-owning generation, then installs
//!    a route and activation opportunity in the requesting cell.
//! 7. The requesting cell becomes authoritative before either guard settles the
//!    assignment. Refusal and drop execute the same fallback paths.
//!
//! Capacity and HTTP/1 handoffs own the payload they must return on failure.
//! HTTP/2 route installation moves no payload: the connection-owning cell
//! retains its request handle, driver, socket, and capacity. No cell lock is
//! held with the admission lock, and connection-owning and requesting cell
//! locks are never held together.

use super::cell::{H1IdleProbeDecision, H1ReservationDecision, OriginCell};
use super::origin::OriginKey;
use super::partition::{EligibilityGroup, PartitionId};
use super::registry::AdmissionPolicy;
use super::stats::ConnectionCapacityStats;
use crate::sync::{Arc, Mutex, Weak};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;

mod baton;
mod delivery;
mod demand;
mod h1;
mod h2;
mod order;

use self::demand::{
    DemandAssignment, DemandAssignmentId, DemandAssignmentOutcome, DemandSchedule,
    PreparedCapacityDelivery,
};
pub(in crate::client::pool) use self::demand::{
    DemandId, DemandSnapshot, ProtocolRequirement, SnapshotVersion,
};
use self::h1::{
    H1CancellationAction, H1CapacityReclaim, H1IdleProbeAction, H1ReservationAction,
    H1SupplierSettlement, H1Supply, H1SupplyOutcome, PreparedH1Match,
};
use self::h2::{H2CapacityReclaim, H2RouteGuard, H2Supply, PreparedH2Reclaim, PreparedH2Route};
use self::order::{IntrusiveLinks, IntrusiveOrder};
use baton::AdmissionProcessing;
pub(in crate::client::pool) use baton::{BatonScope, DemandPublication};
pub(in crate::client::pool) use delivery::DeliveryGuard;
pub(in crate::client::pool) use h1::{
    H1Candidate, H1MatchId, H1SupplyStatus, PreparedH1IdleProbe, PreparedH1Reservation,
};
pub(in crate::client::pool) use h2::H2SupplyStatus;

/// Complete admission-facing protocol status at one cell-owned revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct SupplyRevision<T> {
    /// Monotonic sequence number allocated under the cell lock.
    pub(in crate::client::pool) revision: u64,
    /// Complete status represented by this revision.
    pub(in crate::client::pool) status: T,
}

impl<T> SupplyRevision<T> {
    pub(in crate::client::pool) fn new(revision: u64, status: T) -> Self {
        Self { revision, status }
    }
}

/// One conserved unit of bounded-origin connection capacity.
///
/// This non-`Copy` value proves that admission removed one unit from its
/// available count. Identities are never reused so diagnostics remain
/// unambiguous.
pub(super) struct CapacityPermit(u64);

impl fmt::Debug for CapacityPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CapacityPermit").field(&self.0).finish()
    }
}

/// Shared admission authority for one bounded origin.
///
/// The state lock is the only authority for available capacity while it
/// resides in admission, and for demand order while no delivery is crossing
/// to a cell. Delivery work is detached before this lock is released.
pub(in crate::client::pool) struct OriginAdmission {
    /// Canonical origin shared by every partition represented in `state`.
    origin: OriginKey,
    /// Whether admission may close idle H2 capacity for H1-required demand.
    can_reclaim_h2_for_h1: bool,
    /// Capacity, demand, and supply state for this origin.
    state: Mutex<AdmissionState>,
    /// Pending demand and detached actions processed by one caller-held baton.
    processing: AdmissionProcessing,
}

impl OriginAdmission {
    /// Creates admission for one bounded origin policy.
    pub(in crate::client::pool) fn new(origin: OriginKey, policy: AdmissionPolicy) -> Arc<Self> {
        Arc::new(Self {
            origin,
            can_reclaim_h2_for_h1: policy.can_reclaim_h2_for_h1(),
            state: Mutex::new(AdmissionState::new(policy.connection_limit())),
            processing: AdmissionProcessing::default(),
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(limit: NonZeroUsize) -> Arc<Self> {
        Self::for_test_with_h2_reclaim(limit, true)
    }

    #[cfg(test)]
    pub(super) fn for_test_with_h2_reclaim(
        limit: NonZeroUsize,
        can_reclaim_h2_for_h1: bool,
    ) -> Arc<Self> {
        Self::new(
            OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "example.com", None)
                .expect("test origin is valid"),
            AdmissionPolicy::new(limit, can_reclaim_h2_for_h1),
        )
    }

    /// Returns authoritative bounded capacity for this origin.
    pub(in crate::client::pool) fn connection_capacity_stats(&self) -> ConnectionCapacityStats {
        let state = self.state.lock();
        ConnectionCapacityStats::new(
            state.capacity.limit,
            state.capacity.limit - state.capacity.available,
        )
    }

    /// Returns the canonical origin coordinated by this admission authority.
    fn origin(&self) -> &OriginKey {
        &self.origin
    }

    /// Registers or returns the unique retained cell for an identity.
    pub(in crate::client::pool) fn register_cell(
        origin: &Arc<Self>,
        candidate: Arc<OriginCell>,
    ) -> Arc<OriginCell> {
        let partition = candidate.id().partition();
        assert_eq!(
            origin.origin(),
            candidate.id().origin(),
            "cell origin did not match its admission authority"
        );
        let existing = {
            let mut state = origin.state.lock();
            match state.cells.get(&partition).and_then(Weak::upgrade) {
                Some(existing) => Some(existing),
                None => {
                    state.cells.insert(partition, Weak::from_arc(&candidate));
                    None
                }
            }
        };
        if let Some(existing) = existing {
            // Dropping the losing cell may return capacity to this admission
            // authority, so release the state lock first.
            drop(candidate);
            existing
        } else {
            candidate
        }
    }

    /// Returns the shared pending-demand slot for this partition.
    ///
    /// Concurrent construction of the same cell uses the same slot. The cell
    /// caches it so each publication avoids the slot directory lock.
    pub(in crate::client::pool) fn demand_publication(
        &self,
        requester: PartitionId,
    ) -> Arc<DemandPublication> {
        self.processing.slot(requester)
    }

    /// Records demand and opens a local-completion scope for the admission baton.
    ///
    /// The caller releases its cell lock first. Recording uses the publication
    /// slot's mutex, without taking the origin admission lock. A new holder
    /// finishes its local transition before processing; an existing holder can
    /// observe these updates concurrently. After cleanup and wakes, `process`
    /// may nominate this ready caller against an eligible holder.
    pub(in crate::client::pool) fn record_demand_updates<'a>(
        admission: &'a Arc<Self>,
        publication: &DemandPublication,
        snapshots: impl IntoIterator<Item = DemandSnapshot>,
    ) -> BatonScope<'a> {
        BatonScope::record(admission, publication, snapshots)
    }

    /// Exercises production publication and processing without a cell transition.
    ///
    /// This has no local cleanup. It follows the production scope, including
    /// contender nomination when an eligible holder exists.
    #[cfg(test)]
    pub(super) fn submit_demand_for_test(
        admission: &Arc<Self>,
        requester: PartitionId,
        snapshots: impl IntoIterator<Item = DemandSnapshot>,
    ) {
        let publication = admission.processing.slot(requester);
        Self::record_demand_updates(admission, &publication, snapshots).process();
    }

    /// Selects at most one action while admission is locked.
    ///
    /// Cancellation cleanup runs first, followed by available capacity,
    /// a compatible peer HTTP/2 route, HTTP/1 reuse, and idle-H2 reclaim. The
    /// returned action owns everything needed to run after releasing the
    /// admission lock. The baton holder runs one action at a time, queuing any
    /// successor. Guard cleanup queues work through the same ingress. Admission
    /// and cell locks are never nested.
    fn prepare_action(origin: &Arc<Self>, state: &mut AdmissionState) -> Option<AdmissionAction> {
        if let Some(cancellation) = state.h1_supply.prepare_cancellation() {
            return Some(AdmissionAction::CancelH1Reservation(
                H1CancellationAction::new(origin.clone(), cancellation),
            ));
        }
        if let Some(prepared) = state.prepare_capacity_delivery() {
            return Some(AdmissionAction::Deliver(DeliveryGuard::capacity(
                origin.clone(),
                prepared.assignment,
                prepared.permit,
            )));
        }
        if let Some(route) = state.prepare_h2_route() {
            return Some(AdmissionAction::AttachH2Route(H2RouteGuard::new(
                origin.clone(),
                route,
            )));
        }
        if let Some(prepared) = state.h1_supply.prepare_match(&state.demand) {
            return Some(match prepared {
                PreparedH1Match::ProbeIdle(probe) => {
                    AdmissionAction::ProbeH1Supplier(H1IdleProbeAction::new(origin.clone(), probe))
                }
                PreparedH1Match::Reserve(reservation) => AdmissionAction::ReserveH1Supplier(
                    H1ReservationAction::new(origin.clone(), reservation),
                ),
            });
        }
        if !origin.can_reclaim_h2_for_h1 {
            return None;
        }
        state
            .h2_supply
            .prepare_reclaim(&state.demand)
            .map(|reclaim| {
                AdmissionAction::ReclaimCapacity(CapacityReclaim::FromH2(H2CapacityReclaim::new(
                    origin.clone(),
                    reclaim,
                )))
            })
    }

    /// Upgrades a registered requesting cell without holding the admission lock.
    fn cell(&self, id: &PartitionId) -> Option<Arc<OriginCell>> {
        let cell = {
            let state = self.state.lock();
            state.cells.get(id).cloned()
        };
        cell.and_then(|cell| cell.upgrade())
    }

    /// Enqueues detached work for the caller-held baton.
    ///
    /// A new holder drains until quiescent. Under an existing holder this
    /// returns after enqueueing; guards retain exact ownership until execution.
    pub(in crate::client::pool) fn enqueue_action(action: Option<AdmissionAction>) {
        if let Some(action) = action {
            let admission = action.admission().clone();
            AdmissionProcessing::enqueue_action(&admission, action);
        }
    }

    /// Returns whether admission still recognizes this exact assignment.
    #[cfg(test)]
    fn assignment_is_current(&self, assignment: &DemandAssignment) -> bool {
        self.state.lock().demand.assignment_is_current(assignment)
    }

    /// Returns `permit` to admission and serves ordered demand when possible.
    fn return_permit(origin: &Arc<Self>, permit: CapacityPermit) {
        let action = {
            let mut state = origin.state.lock();
            state.return_permit(permit);
            Self::prepare_action(origin, &mut state)
        };
        Self::enqueue_action(action);
    }

    /// Settles one demand assignment and returns an unused capacity permit.
    fn settle_delivery(
        admission: &Arc<Self>,
        assignment: &DemandAssignment,
        permit: Option<CapacityPermit>,
        outcome: DemandAssignmentOutcome,
    ) -> Option<AdmissionAction> {
        let mut state = admission.state.lock();
        if let Some(permit) = permit {
            state.return_permit(permit);
        }
        state.settle_assignment(assignment, outcome);
        Self::prepare_action(admission, &mut state)
    }

    /// Applies one cell's complete HTTP/1 supply revision.
    pub(in crate::client::pool) fn apply_h1_supply_revision(
        admission: &Arc<Self>,
        supplier: PartitionId,
        eligibility_group: EligibilityGroup,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        h1::apply_supply_revision(admission, supplier, eligibility_group, revision);
    }

    /// Rejects a returned sender whose retained match is no longer current.
    pub(in crate::client::pool) fn reject_returned_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        supplier: PartitionId,
        revision: SupplyRevision<H1SupplyStatus>,
    ) {
        h1::reject_returned_match(admission, match_id, supplier, revision);
    }

    /// Settles one supplier-cell idle probe against its retained H1 match.
    pub(in crate::client::pool) fn settle_h1_idle_probe(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        supplier: PartitionId,
        decision: H1IdleProbeDecision<H1Candidate>,
    ) -> Option<AdmissionAction> {
        h1::settle_idle_probe(admission, match_id, supplier, decision)
    }

    /// Settles installation of one supplier-cell reservation.
    pub(in crate::client::pool) fn settle_h1_reservation(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        supplier: PartitionId,
        decision: H1ReservationDecision<H1Candidate>,
    ) -> Option<AdmissionAction> {
        h1::settle_reservation(admission, match_id, supplier, decision)
    }

    /// Resolves a provisional sender against its retained match and demand.
    pub(in crate::client::pool) fn resolve_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        candidate: H1Candidate,
    ) -> Option<AdmissionAction> {
        h1::resolve_match(admission, match_id, candidate)
    }

    /// Removes one retained H1 match and applies its supplier outcome.
    fn settle_h1_match(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        outcome: H1SupplyOutcome,
    ) -> Option<AdmissionAction> {
        h1::settle_match(admission, match_id, outcome)
    }

    /// Settles a borrowed-sender assignment and its retained H1 match.
    fn settle_borrow_delivery(
        admission: &Arc<Self>,
        match_id: H1MatchId,
        assignment: &DemandAssignment,
        outcome: DemandAssignmentOutcome,
        transferred_supplier: Option<PartitionId>,
        refused_outcome: Option<H1SupplyOutcome>,
    ) -> Option<AdmissionAction> {
        h1::settle_borrow_delivery(
            admission,
            match_id,
            assignment,
            outcome,
            transferred_supplier,
            refused_outcome,
        )
    }

    /// Applies one cell's complete HTTP/2 supply revision.
    pub(in crate::client::pool) fn apply_h2_supply_revision(
        admission: &Arc<Self>,
        supplier: PartitionId,
        eligibility_group: EligibilityGroup,
        revision: SupplyRevision<H2SupplyStatus>,
    ) {
        h2::apply_supply_revision(admission, supplier, eligibility_group, revision);
    }

    /// Settles one H2 route assignment and repairs stale supply if needed.
    fn settle_h2_route(
        admission: &Arc<Self>,
        prepared: &PreparedH2Route,
        stale_generation: Option<super::cell::h2::H2GenerationId>,
        outcome: DemandAssignmentOutcome,
    ) -> Option<AdmissionAction> {
        h2::settle_route(admission, prepared, stale_generation, outcome)
    }

    /// Completes one exact idle-H2 reclaim crossing.
    fn settle_h2_reclaim(
        admission: &Arc<Self>,
        prepared: &PreparedH2Reclaim,
        revision: Option<SupplyRevision<H2SupplyStatus>>,
    ) -> Option<AdmissionAction> {
        h2::settle_reclaim(admission, prepared, revision)
    }

    #[cfg(test)]
    pub(super) fn submit_action_without_running(
        admission: &Arc<Self>,
        requester: PartitionId,
        snapshot: DemandSnapshot,
    ) -> Option<AdmissionAction> {
        let mut state = admission.state.lock();
        state.apply_demand_snapshot(requester, snapshot);
        Self::prepare_action(admission, &mut state)
    }

    #[cfg(test)]
    pub(super) fn submit_without_running(
        admission: &Arc<Self>,
        requester: PartitionId,
        snapshot: DemandSnapshot,
    ) -> Option<DeliveryGuard> {
        match Self::submit_action_without_running(admission, requester, snapshot) {
            Some(AdmissionAction::Deliver(delivery)) => Some(delivery),
            Some(_) => panic!("capacity-only test unexpectedly prepared another action"),
            None => None,
        }
    }

    #[cfg(test)]
    fn probe(&self) -> AdmissionProbe {
        let state = self.state.lock();
        AdmissionProbe {
            limit: state.capacity.limit,
            available: state.available_capacity(),
            ordered: state.demand.len(),
            queued: state.demand.queued_len(),
            assigned: state.demand.pending_assignment_count(),
        }
    }

    #[cfg(test)]
    pub(super) fn lease_for_test(origin: &Arc<Self>) -> CapacityLease {
        let permit = origin
            .state
            .lock()
            .take_permit()
            .expect("test origin had no available capacity");
        CapacityLease::new(origin.clone(), permit)
    }

    #[cfg(test)]
    pub(super) fn available_capacity_for_test(&self) -> usize {
        self.state.lock().available_capacity()
    }

    #[cfg(test)]
    pub(super) fn ordered_demand_count_for_test(&self) -> usize {
        self.state.lock().demand.len()
    }

    #[cfg(test)]
    pub(super) fn latest_demand_for_test(&self, requester: PartitionId) -> Option<DemandSnapshot> {
        self.state
            .lock()
            .demand
            .latest_for_test(&requester)
            .cloned()
    }

    #[cfg(all(test, smithy_http_client_loom))]
    pub(super) fn clear_modeled_cells_for_test(&self) {
        // Loom has no modeled Weak, so its synchronization facade retains
        // cells strongly. Explicit teardown prevents that model-only
        // substitution from appearing as an Arc leak.
        self.state.lock().cells.clear();
    }
}

/// One detached step prepared while holding the bounded-origin lock.
pub(super) enum AdmissionAction {
    /// One capacity or borrowed-H1 payload handed to a requesting cell.
    Deliver(DeliveryGuard),
    /// Probe one selected HTTP/1 supplier for an immediately idle sender.
    ProbeH1Supplier(H1IdleProbeAction),
    /// Reserve the selected supplier at its owning HTTP/1 cell.
    ReserveH1Supplier(H1ReservationAction),
    /// Cancel a retained HTTP/1 supplier reservation.
    CancelH1Reservation(H1CancellationAction),
    /// Settle sender ownership at the original HTTP/1 supplier cell.
    SettleH1Supplier(H1SupplierSettlement),
    /// HTTP/2 generation route handed to one requesting cell.
    AttachH2Route(H2RouteGuard),
    /// Close selected connection supply outside admission to recover capacity.
    ReclaimCapacity(CapacityReclaim),
}

impl AdmissionAction {
    fn admission(&self) -> &Arc<OriginAdmission> {
        match self {
            Self::Deliver(action) => &action.admission,
            Self::ProbeH1Supplier(action) => &action.admission,
            Self::ReserveH1Supplier(action) => &action.admission,
            Self::CancelH1Reservation(action) => &action.admission,
            Self::SettleH1Supplier(action) => &action.admission,
            Self::AttachH2Route(action) => &action.admission,
            Self::ReclaimCapacity(CapacityReclaim::FromH1(action)) => &action.admission,
            Self::ReclaimCapacity(CapacityReclaim::FromH2(action)) => &action.admission,
        }
    }

    /// Advances one crossing without recursively driving its successor.
    fn run_once(self) -> Option<Self> {
        match self {
            Self::Deliver(delivery) => delivery.deliver(),
            Self::ProbeH1Supplier(probe) => probe.probe_supplier(),
            Self::ReserveH1Supplier(reservation) => reservation.reserve_supplier(),
            Self::CancelH1Reservation(cancellation) => cancellation.cancel_reservation(),
            Self::SettleH1Supplier(settlement) => settlement.settle_supplier(),
            Self::AttachH2Route(route) => route.attach_route(),
            Self::ReclaimCapacity(reclaim) => reclaim.reclaim_capacity(),
        }
    }

    #[cfg(all(test, smithy_http_client_loom))]
    pub(super) fn run_once_for_test(self) -> Option<Self> {
        self.run_once()
    }
}

/// Protocol-specific detached reclaim with one shared admission action.
pub(super) enum CapacityReclaim {
    FromH1(H1CapacityReclaim),
    FromH2(H2CapacityReclaim),
}

impl CapacityReclaim {
    fn reclaim_capacity(self) -> Option<AdmissionAction> {
        match self {
            Self::FromH1(reclaim) => reclaim.reclaim_capacity(),
            Self::FromH2(reclaim) => reclaim.reclaim_capacity(),
        }
    }
}

impl fmt::Debug for OriginAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OriginAdmission")
            .field("state", &self.state)
            .finish()
    }
}

/// One admitted connection slot for a bounded origin.
///
/// While this lease exists, one place in the origin's configured connection
/// limit is occupied. The lease moves through capacity delivery, waiter
/// resolution, and establishment into the installed
/// [`super::connection::ConnectionState`]. Logical close drops the installed
/// lease and makes that place available again.
///
/// Dropping a lease may synchronously deliver capacity to another waiter, so
/// callers must move it out of protected state before drop.
pub(in crate::client::pool) struct CapacityLease {
    /// Admission state to which this slot returns when the lease ends.
    admission: Arc<OriginAdmission>,
    /// Permit returned to admission when this lease ends.
    permit: Option<CapacityPermit>,
}

impl CapacityLease {
    /// Takes ownership of a permit removed from admission's available set.
    fn new(admission: Arc<OriginAdmission>, permit: CapacityPermit) -> Self {
        Self {
            admission,
            permit: Some(permit),
        }
    }
}

impl fmt::Debug for CapacityLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityLease")
            .field("permit", &self.permit)
            .finish_non_exhaustive()
    }
}

impl Drop for CapacityLease {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            OriginAdmission::return_permit(&self.admission, permit);
        }
    }
}

/// Capacity not currently owned by a delivery, attempt, or connection.
#[derive(Debug)]
struct CapacityBudget {
    /// Configured connection bound used to check capacity conservation.
    limit: usize,
    /// Permits available for a new establishment attempt.
    available: usize,
    /// Next never-reused permit identity.
    next_permit_id: u64,
}

impl CapacityBudget {
    fn new(limit: NonZeroUsize) -> Self {
        let limit = limit.get();
        Self {
            limit,
            available: limit,
            next_permit_id: 0,
        }
    }

    fn take_permit(&mut self) -> Option<CapacityPermit> {
        if self.available == 0 {
            return None;
        }
        let id = self.next_permit_id;
        self.next_permit_id = id.checked_add(1).expect("permit identity exhausted");
        self.available -= 1;
        Some(CapacityPermit(id))
    }

    fn return_permit(&mut self, _permit: CapacityPermit) {
        let available = self
            .available
            .checked_add(1)
            .expect("available capacity count overflowed");
        assert!(
            available <= self.limit,
            "available capacity exceeded the configured limit"
        );
        self.available = available;
    }
}

/// Mutable bounded-origin state protected by one admission lock.
#[derive(Debug)]
struct AdmissionState {
    /// Requesting cells, held weakly to avoid an ownership cycle.
    cells: HashMap<PartitionId, Weak<OriginCell>>,
    /// Conserved bounded-origin connection capacity.
    capacity: CapacityBudget,
    /// Canonical cross-cell demand and scheduling orders.
    demand: DemandSchedule,
    /// Admission's indexed view of HTTP/1 connection supply.
    h1_supply: H1Supply,
    /// Admission's indexed view of HTTP/2 connection supply.
    h2_supply: H2Supply,
    /// Next never-reused resource-to-demand assignment identity.
    next_assignment_id: u64,
}

impl AdmissionState {
    /// Creates admission with all configured capacity available.
    fn new(limit: NonZeroUsize) -> Self {
        Self {
            cells: HashMap::new(),
            capacity: CapacityBudget::new(limit),
            demand: DemandSchedule::default(),
            h1_supply: H1Supply::default(),
            h2_supply: H2Supply::default(),
            next_assignment_id: 0,
        }
    }

    /// Removes one available permit from admission.
    fn take_permit(&mut self) -> Option<CapacityPermit> {
        self.capacity.take_permit()
    }

    /// Consumes a returned permit and restores one available slot.
    fn return_permit(&mut self, permit: CapacityPermit) {
        self.capacity.return_permit(permit);
    }

    #[cfg(test)]
    fn available_capacity(&self) -> usize {
        self.capacity.available
    }

    /// Applies one complete cell snapshot to cross-cell demand scheduling.
    fn apply_demand_snapshot(&mut self, requester: PartitionId, snapshot: DemandSnapshot) {
        let old_group = self.demand.group_for(&requester);
        self.demand.apply_snapshot(requester, snapshot);
        self.reconcile_demand_indexes(&requester, old_group);
    }

    /// Refreshes protocol indexes derived from the canonical demand schedule.
    ///
    /// HTTP/1 indexes depend on the requesting partition's origin position.
    /// HTTP/2 route scheduling depends on both previous and current eligibility
    /// groups, so a group change must repair each view. The derived indexes do
    /// not own demand or ordering.
    fn reconcile_demand_indexes(
        &mut self,
        requesting_partition: &PartitionId,
        old_group: Option<EligibilityGroup>,
    ) {
        self.h1_supply
            .reconcile_requester(requesting_partition, &self.demand);
        if let Some(old_group) = old_group {
            self.h2_supply.reconcile_group(&old_group, &self.demand);
        }
        if let Some(group) = self.demand.group_for(requesting_partition) {
            self.h2_supply.reconcile_group(&group, &self.demand);
        }
    }

    /// Pairs the oldest deliverable demand with one available permit.
    fn prepare_capacity_delivery(&mut self) -> Option<PreparedCapacityDelivery> {
        if !self.demand.head_is_queued() {
            return None;
        }

        let permit = self.take_permit()?;
        let assignment_id = self.take_assignment_id();
        let old_group = self
            .demand
            .queued_head()
            .and_then(|head| self.demand.group_for(&head.requester));
        let assignment = self
            .demand
            .prepare_origin_assignment(assignment_id)
            .expect("queued demand head disappeared");
        self.reconcile_demand_indexes(&assignment.requester, old_group);
        Some(PreparedCapacityDelivery { permit, assignment })
    }

    /// Applies one detached assignment outcome to canonical demand.
    fn settle_assignment(
        &mut self,
        assignment: &DemandAssignment,
        outcome: DemandAssignmentOutcome,
    ) {
        let old_group = self.demand.group_for(&assignment.requester);
        self.demand.settle_assignment(assignment, outcome);
        self.reconcile_demand_indexes(&assignment.requester, old_group);
    }

    /// Reserves one eligibility-group head for an identity-only H2 route.
    fn prepare_h2_route(&mut self) -> Option<PreparedH2Route> {
        if !self.h2_supply.has_route_ready_group() {
            return None;
        }
        let assignment_id = self.take_assignment_id();
        self.h2_supply
            .prepare_route(&mut self.demand, assignment_id)
    }

    /// Allocates an assignment identity that is never reused by this origin.
    fn take_assignment_id(&mut self) -> DemandAssignmentId {
        let value = self.next_assignment_id;
        self.next_assignment_id = value
            .checked_add(1)
            .expect("demand assignment identity exhausted");
        DemandAssignmentId(value)
    }
}

/// Test snapshot of the bounded-origin capacity and demand ledger.
#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
struct AdmissionProbe {
    /// Configured connection capacity.
    limit: usize,
    /// Permits not currently owned by a connection or delivery.
    available: usize,
    /// Demands linked in origin order, including a pending assignment.
    ordered: usize,
    /// Demands eligible to start a delivery.
    queued: usize,
    /// Demands currently owned by a detached assignment.
    assigned: usize,
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::partition::PartitionId;

    fn cell(origin: &Arc<OriginAdmission>, partition: usize) -> Arc<OriginCell> {
        let cell = Arc::new(OriginCell::new(
            PartitionId::from_index(partition),
            OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(origin.clone()),
            None,
        ));
        OriginAdmission::register_cell(origin, cell)
    }

    fn demand(id: u64) -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(id),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Compatible,
            EligibilityGroup::Pool,
        )
    }

    #[test]
    fn batched_local_successor_moves_behind_other_queued_demand() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let held = origin.state.lock().take_permit().unwrap();
        let first = PartitionId::from_index(1);
        let second = PartitionId::from_index(2);
        OriginAdmission::submit_demand_for_test(&origin, first, [demand(1)]);
        OriginAdmission::submit_demand_for_test(&origin, second, [demand(1)]);
        OriginAdmission::submit_demand_for_test(
            &origin,
            first,
            [
                DemandSnapshot::inactive(DemandId::from_u64(1), SnapshotVersion::INITIAL.next()),
                demand(2),
            ],
        );
        let mut state = origin.state.lock();
        assert_eq!(Some(&demand(2)), state.demand.latest_for_test(&first));
        assert_eq!(2, state.demand.len());
        state.return_permit(held);
        let other = state.prepare_capacity_delivery().unwrap();
        assert_eq!(second, other.assignment.requester);
        state.settle_assignment(
            &other.assignment,
            DemandAssignmentOutcome::Accepted { successor: None },
        );
        state.return_permit(other.permit);
        let successor = state.prepare_capacity_delivery().unwrap();
        assert_eq!(first, successor.assignment.requester);
        state.settle_assignment(
            &successor.assignment,
            DemandAssignmentOutcome::Accepted { successor: None },
        );
        state.return_permit(successor.permit);
        assert_eq!(1, state.available_capacity());
        assert_eq!(0, state.demand.len());
    }

    #[test]
    fn batched_local_successor_survives_old_assignment_settlement() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let first = PartitionId::from_index(1);
        let pending = {
            let mut state = origin.state.lock();
            state.apply_demand_snapshot(first, demand(1));
            state.prepare_capacity_delivery().unwrap()
        };
        OriginAdmission::submit_demand_for_test(
            &origin,
            first,
            [
                DemandSnapshot::inactive(DemandId::from_u64(1), SnapshotVersion::INITIAL.next()),
                demand(2),
            ],
        );
        let mut state = origin.state.lock();
        assert_eq!(Some(&demand(2)), state.demand.latest_for_test(&first));
        assert!(
            !state.demand.assignment_is_current(&pending.assignment),
            "retired demand must no longer authorize the old assignment"
        );
        assert!(
            state.prepare_capacity_delivery().is_none(),
            "old detached assignment must prevent a second assignment"
        );
        state.settle_assignment(
            &pending.assignment,
            DemandAssignmentOutcome::Refused { successor: None },
        );
        state.return_permit(pending.permit);
        let successor = state.prepare_capacity_delivery().unwrap();
        assert_eq!(DemandId::from_u64(2), successor.assignment.demand);
        state.settle_assignment(
            &successor.assignment,
            DemandAssignmentOutcome::Accepted { successor: None },
        );
        state.return_permit(successor.permit);
        assert_eq!(1, state.available_capacity());
        assert_eq!(0, state.demand.len());
    }

    #[test]
    fn stale_batch_cannot_resurrect_cancelled_successor() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let held = origin.state.lock().take_permit().unwrap();
        let first = PartitionId::from_index(1);
        let cancelled =
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL.next());
        OriginAdmission::submit_demand_for_test(&origin, first, [cancelled.clone()]);
        OriginAdmission::submit_demand_for_test(
            &origin,
            first,
            [
                DemandSnapshot::inactive(DemandId::from_u64(1), SnapshotVersion::INITIAL.next()),
                demand(2),
            ],
        );
        let mut state = origin.state.lock();
        assert_eq!(Some(&cancelled), state.demand.latest_for_test(&first));
        assert_eq!(0, state.demand.len());
        state.return_permit(held);
        assert_eq!(1, state.available_capacity());
    }

    #[test]
    #[should_panic(expected = "cell origin did not match its admission authority")]
    fn registration_rejects_a_cell_from_another_origin() {
        let admission = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let candidate = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "other.example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(admission.clone()),
            None,
        ));

        OriginAdmission::register_cell(&admission, candidate);
    }

    #[test]
    fn permits_are_linear_and_never_reused() {
        let mut state = AdmissionState::new(NonZeroUsize::new(2).unwrap());

        assert_eq!(2, state.available_capacity());
        let first = state.take_permit().unwrap();
        let first_id = first.0;
        let second = state.take_permit().unwrap();
        assert_eq!(0, state.available_capacity());
        assert!(state.take_permit().is_none());

        state.return_permit(first);
        let third = state.take_permit().unwrap();
        assert_ne!(first_id, third.0);
        state.return_permit(second);
        state.return_permit(third);
        assert_eq!(2, state.available_capacity());
    }

    #[test]
    fn equal_or_older_snapshots_do_not_replace_current_demand() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let requesting_partition = PartitionId::from_index(1);
        let current = demand(2);
        state.apply_demand_snapshot(requesting_partition, current.clone());
        state.apply_demand_snapshot(
            requesting_partition,
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL),
        );
        state.apply_demand_snapshot(requesting_partition, demand(1));

        assert_eq!(1, state.demand.len());
        assert_eq!(
            Some(&current),
            state.demand.latest_for_test(&requesting_partition)
        );
    }

    #[test]
    fn cancellation_churn_does_not_retain_order_entries() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_permit().unwrap();
        let requesting_partition = PartitionId::from_index(1);

        for id in 0..2_000 {
            let id = DemandId::from_u64(id);
            state.apply_demand_snapshot(
                requesting_partition,
                DemandSnapshot::active(
                    id,
                    SnapshotVersion::INITIAL,
                    ProtocolRequirement::H1Compatible,
                    EligibilityGroup::Pool,
                ),
            );
            state.apply_demand_snapshot(
                requesting_partition,
                DemandSnapshot::inactive(id, SnapshotVersion::INITIAL.next()),
            );
        }

        assert_eq!(0, state.demand.len());
        state.return_permit(held);
    }

    #[test]
    fn removing_middle_and_tail_demands_repairs_order() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_permit().unwrap();
        let targets: Vec<_> = (1..=5).map(PartitionId::from_index).collect();

        for (index, requesting_partition) in targets[..4].iter().enumerate() {
            state.apply_demand_snapshot(*requesting_partition, demand(index as u64 + 1));
        }
        state.apply_demand_snapshot(
            targets[1],
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL.next()),
        );
        state.apply_demand_snapshot(
            targets[3],
            DemandSnapshot::inactive(DemandId::from_u64(4), SnapshotVersion::INITIAL.next()),
        );
        state.apply_demand_snapshot(targets[4], demand(5));
        assert_eq!(3, state.demand.len());

        state.return_permit(held);
        for expected in [&targets[0], &targets[2], &targets[4]] {
            let pending = state.prepare_capacity_delivery().unwrap();
            assert_eq!(expected, &pending.assignment.requester);
            state.settle_assignment(
                &pending.assignment,
                DemandAssignmentOutcome::Accepted { successor: None },
            );
            state.return_permit(pending.permit);
        }
        assert_eq!(0, state.demand.len());
    }

    #[test]
    fn new_demand_moves_queued_cell_to_the_tail() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_permit().unwrap();
        let first = PartitionId::from_index(1);
        let second = PartitionId::from_index(2);
        state.apply_demand_snapshot(first, demand(1));
        state.apply_demand_snapshot(second, demand(2));
        state.apply_demand_snapshot(first, demand(3));
        state.return_permit(held);

        assert_eq!(
            second,
            state
                .prepare_capacity_delivery()
                .unwrap()
                .assignment
                .requester
        );
    }

    #[test]
    fn losing_registered_cell_returns_capacity_after_admission_unlocks() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let retained = cell(&origin, 1);
        let candidate = Arc::new(OriginCell::new(
            retained.id().partition(),
            retained.id().origin().clone(),
            EligibilityGroup::Pool,
            Some(origin.clone()),
            None,
        ));
        assert_eq!(retained.id(), candidate.id());

        let (_waiter, snapshot) =
            candidate.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let mut delivery =
            OriginAdmission::submit_without_running(&origin, candidate.id().partition(), snapshot)
                .expect("candidate demand did not reserve capacity");
        assert!(delivery.resolve_payload_for_test());
        assert!(OriginCell::receive_delivery(&candidate, delivery).is_none());
        assert_eq!(0, origin.available_capacity_for_test());

        let winner = OriginAdmission::register_cell(&origin, candidate);
        assert!(Arc::ptr_eq(&retained, &winner));
        assert_eq!(
            1,
            origin.available_capacity_for_test(),
            "losing candidate did not return capacity after registration"
        );
    }

    #[test]
    fn separate_origins_conserve_capacity_independently() {
        let first = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let second = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let first_cell = cell(&first, 1);
        let second_cell = cell(&second, 1);

        let first_delivery =
            OriginAdmission::submit_without_running(&first, first_cell.id().partition(), demand(1))
                .unwrap();
        let second_delivery = OriginAdmission::submit_without_running(
            &second,
            second_cell.id().partition(),
            demand(1),
        )
        .unwrap();
        assert_eq!(0, first.available_capacity_for_test());
        assert_eq!(0, second.available_capacity_for_test());

        drop(first_delivery);
        assert_eq!(1, first.available_capacity_for_test());
        assert_eq!(0, second.available_capacity_for_test());

        drop(second_delivery);
        assert_eq!(1, second.available_capacity_for_test());
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use crate::client::pool::partition::PartitionId;

    fn id() -> PartitionId {
        PartitionId::from_index(1)
    }

    fn demand() -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(1),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Compatible,
            EligibilityGroup::Pool,
        )
    }

    /// A completed old assignment must not erase a concurrently batched successor.
    #[test]
    fn batched_successor_races_old_assignment_settlement() {
        loom::model(|| {
            let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
            let pending = {
                let mut state = origin.state.lock();
                state.apply_demand_snapshot(id(), demand());
                state.prepare_capacity_delivery().unwrap()
            };
            // Retain the only permit until both transitions finish, isolating
            // publication and settlement rather than running a new delivery.
            let settling_origin = origin.clone();
            let settling = loom::thread::spawn(move || {
                settling_origin.state.lock().settle_assignment(
                    &pending.assignment,
                    DemandAssignmentOutcome::Refused { successor: None },
                );
                pending.permit
            });
            let publishing_origin = origin.clone();
            let publishing = loom::thread::spawn(move || {
                OriginAdmission::submit_demand_for_test(
                    &publishing_origin,
                    id(),
                    [
                        DemandSnapshot::inactive(
                            DemandId::from_u64(1),
                            SnapshotVersion::INITIAL.next(),
                        ),
                        DemandSnapshot::active(
                            DemandId::from_u64(2),
                            SnapshotVersion::INITIAL,
                            ProtocolRequirement::H1Compatible,
                            EligibilityGroup::Pool,
                        ),
                    ],
                );
            });
            let permit = settling.join().unwrap();
            publishing.join().unwrap();
            let mut state = origin.state.lock();
            state.return_permit(permit);
            let successor = state.prepare_capacity_delivery().unwrap();
            assert_eq!(DemandId::from_u64(2), successor.assignment.demand);
            state.settle_assignment(
                &successor.assignment,
                DemandAssignmentOutcome::Accepted { successor: None },
            );
            state.return_permit(successor.permit);
            assert_eq!(1, state.available_capacity());
            assert_eq!(0, state.demand.len());
        });
    }

    /// Races return of the only capacity permit with publication of one demand.
    ///
    /// The permit remains conserved and demand is either absent or represented
    /// by one schedulable record, never by an outstanding assignment.
    #[test]
    fn release_and_demand_submission_conserve_one_permit() {
        loom::model(|| {
            let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
            let permit = origin.state.lock().take_permit().unwrap();
            let lease = CapacityLease::new(origin.clone(), permit);

            let release = loom::thread::spawn(move || drop(lease));
            let publish_origin = origin.clone();
            let publish = loom::thread::spawn(move || {
                let delivery =
                    OriginAdmission::submit_without_running(&publish_origin, id(), demand());
                drop(delivery);
            });
            release.join().unwrap();
            publish.join().unwrap();

            let probe = origin.probe();
            assert_eq!(1, probe.limit);
            assert_eq!(1, probe.available);
            assert_eq!(0, probe.assigned);
            assert!(
                matches!((probe.ordered, probe.queued), (0, 0) | (1, 1)),
                "demand publication left duplicate or unschedulable demand: {probe:?}"
            );
        });
    }

    /// Replaces cancelled demand while its previous assignment is still detached.
    ///
    /// Assignment settlement and replacement publication may enter in either
    /// order, but capacity and the requester's active assignment remain singular.
    #[test]
    fn cancellation_preserves_an_outstanding_demand_assignment() {
        loom::model(|| {
            let origin = OriginAdmission::for_test(NonZeroUsize::new(2).unwrap());
            let requesting_partition = id();
            let delivery =
                OriginAdmission::submit_without_running(&origin, requesting_partition, demand())
                    .unwrap();
            let dropping = loom::thread::spawn(move || drop(delivery));
            let replacement_origin = origin.clone();
            let replacing = loom::thread::spawn(move || {
                replacement_origin.state.lock().apply_demand_snapshot(
                    requesting_partition,
                    DemandSnapshot::inactive(
                        DemandId::from_u64(1),
                        SnapshotVersion::INITIAL.next(),
                    ),
                );
                OriginAdmission::submit_without_running(
                    &replacement_origin,
                    requesting_partition,
                    DemandSnapshot::active(
                        DemandId::from_u64(2),
                        SnapshotVersion::INITIAL,
                        ProtocolRequirement::H1Compatible,
                        EligibilityGroup::Pool,
                    ),
                )
            });

            dropping.join().unwrap();
            if let Some(replacement) = replacing.join().unwrap() {
                assert!(
                    replacement.is_current(),
                    "replacement delivery did not own the current demand assignment"
                );
                drop(replacement);
            }

            let probe = origin.probe();
            assert_eq!(2, probe.available);
            assert_eq!(0, probe.assigned);
            assert_eq!(0, probe.ordered);
        });
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Capacity and demand shared by the cells of one bounded origin.
//!
//! [`OriginAdmission`] is the authority for connection permits and cross-cell
//! demand order. Its state lives behind one lock. Values that cross to a cell
//! carry their own fallback so delivery and cancellation complete without
//! nesting admission and cell locks.

use super::cell::{CellId, OriginCell};
use super::partition::EligibilityGroup;
use crate::sync::{Arc, Mutex, Weak};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;

mod delivery;
mod demand;
pub(in crate::client::pool) mod reuse;

use self::demand::{DemandSchedule, PreparedCapacityDelivery};
pub(in crate::client::pool) use delivery::DeliveryGuard;

/// Protocol capability required by the head waiter in a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtocolRequirement {
    /// The waiter may dispatch over HTTP/1 or HTTP/2.
    H1Compatible,
    /// The waiter requires HTTP/2.
    #[allow(dead_code, reason = "used when HTTP/2-only acquisition is implemented")]
    H2Required,
}

/// Identity of one cell-local demand generation.
///
/// A generation begins when a waiter becomes the cell's bounded-demand head.
/// The cell identity and this value together identify its publications.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct DemandId(u64);

impl DemandId {
    /// Constructs an identity allocated by one cell's monotonic demand counter.
    pub(crate) const fn from_u64(value: u64) -> Self {
        Self(value)
    }
}

/// Strict ordering of complete publications within one [`DemandId`].
///
/// Versions never wrap. Preserving strict order prevents a delayed publication
/// from becoming current again after counter reuse (an ABA). A new FIFO head
/// receives a new [`DemandId`] and starts again at [`SnapshotVersion::INITIAL`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SnapshotVersion(u64);

impl SnapshotVersion {
    /// Version assigned to the first snapshot for a demand generation.
    pub(crate) const INITIAL: Self = Self(0);

    /// Advances the publication version for the same demand generation.
    ///
    /// # Panics
    ///
    /// Panics after `u64::MAX` replacements of one generation. Wrapping would
    /// break stale-publication rejection; a new head waiter normally starts a
    /// new generation and resets this counter.
    pub(crate) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("demand snapshot version exhausted"),
        )
    }
}

/// Complete state published for one demand identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DemandState {
    /// The cell has a waiter that still needs a connection.
    Active {
        /// Protocol capability required by the cell's oldest waiter.
        head: ProtocolRequirement,
        /// Partitions whose connections may satisfy this demand.
        eligibility_group: EligibilityGroup,
    },
    /// The demand has ended without a successor in this snapshot.
    Inactive,
}

/// Versioned replacement state for one cell's current demand generation.
///
/// A publication replaces the complete previous snapshot. This prevents a
/// delayed active publication from reviving demand retired by a newer version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DemandSnapshot {
    /// Cell-local identity of the head-waiter generation.
    id: DemandId,
    /// Ordering among publications for the same demand identity.
    version: SnapshotVersion,
    /// Complete current state of the demand generation.
    state: DemandState,
}

impl DemandSnapshot {
    /// Describes the current head waiter for an active demand generation.
    pub(crate) fn active(
        id: DemandId,
        version: SnapshotVersion,
        head: ProtocolRequirement,
        eligibility_group: EligibilityGroup,
    ) -> Self {
        Self {
            id,
            version,
            state: DemandState::Active {
                head,
                eligibility_group,
            },
        }
    }

    /// Retires a demand generation at the supplied publication version.
    pub(crate) fn inactive(id: DemandId, version: SnapshotVersion) -> Self {
        Self {
            id,
            version,
            state: DemandState::Inactive,
        }
    }

    /// Returns whether this snapshot still requests capacity.
    fn is_active(&self) -> bool {
        matches!(self.state, DemandState::Active { .. })
    }

    /// Returns whether this snapshot may replace `current`.
    ///
    /// A newer generation supersedes every snapshot of an older generation;
    /// publications within one generation are ordered by snapshot version.
    fn is_newer_than(&self, current: &Self) -> bool {
        self.id > current.id || (self.id == current.id && self.version > current.version)
    }
}

/// Stable identity of one admitted connection slot within an origin.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct PermitId(u64);

/// Stable identity of one admission-to-cell delivery.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct DeliveryId(u64);

/// Shared admission authority for one bounded origin.
///
/// The state lock is the only authority for permit ownership while capacity
/// resides in admission, and for demand order while no delivery is crossing
/// to a cell. Delivery work is detached before this lock is released.
pub(crate) struct OriginAdmission {
    /// Permit, demand, and delivery-fence state for this origin.
    state: Mutex<AdmissionState>,
}

impl OriginAdmission {
    /// Creates admission for at most `limit` logically open connections.
    pub(crate) fn new(limit: NonZeroUsize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(AdmissionState::new(limit)),
        })
    }

    /// Registers or returns the unique retained cell for an identity.
    pub(crate) fn register_cell(origin: &Arc<Self>, candidate: Arc<OriginCell>) -> Arc<OriginCell> {
        let id = candidate.id().clone();
        let mut state = origin.state.lock();
        if let Some(existing) = state.cells.get(&id).and_then(Weak::upgrade) {
            return existing;
        }
        state.cells.insert(id, Weak::from_arc(&candidate));
        candidate
    }

    /// Publishes a complete demand snapshot and drives any resulting delivery.
    pub(crate) fn publish_demand(
        origin: &Arc<Self>,
        requesting_cell: CellId,
        snapshot: DemandSnapshot,
    ) {
        let action = {
            let mut state = origin.state.lock();
            state.publish_demand(requesting_cell, snapshot);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Extracts the next bounded-origin action while admission is locked.
    fn prepare_action(origin: &Arc<Self>, state: &mut AdmissionState) -> Option<AdmissionAction> {
        if let Some(cancellation) = state.h1.prepare_cancellation() {
            return Some(AdmissionAction::H1(reuse::H1ReuseAction::cancel(
                origin.clone(),
                cancellation,
            )));
        }
        if let Some(pending) = state.schedule_one() {
            return Some(AdmissionAction::Delivery(DeliveryGuard::capacity(
                origin.clone(),
                pending.delivery,
                pending.requesting_cell,
                pending.demand,
                pending.permit,
            )));
        }
        state
            .h1
            .prepare_reuse(&state.demand_schedule)
            .map(|reuse| AdmissionAction::H1(reuse::H1ReuseAction::install(origin.clone(), reuse)))
    }

    /// Upgrades a registered requesting cell without holding the admission lock.
    fn cell(&self, id: &CellId) -> Option<Arc<OriginCell>> {
        let requesting_cell = {
            let state = self.state.lock();
            state.cells.get(id).cloned()
        };
        requesting_cell.and_then(|requesting_cell| requesting_cell.upgrade())
    }

    /// Drives sequential unlocked actions until no completion schedules another.
    pub(in crate::client::pool) fn drive(mut action: Option<AdmissionAction>) {
        while let Some(current) = action {
            action = current.drive_once();
        }
    }

    /// Returns whether the named delivery still owns the requesting cell's demand fence.
    #[cfg(test)]
    fn delivery_is_current(
        &self,
        delivery: DeliveryId,
        requesting_cell: &CellId,
        demand: DemandId,
    ) -> bool {
        self.state
            .lock()
            .delivery_is_current(delivery, requesting_cell, demand)
    }

    /// Returns `permit` to admission and serves ordered demand when possible.
    fn return_permit(origin: &Arc<Self>, permit: PermitId) {
        let action = {
            let mut state = origin.state.lock();
            state.available.push(permit);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Closes a delivery fence and prepares at most one successor.
    ///
    /// `permit` is present when an undelivered payload is refunnelled. A
    /// committed delivery leaves the permit in its installed
    /// [`CapacityLease`] and acknowledges with `None`.
    fn finish_delivery(
        origin: &Arc<Self>,
        delivery: DeliveryId,
        requesting_cell: &CellId,
        permit: Option<PermitId>,
        result: DeliveryAckResult,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        if let Some(permit) = permit {
            state.available.push(permit);
        }
        state.finish_delivery(delivery, requesting_cell, result);
        Self::prepare_action(origin, &mut state)
    }

    #[cfg(test)]
    pub(super) fn publish_action_without_driving(
        origin: &Arc<Self>,
        requesting_cell: CellId,
        snapshot: DemandSnapshot,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.publish_demand(requesting_cell, snapshot);
        Self::prepare_action(origin, &mut state)
    }

    #[cfg(test)]
    pub(super) fn publish_without_driving(
        origin: &Arc<Self>,
        requesting_cell: CellId,
        snapshot: DemandSnapshot,
    ) -> Option<DeliveryGuard> {
        match Self::publish_action_without_driving(origin, requesting_cell, snapshot) {
            Some(AdmissionAction::Delivery(delivery)) => Some(delivery),
            Some(AdmissionAction::H1(_)) => {
                panic!("capacity-only test unexpectedly prepared an HTTP/1 action")
            }
            None => None,
        }
    }

    #[cfg(test)]
    fn counts(&self) -> AdmissionCounts {
        let state = self.state.lock();
        AdmissionCounts {
            limit: state.limit,
            available: state.available_capacity(),
            ordered: state.demand_schedule.len(),
            queued: state.demand_schedule.queued_len(),
            delivering: state.demand_schedule.delivering_len(),
        }
    }

    #[cfg(test)]
    pub(super) fn lease_for_test(origin: &Arc<Self>) -> CapacityLease {
        let permit = origin
            .state
            .lock()
            .take_available()
            .expect("test origin had no available capacity");
        CapacityLease::new(origin.clone(), permit)
    }

    #[cfg(test)]
    pub(super) fn available_capacity_for_test(&self) -> usize {
        self.state.lock().available_capacity()
    }

    #[cfg(test)]
    pub(super) fn ordered_demand_count_for_test(&self) -> usize {
        self.state.lock().demand_schedule.len()
    }

    #[cfg(all(test, smithy_http_client_loom))]
    pub(super) fn clear_modeled_cells_for_test(&self) {
        // Loom has no modeled Weak, so its synchronization facade retains
        // cells strongly. Explicit teardown prevents that model-only
        // substitution from appearing as an Arc leak.
        self.state.lock().cells.clear();
    }
}

/// One unlocked step prepared while holding the bounded-origin lock.
pub(super) enum AdmissionAction {
    /// One capacity or borrowed-H1 payload crossing to a requesting cell.
    Delivery(DeliveryGuard),
    /// HTTP/1 availability, reservation, or borrowed-sender work.
    H1(reuse::H1ReuseAction),
}

impl AdmissionAction {
    /// Executes one lock-domain crossing and returns the next prepared step.
    fn drive_once(self) -> Option<Self> {
        match self {
            Self::Delivery(delivery) => delivery.deliver_once(),
            Self::H1(action) => action.drive_once(),
        }
    }

    /// Advances one crossing without recursively driving its successor.
    #[cfg(all(test, smithy_http_client_loom))]
    pub(super) fn drive_once_for_test(self) -> Option<Self> {
        self.drive_once()
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
pub(crate) struct CapacityLease {
    /// Admission state to which this slot returns when the lease ends.
    origin: Arc<OriginAdmission>,
    /// Internal identity of the occupied slot.
    permit: Option<PermitId>,
}

impl CapacityLease {
    /// Takes ownership of a permit removed from admission's available set.
    fn new(origin: Arc<OriginAdmission>, permit: PermitId) -> Self {
        Self {
            origin,
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
            OriginAdmission::return_permit(&self.origin, permit);
        }
    }
}

/// Admission's acknowledgement of a requesting cell-side delivery attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
enum DeliveryAckResult {
    /// The requesting cell accepted the payload and optionally published its successor demand.
    Accepted { successor: Option<DemandSnapshot> },
    /// The same demand remains live at its existing order position.
    RetrySameResidence,
    /// The old demand ended and may have a successor.
    Rejected { successor: Option<DemandSnapshot> },
}

/// Mutable bounded-origin state protected by one admission lock.
#[derive(Debug)]
struct AdmissionState {
    /// Requesting cells, held weakly to avoid an ownership cycle.
    cells: HashMap<CellId, Weak<OriginCell>>,
    /// Issued permits returned by failed attempts or closed connections.
    available: Vec<PermitId>,
    /// Permits not yet materialized from the configured limit.
    unissued: usize,
    /// Next never-reused permit identity.
    next_permit: u64,
    /// Cross-cell demand records and their origin-wide scheduling order.
    demand_schedule: DemandSchedule,
    /// HTTP/1 availability reports and cross-cell reuse operations.
    h1: reuse::H1Reuse,
    /// Next never-reused delivery identity.
    next_delivery: u64,
    /// Configured number of permits, retained only for test snapshots.
    #[cfg(test)]
    limit: usize,
}

impl AdmissionState {
    /// Creates a lazy permit ledger with no materialized permit identities.
    fn new(limit: NonZeroUsize) -> Self {
        let limit = limit.get();
        Self {
            cells: HashMap::new(),
            available: Vec::new(),
            unissued: limit,
            next_permit: 0,
            demand_schedule: DemandSchedule::default(),
            h1: reuse::H1Reuse::default(),
            next_delivery: 0,
            #[cfg(test)]
            limit,
        }
    }

    /// Removes an available permit or materializes the next configured slot.
    fn take_available(&mut self) -> Option<PermitId> {
        if let Some(permit) = self.available.pop() {
            return Some(permit);
        }
        if self.unissued == 0 {
            return None;
        }

        let permit = PermitId(self.next_permit);
        self.next_permit = self
            .next_permit
            .checked_add(1)
            .expect("capacity identity exhausted");
        self.unissued -= 1;
        Some(permit)
    }

    #[cfg(test)]
    fn available_capacity(&self) -> usize {
        self.unissued
            .checked_add(self.available.len())
            .expect("available capacity count overflowed")
    }

    /// Applies one complete cell publication to cross-cell scheduling.
    fn publish_demand(&mut self, requesting_cell: CellId, snapshot: DemandSnapshot) {
        self.demand_schedule
            .publish(requesting_cell.clone(), snapshot);
        self.h1
            .reconcile_requesting_cell(&requesting_cell, &self.demand_schedule);
    }

    /// Pairs the oldest deliverable demand with one available permit.
    fn schedule_one(&mut self) -> Option<PreparedCapacityDelivery> {
        if !self.demand_schedule.head_is_queued() {
            return None;
        }

        let permit = self.take_available()?;
        let delivery = self.take_delivery_id();
        let scheduled = self
            .demand_schedule
            .reserve_head(delivery)
            .expect("queued demand head disappeared");
        self.h1
            .reconcile_requesting_cell(&scheduled.requesting_cell, &self.demand_schedule);
        Some(PreparedCapacityDelivery {
            permit,
            delivery,
            requesting_cell: scheduled.requesting_cell,
            demand: scheduled.demand,
        })
    }

    /// Delegates delivery-fence revalidation to the demand schedule.
    #[cfg(test)]
    fn delivery_is_current(
        &self,
        delivery: DeliveryId,
        requesting_cell: &CellId,
        demand: DemandId,
    ) -> bool {
        self.demand_schedule
            .delivery_is_current(delivery, requesting_cell, demand)
    }

    /// Applies a requesting cell acknowledgement to the demand schedule.
    fn finish_delivery(
        &mut self,
        delivery: DeliveryId,
        requesting_cell: &CellId,
        result: DeliveryAckResult,
    ) {
        self.demand_schedule
            .finish_delivery(delivery, requesting_cell, result);
    }

    /// Allocates a delivery-fence identity that is never reused by this origin.
    fn take_delivery_id(&mut self) -> DeliveryId {
        let value = self.next_delivery;
        self.next_delivery = value.checked_add(1).expect("delivery identity exhausted");
        DeliveryId(value)
    }
}

/// Test snapshot of the bounded-origin permit and demand ledger.
#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
struct AdmissionCounts {
    /// Configured permit count.
    limit: usize,
    /// Permits not currently owned by a connection or delivery.
    available: usize,
    /// Demands linked in origin order, including a delivery fence.
    ordered: usize,
    /// Demands eligible to start a delivery.
    queued: usize,
    /// Demands currently fenced by a crossing delivery.
    delivering: usize,
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::PartitionId;
    use http_1x::uri::Scheme;

    fn cell(origin: &Arc<OriginAdmission>, partition: usize) -> Arc<OriginCell> {
        let cell = Arc::new(OriginCell::new(
            PartitionId::from_index(partition),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
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
    fn configured_limit_materializes_permits_only_when_used() {
        let limit = NonZeroUsize::new(4096).unwrap();
        let mut state = AdmissionState::new(limit);

        assert!(state.available.is_empty());
        assert_eq!(limit.get(), state.unissued);
        assert_eq!(PermitId(0), state.take_available().unwrap());
        assert!(state.available.is_empty());
        assert_eq!(limit.get() - 1, state.unissued);
    }

    #[test]
    fn equal_or_older_snapshots_do_not_replace_current_demand() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let requesting_cell = CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );
        let current = demand(2);
        state.publish_demand(requesting_cell.clone(), current.clone());
        state.publish_demand(
            requesting_cell.clone(),
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL),
        );
        state.publish_demand(requesting_cell.clone(), demand(1));

        assert_eq!(1, state.demand_schedule.len());
        assert_eq!(
            Some(&current),
            state.demand_schedule.latest_for_test(&requesting_cell)
        );
    }

    #[test]
    fn cancellation_churn_does_not_retain_order_entries() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_available().unwrap();
        let requesting_cell = CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );

        for id in 0..2_000 {
            let id = DemandId::from_u64(id);
            state.publish_demand(
                requesting_cell.clone(),
                DemandSnapshot::active(
                    id,
                    SnapshotVersion::INITIAL,
                    ProtocolRequirement::H1Compatible,
                    EligibilityGroup::Pool,
                ),
            );
            state.publish_demand(
                requesting_cell.clone(),
                DemandSnapshot::inactive(id, SnapshotVersion::INITIAL.next()),
            );
        }

        assert_eq!(0, state.demand_schedule.len());
        state.available.push(held);
    }

    #[test]
    fn removing_middle_and_tail_demands_repairs_order() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_available().unwrap();
        let targets: Vec<_> = (1..=5)
            .map(|partition| {
                CellId::new(
                    PartitionId::from_index(partition),
                    OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
                )
            })
            .collect();

        for (index, requesting_cell) in targets[..4].iter().enumerate() {
            state.publish_demand(requesting_cell.clone(), demand(index as u64 + 1));
        }
        state.publish_demand(
            targets[1].clone(),
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL.next()),
        );
        state.publish_demand(
            targets[3].clone(),
            DemandSnapshot::inactive(DemandId::from_u64(4), SnapshotVersion::INITIAL.next()),
        );
        state.publish_demand(targets[4].clone(), demand(5));
        assert_eq!(3, state.demand_schedule.len());

        state.available.push(held);
        for expected in [&targets[0], &targets[2], &targets[4]] {
            let pending = state.schedule_one().unwrap();
            assert_eq!(expected, &pending.requesting_cell);
            state.finish_delivery(
                pending.delivery,
                &pending.requesting_cell,
                DeliveryAckResult::Accepted { successor: None },
            );
            state.available.push(pending.permit);
        }
        assert_eq!(0, state.demand_schedule.len());
    }

    #[test]
    fn new_demand_moves_queued_cell_to_the_tail() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_available().unwrap();
        let first = CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );
        let second = CellId::new(
            PartitionId::from_index(2),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );
        state.publish_demand(first.clone(), demand(1));
        state.publish_demand(second.clone(), demand(2));
        state.publish_demand(first, demand(3));
        state.available.push(held);

        assert_eq!(second, state.schedule_one().unwrap().requesting_cell);
    }

    #[test]
    fn delivery_currency_includes_the_demand_id() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let requesting_cell = cell(&origin, 1);
        let delivery = OriginAdmission::publish_without_driving(
            &origin,
            requesting_cell.id().clone(),
            demand(1),
        )
        .unwrap();
        assert!(delivery.is_current());

        origin
            .state
            .lock()
            .publish_demand(requesting_cell.id().clone(), demand(2));
        assert!(!delivery.is_current());
        delivery.reject(None);
    }

    #[test]
    fn stale_successor_cannot_leave_active_demand_idle() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let requesting_cell = cell(&origin, 1);
        let delivery = OriginAdmission::publish_without_driving(
            &origin,
            requesting_cell.id().clone(),
            demand(1),
        )
        .unwrap();
        origin.state.lock().publish_demand(
            requesting_cell.id().clone(),
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL.next(),
                ProtocolRequirement::H1Compatible,
                EligibilityGroup::Pool,
            ),
        );
        delivery.reject(Some(demand(1)));

        assert_eq!(1, origin.counts().available);
        assert_eq!(0, origin.counts().ordered);
    }

    #[test]
    fn dropped_delivery_refunnels_capacity_and_preserves_order() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let first = cell(&origin, 1);
        let second = cell(&origin, 2);
        let (first_waiter, first_demand) =
            first.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let (second_waiter, second_demand) =
            second.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, first.id().clone(), first_demand)
                .unwrap();
        {
            let mut state = origin.state.lock();
            state.publish_demand(second.id().clone(), second_demand);
        }
        drop(delivery);

        let first_lease = OriginCell::take_ready_lease(&first, first_waiter)
            .expect("dropped delivery did not retry the original head");
        assert!(OriginCell::take_ready_lease(&second, second_waiter).is_none());
        assert_eq!(1, origin.counts().ordered);

        drop(first_lease);
        let second_lease = OriginCell::take_ready_lease(&second, second_waiter)
            .expect("younger demand did not run after the original head");
        drop(second_lease);
    }

    #[test]
    fn expired_requesting_cell_refunnels_capacity() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let requesting_cell = cell(&origin, 1);
        let requesting_cell_id = requesting_cell.id().clone();
        let (_waiter, snapshot) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, requesting_cell_id.clone(), snapshot)
                .unwrap();

        drop(requesting_cell);
        assert!(origin.cell(&requesting_cell_id).is_none());
        OriginAdmission::drive(Some(AdmissionAction::Delivery(delivery)));

        let counts = origin.counts();
        assert_eq!(1, counts.available);
        assert_eq!(0, counts.delivering);
        assert_eq!(0, counts.ordered);
    }

    #[test]
    fn separate_origins_conserve_capacity_independently() {
        let first = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let second = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let first_cell = cell(&first, 1);
        let second_cell = cell(&second, 1);

        let first_delivery =
            OriginAdmission::publish_without_driving(&first, first_cell.id().clone(), demand(1))
                .unwrap();
        let second_delivery =
            OriginAdmission::publish_without_driving(&second, second_cell.id().clone(), demand(1))
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
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::PartitionId;
    use http_1x::uri::Scheme;

    fn id() -> CellId {
        CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        )
    }

    fn demand() -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(1),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Compatible,
            EligibilityGroup::Pool,
        )
    }

    #[test]
    fn release_and_demand_publication_conserve_one_permit() {
        loom::model(|| {
            let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
            let permit = origin.state.lock().take_available().unwrap();
            let lease = CapacityLease::new(origin.clone(), permit);

            let release = loom::thread::spawn(move || drop(lease));
            let publish_origin = origin.clone();
            let publish = loom::thread::spawn(move || {
                let delivery =
                    OriginAdmission::publish_without_driving(&publish_origin, id(), demand());
                drop(delivery);
            });
            release.join().unwrap();
            publish.join().unwrap();

            assert_eq!(1, origin.counts().available);
            assert!(origin.counts().ordered <= 1);
        });
    }

    #[test]
    fn cancellation_preserves_an_outstanding_delivery_fence() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};

            let origin = OriginAdmission::new(NonZeroUsize::new(2).unwrap());
            let requesting_cell = id();
            let delivery = OriginAdmission::publish_without_driving(
                &origin,
                requesting_cell.clone(),
                demand(),
            )
            .unwrap();
            let release = Arc::new(AtomicBool::new(false));

            let delivery_release = release.clone();
            let dropped = loom::thread::spawn(move || {
                while !delivery_release.load(Ordering::Acquire) {
                    loom::thread::yield_now();
                }
                drop(delivery);
            });

            origin.state.lock().publish_demand(
                requesting_cell.clone(),
                DemandSnapshot::inactive(DemandId::from_u64(1), SnapshotVersion::INITIAL.next()),
            );
            let duplicate = OriginAdmission::publish_without_driving(
                &origin,
                requesting_cell,
                DemandSnapshot::active(
                    DemandId::from_u64(2),
                    SnapshotVersion::INITIAL,
                    ProtocolRequirement::H1Compatible,
                    EligibilityGroup::Pool,
                ),
            );

            release.store(true, Ordering::Release);
            dropped.join().unwrap();
            assert!(
                duplicate.is_none(),
                "outstanding delivery fence admitted a second delivery"
            );

            let counts = origin.counts();
            assert_eq!(2, counts.available);
            assert_eq!(0, counts.delivering);
            assert_eq!(0, counts.ordered);
        });
    }
}

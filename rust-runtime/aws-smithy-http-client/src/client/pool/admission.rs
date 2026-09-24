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

/// Protocol capability required by the head waiter in a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtocolRequirement {
    /// The waiter may dispatch over HTTP/1 or HTTP/2.
    H1Compatible,
    /// The waiter requires HTTP/2.
    H2Required,
}

/// Identity of one cell-local demand episode.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct DemandId(u64);

impl DemandId {
    /// Constructs an identity allocated by one cell's monotonic demand counter.
    pub(crate) const fn from_u64(value: u64) -> Self {
        Self(value)
    }
}

/// Version of a complete snapshot for one demand identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SnapshotVersion(u64);

impl SnapshotVersion {
    /// Version assigned to the first complete snapshot for a demand episode.
    pub(crate) const INITIAL: Self = Self(0);

    /// Returns the version for the next complete publication.
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

/// Versioned replacement state for one cell's current demand episode.
///
/// A publication replaces the complete previous snapshot. This prevents a
/// delayed active publication from reviving demand retired by a newer version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DemandSnapshot {
    /// Cell-local identity of the head-waiter episode.
    id: DemandId,
    /// Ordering among publications for the same demand identity.
    version: SnapshotVersion,
    /// Complete current state of the demand episode.
    state: DemandState,
}

impl DemandSnapshot {
    /// Describes the current head waiter for an active demand episode.
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

    /// Retires a demand episode at the supplied publication version.
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
    /// A newer demand episode supersedes every snapshot of an older episode;
    /// publications within one episode are ordered by snapshot version.
    fn is_newer_than(&self, current: &Self) -> bool {
        self.id > current.id || (self.id == current.id && self.version > current.version)
    }
}

/// Stable identity of one admitted connection slot within an origin.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PermitId(u64);

/// Stable identity of one admission-to-cell delivery.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DeliveryId(u64);

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
        let existing = {
            let mut state = origin.state.lock();
            match state.cells.get(&id).and_then(Weak::upgrade) {
                Some(existing) => Some(existing),
                None => {
                    state.cells.insert(id, Weak::from_arc(&candidate));
                    None
                }
            }
        };
        if let Some(existing) = existing {
            // Dropping the losing cell may drop a CapacityLease and re-enter
            // this admission lock, so release the lock first.
            drop(candidate);
            existing
        } else {
            candidate
        }
    }

    /// Publishes a complete demand snapshot and drives any resulting delivery.
    pub(crate) fn publish_demand(origin: &Arc<Self>, target: CellId, snapshot: DemandSnapshot) {
        let delivery = {
            let mut state = origin.state.lock();
            state.publish_demand(target, snapshot);
            Self::prepare_delivery(origin, &mut state)
        };
        Self::drive(delivery);
    }

    /// Extracts one permit and ordered demand into an unlocked delivery guard.
    fn prepare_delivery(
        origin: &Arc<Self>,
        state: &mut AdmissionState,
    ) -> Option<CapacityDelivery> {
        state.schedule_one().map(|pending| CapacityDelivery {
            origin: origin.clone(),
            delivery: pending.delivery,
            target: pending.target,
            demand: pending.demand,
            state: CapacityDeliveryState::Undelivered {
                permit: Some(pending.permit),
                on_drop: TargetAckResult::RetrySameResidence,
            },
        })
    }

    /// Upgrades a registered target without holding the admission lock.
    fn target(&self, id: &CellId) -> Option<Arc<OriginCell>> {
        let target = {
            let state = self.state.lock();
            state.cells.get(id).cloned()
        };
        target.and_then(|target| target.upgrade())
    }

    /// Drives sequential deliveries until no completion schedules another.
    pub(super) fn drive(mut delivery: Option<CapacityDelivery>) {
        while let Some(current) = delivery {
            delivery = current.deliver_once();
        }
    }

    /// Returns whether the named delivery still owns the target's demand fence.
    fn delivery_is_current(&self, delivery: DeliveryId, target: &CellId, demand: DemandId) -> bool {
        self.state
            .lock()
            .delivery_is_current(delivery, target, demand)
    }

    /// Returns `permit` to admission and serves ordered demand when possible.
    fn return_permit(origin: &Arc<Self>, permit: PermitId) {
        let delivery = {
            let mut state = origin.state.lock();
            state.available.push(permit);
            Self::prepare_delivery(origin, &mut state)
        };
        Self::drive(delivery);
    }

    /// Closes a delivery fence and prepares at most one successor.
    ///
    /// `permit` is present when an undelivered payload is refunnelled. A
    /// committed delivery leaves the permit in its installed
    /// [`CapacityLease`] and acknowledges with `None`.
    fn finish_delivery(
        origin: &Arc<Self>,
        delivery: DeliveryId,
        target: &CellId,
        permit: Option<PermitId>,
        result: TargetAckResult,
    ) -> Option<CapacityDelivery> {
        let mut state = origin.state.lock();
        if let Some(permit) = permit {
            state.available.push(permit);
        }
        state.finish_delivery(delivery, target, result);
        Self::prepare_delivery(origin, &mut state)
    }

    #[cfg(test)]
    pub(super) fn publish_without_driving(
        origin: &Arc<Self>,
        target: CellId,
        snapshot: DemandSnapshot,
    ) -> Option<CapacityDelivery> {
        let mut state = origin.state.lock();
        state.publish_demand(target, snapshot);
        Self::prepare_delivery(origin, &mut state)
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

    #[cfg(all(test, not(smithy_http_client_loom)))]
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

/// Admission's acknowledgement of a target-side delivery attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
enum TargetAckResult {
    /// The target accepted the payload and optionally published its successor demand.
    Accepted { successor: Option<DemandSnapshot> },
    /// The same demand remains live at its existing order position.
    RetrySameResidence,
    /// The old demand ended and may have a successor.
    Rejected { successor: Option<DemandSnapshot> },
}

/// An available bounded-origin slot being delivered to a waiting cell.
///
/// Until committed or rejected, this value owns both the permit and the
/// delivery fence. Dropping it refunnels the permit, restores the demand's
/// scheduling residence, and may drive the next delivery synchronously.
pub(crate) struct CapacityDelivery {
    /// Admission state that issued the permit.
    origin: Arc<OriginAdmission>,
    /// Identity of the outstanding delivery fence.
    delivery: DeliveryId,
    /// Cell selected while the admission lock was held.
    target: CellId,
    /// Demand episode the target must revalidate.
    demand: DemandId,
    /// Permit and fallback ownership before the terminal transition.
    state: CapacityDeliveryState,
}

impl CapacityDelivery {
    /// Returns the demand identity fenced by this delivery.
    pub(crate) fn demand(&self) -> DemandId {
        self.demand
    }

    /// Returns whether this guard still owns the target's delivery fence.
    pub(crate) fn is_current(&self) -> bool {
        self.origin
            .delivery_is_current(self.delivery, &self.target, self.demand)
    }

    /// Attempts one delivery to the registered target cell.
    ///
    /// A live target either accepts or rejects the guard. An expired target
    /// refunnels the permit and retires the fenced demand.
    fn deliver_once(self) -> Option<CapacityDelivery> {
        let target = self.origin.target(&self.target);
        match target {
            Some(target) => OriginCell::receive_capacity(&target, self),
            None => self.finish_undelivered(TargetAckResult::Rejected { successor: None }),
        }
    }

    /// Transfers the admitted slot to target-owned state.
    ///
    /// The returned acknowledgement keeps the scheduling fence installed
    /// until target state contains the lease.
    pub(crate) fn commit(
        mut self,
        successor: Option<DemandSnapshot>,
    ) -> (CapacityLease, CapacityDeliveryAck) {
        let CapacityDeliveryState::Undelivered { permit, .. } = &mut self.state else {
            unreachable!("capacity delivery committed after terminal transition");
        };
        let permit = permit
            .take()
            .expect("capacity delivery payload moved more than once");
        self.state = CapacityDeliveryState::Disarmed;

        (
            CapacityLease::new(self.origin.clone(), permit),
            CapacityDeliveryAck {
                origin: self.origin.clone(),
                delivery: self.delivery,
                target: self.target.clone(),
                result: Some(TargetAckResult::Accepted { successor }),
            },
        )
    }

    /// Refunnels this rejected delivery and returns the next scheduled crossing.
    pub(crate) fn reject(self, successor: Option<DemandSnapshot>) -> Option<CapacityDelivery> {
        self.finish_undelivered(TargetAckResult::Rejected { successor })
    }

    /// Disarms this guard and resolves its permit and demand fence together.
    fn finish_undelivered(mut self, result: TargetAckResult) -> Option<CapacityDelivery> {
        let permit = match &mut self.state {
            CapacityDeliveryState::Undelivered { permit, .. } => permit.take(),
            CapacityDeliveryState::Disarmed => None,
        };
        self.state = CapacityDeliveryState::Disarmed;
        OriginAdmission::finish_delivery(&self.origin, self.delivery, &self.target, permit, result)
    }
}

impl fmt::Debug for CapacityDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityDelivery")
            .field("delivery", &self.delivery)
            .field("target", &self.target)
            .field("demand", &self.demand)
            .field("state", &self.state)
            .finish()
    }
}

impl Drop for CapacityDelivery {
    fn drop(&mut self) {
        let CapacityDeliveryState::Undelivered { permit, on_drop } = &mut self.state else {
            return;
        };
        let permit = permit.take();
        let result = on_drop.clone();
        self.state = CapacityDeliveryState::Disarmed;
        let next = OriginAdmission::finish_delivery(
            &self.origin,
            self.delivery,
            &self.target,
            permit,
            result,
        );
        OriginAdmission::drive(next);
    }
}

/// Drop state for a permit crossing between admission and a cell.
#[derive(Debug)]
enum CapacityDeliveryState {
    /// The guard still owns the permit and fallback acknowledgement.
    Undelivered {
        permit: Option<PermitId>,
        on_drop: TargetAckResult,
    },
    /// Ownership and fallback responsibility moved to another guard.
    Disarmed,
}

/// Target-owned acknowledgement that closes one delivery fence on completion.
///
/// A committed delivery has already moved its lease into target-owned state.
/// This value is then the sole owner of fence completion; dropping it performs
/// the acknowledgement and drives any successor delivery.
#[derive(Debug)]
pub(crate) struct CapacityDeliveryAck {
    /// Admission authority that owns the fence.
    origin: Arc<OriginAdmission>,
    /// Identity of the outstanding fence.
    delivery: DeliveryId,
    /// Cell whose delivery is being acknowledged.
    target: CellId,
    /// Terminal result consumed by explicit completion or `Drop`.
    result: Option<TargetAckResult>,
}

impl CapacityDeliveryAck {
    /// Completes this delivery fence and returns the next scheduled crossing.
    pub(super) fn finish(mut self) -> Option<CapacityDelivery> {
        let result = self
            .result
            .take()
            .expect("new capacity delivery acknowledgement had no result");
        OriginAdmission::finish_delivery(&self.origin, self.delivery, &self.target, None, result)
    }
}

impl Drop for CapacityDeliveryAck {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let next = OriginAdmission::finish_delivery(
                &self.origin,
                self.delivery,
                &self.target,
                None,
                result,
            );
            OriginAdmission::drive(next);
        }
    }
}

/// Mutable bounded-origin state protected by one admission lock.
#[derive(Debug)]
struct AdmissionState {
    /// Cells available as delivery targets, held weakly to avoid an ownership cycle.
    cells: HashMap<CellId, Weak<OriginCell>>,
    /// Configured number of permits for this origin.
    limit: usize,
    /// Issued permits returned by failed attempts or closed connections.
    available: Vec<PermitId>,
    /// Permits not yet materialized from the configured limit.
    unissued: usize,
    /// Next never-reused permit identity.
    next_permit: u64,
    /// Cross-cell demand records and their origin-wide scheduling order.
    demand_schedule: DemandSchedule,
    /// Next never-reused delivery identity.
    next_delivery: u64,
}

impl AdmissionState {
    /// Creates a lazy permit ledger with no materialized permit identities.
    fn new(limit: NonZeroUsize) -> Self {
        let limit = limit.get();
        Self {
            cells: HashMap::new(),
            limit,
            available: Vec::new(),
            unissued: limit,
            next_permit: 0,
            demand_schedule: DemandSchedule::default(),
            next_delivery: 0,
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
    fn publish_demand(&mut self, target: CellId, snapshot: DemandSnapshot) {
        self.demand_schedule.publish(target, snapshot);
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
        Some(PreparedCapacityDelivery {
            permit,
            delivery,
            target: scheduled.target,
            demand: scheduled.demand,
            sequence: scheduled.sequence,
        })
    }

    /// Delegates delivery-fence revalidation to the demand schedule.
    fn delivery_is_current(&self, delivery: DeliveryId, target: &CellId, demand: DemandId) -> bool {
        self.demand_schedule
            .delivery_is_current(delivery, target, demand)
    }

    /// Applies a target acknowledgement to the demand schedule.
    fn finish_delivery(&mut self, delivery: DeliveryId, target: &CellId, result: TargetAckResult) {
        self.demand_schedule
            .finish_delivery(delivery, target, result);
    }

    /// Allocates a delivery-fence identity that is never reused by this origin.
    fn take_delivery_id(&mut self) -> DeliveryId {
        let value = self.next_delivery;
        self.next_delivery = value.checked_add(1).expect("delivery identity exhausted");
        DeliveryId(value)
    }
}

/// Cross-cell demand records and their origin-wide scheduling order.
///
/// At every completed transition:
///
/// - `records` owns the newest snapshot for every retained cell.
/// - `DemandOrderState::Active` contains exactly the `Queued` and `Delivering`
///   records.
/// - Queue links live inside those ordered residence variants.
/// - A `Delivering` record remains at the head as a scheduling fence.
///
/// Admission coordinates permit extraction with this schedule while holding
/// the same origin lock.
#[derive(Debug, Default)]
struct DemandSchedule {
    /// Latest demand and scheduling residence for each retained cell.
    records: HashMap<CellId, DemandRecord>,
    /// Origin-wide order, including an outstanding delivery fence.
    order: DemandOrderState,
    /// Next never-reused arrival sequence.
    next_sequence: u64,
}

impl DemandSchedule {
    /// Applies a complete snapshot and updates its cell's scheduling residence.
    fn publish(&mut self, target: CellId, snapshot: DemandSnapshot) {
        if let Some(current) = self.records.get(&target) {
            if !snapshot.is_newer_than(&current.latest) {
                return;
            }
        } else {
            self.records.insert(
                target.clone(),
                DemandRecord {
                    latest: snapshot.clone(),
                    residence: DemandResidence::Idle,
                },
            );
        }

        let should_remove = matches!(
            &self
                .records
                .get(&target)
                .expect("published demand record disappeared")
                .residence,
            DemandResidence::Queued { demand, .. }
                if *demand != snapshot.id || !snapshot.is_active()
        );
        if should_remove {
            self.remove_from_order(&target);
        }

        self.records
            .get_mut(&target)
            .expect("published demand record disappeared")
            .latest = snapshot;

        let record = self
            .records
            .get(&target)
            .expect("published demand record disappeared");
        if record.latest.is_active() && matches!(&record.residence, DemandResidence::Idle) {
            self.enqueue(target);
        }
        self.assert_consistent();
    }

    /// Appends an idle active demand to the origin-wide order.
    fn enqueue(&mut self, target: CellId) {
        let sequence = self.take_sequence();
        let previous = match &self.order {
            DemandOrderState::Empty => None,
            DemandOrderState::Active { tail, .. } => Some(tail.clone()),
        };
        if let Some(previous) = previous.as_ref() {
            self.records
                .get_mut(previous)
                .expect("order tail disappeared")
                .residence
                .links_mut()
                .next = Some(target.clone());
        }

        let record = self
            .records
            .get_mut(&target)
            .expect("queued demand record disappeared");
        debug_assert!(matches!(record.residence, DemandResidence::Idle));
        let demand = record.latest.id;
        record.residence = DemandResidence::Queued {
            demand,
            links: OrderLinks {
                previous,
                next: None,
                sequence,
            },
        };

        match &mut self.order {
            order @ DemandOrderState::Empty => {
                *order = DemandOrderState::Active {
                    head: target.clone(),
                    tail: target,
                    len: NonZeroUsize::MIN,
                };
            }
            DemandOrderState::Active { tail, len, .. } => {
                *tail = target;
                *len = len.checked_add(1).expect("demand order length exhausted");
            }
        }
    }

    /// Removes an ordered demand and leaves its retained record idle.
    fn remove_from_order(&mut self, target: &CellId) {
        let residence = {
            let record = self
                .records
                .get_mut(target)
                .expect("removed demand record disappeared");
            std::mem::replace(&mut record.residence, DemandResidence::Idle)
        };
        let links = residence
            .into_links()
            .expect("removed demand had no order links");

        if let Some(previous) = links.previous.as_ref() {
            self.records
                .get_mut(previous)
                .expect("previous demand disappeared")
                .residence
                .links_mut()
                .next = links.next.clone();
        }
        if let Some(next) = links.next.as_ref() {
            self.records
                .get_mut(next)
                .expect("next demand disappeared")
                .residence
                .links_mut()
                .previous = links.previous.clone();
        }

        let order = std::mem::take(&mut self.order);
        let DemandOrderState::Active { head, tail, len } = order else {
            unreachable!("removed a demand from an empty order");
        };
        debug_assert_eq!(head == *target, links.previous.is_none());
        debug_assert_eq!(tail == *target, links.next.is_none());

        if len == NonZeroUsize::MIN {
            debug_assert!(links.previous.is_none());
            debug_assert!(links.next.is_none());
            return;
        }

        self.order = DemandOrderState::Active {
            head: if head == *target {
                links.next.clone().expect("removed head had no successor")
            } else {
                head
            },
            tail: if tail == *target {
                links
                    .previous
                    .clone()
                    .expect("removed tail had no predecessor")
            } else {
                tail
            },
            len: NonZeroUsize::new(
                len.get()
                    .checked_sub(1)
                    .expect("demand order length underflowed"),
            )
            .expect("nonempty demand order lost its length"),
        };
    }

    /// Returns whether the head can begin a new one-to-one delivery.
    fn head_is_queued(&self) -> bool {
        let DemandOrderState::Active { head, .. } = &self.order else {
            return false;
        };
        matches!(
            &self
                .records
                .get(head)
                .expect("order head disappeared")
                .residence,
            DemandResidence::Queued { .. }
        )
    }

    /// Changes the queued head into a delivery fence at the same order position.
    fn reserve_head(&mut self, delivery: DeliveryId) -> Option<ScheduledDemand> {
        let DemandOrderState::Active { head, .. } = &self.order else {
            return None;
        };
        let target = head.clone();
        let record = self
            .records
            .get_mut(&target)
            .expect("order head disappeared");
        let residence = std::mem::replace(&mut record.residence, DemandResidence::Idle);
        match residence {
            DemandResidence::Queued { demand, links } => {
                debug_assert_eq!(record.latest.id, demand);
                debug_assert!(record.latest.is_active());
                let sequence = links.sequence;
                record.residence = DemandResidence::Delivering {
                    demand,
                    delivery,
                    links,
                };
                self.assert_consistent();
                Some(ScheduledDemand {
                    target,
                    demand,
                    sequence,
                })
            }
            residence => {
                record.residence = residence;
                None
            }
        }
    }

    /// Returns whether a delivery still owns the target demand's head fence.
    ///
    /// Cell-side reservation uses this identity check to reject a crossing
    /// prepared for demand that was replaced while no pool lock was held.
    fn delivery_is_current(&self, delivery: DeliveryId, target: &CellId, demand: DemandId) -> bool {
        let Some(record) = self.records.get(target) else {
            return false;
        };
        matches!(
            &record.residence,
            DemandResidence::Delivering {
                demand: current_demand,
                delivery: current_delivery,
                ..
            } if *current_delivery == delivery
                && *current_demand == demand
                && record.latest.id == demand
                && record.latest.is_active()
        )
    }

    /// Closes one delivery fence and resolves the demand's next residence.
    ///
    /// `Accepted` consumes the delivered episode and installs a newer
    /// successor when the target supplied one. `RetrySameResidence` restores
    /// the same episode at its existing order position when it remains live;
    /// if it was replaced, the latest active episode is appended as new
    /// demand. `Rejected` removes the fenced episode after its permit has
    /// already been refunnelled and applies the same successor arbitration as
    /// acceptance.
    fn finish_delivery(&mut self, delivery: DeliveryId, target: &CellId, result: TargetAckResult) {
        let Some(record) = self.records.get(target) else {
            return;
        };
        let delivered_demand = match &record.residence {
            DemandResidence::Delivering {
                demand,
                delivery: current,
                ..
            } if *current == delivery => *demand,
            _ => return,
        };

        match result {
            TargetAckResult::RetrySameResidence => {
                let record = self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared");
                if record.latest.id == delivered_demand && record.latest.is_active() {
                    let record = self
                        .records
                        .get_mut(target)
                        .expect("delivery demand record disappeared");
                    let residence = std::mem::replace(&mut record.residence, DemandResidence::Idle);
                    let DemandResidence::Delivering {
                        demand,
                        delivery: current,
                        links,
                    } = residence
                    else {
                        unreachable!("delivery fence disappeared");
                    };
                    debug_assert_eq!(current, delivery);
                    record.residence = DemandResidence::Queued { demand, links };
                    self.assert_consistent();
                    return;
                }

                self.remove_from_order(target);
                if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(target.clone());
                }
            }
            TargetAckResult::Accepted { successor } | TargetAckResult::Rejected { successor } => {
                self.remove_from_order(target);

                let install_successor = successor.as_ref().is_some_and(|successor| {
                    successor.id > delivered_demand
                        && successor.is_newer_than(
                            &self
                                .records
                                .get(target)
                                .expect("delivery demand record disappeared")
                                .latest,
                        )
                });
                if install_successor {
                    self.records
                        .get_mut(target)
                        .expect("delivery demand record disappeared")
                        .latest = successor.expect("validated successor disappeared");
                } else if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .id
                    == delivered_demand
                {
                    let record = self
                        .records
                        .get_mut(target)
                        .expect("delivery demand record disappeared");
                    record.latest =
                        DemandSnapshot::inactive(delivered_demand, record.latest.version.next());
                }

                if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(target.clone());
                }
            }
        }
        self.assert_consistent();
    }

    /// Allocates a shared arrival sequence that is never reused.
    fn take_sequence(&mut self) -> u64 {
        let value = self.next_sequence;
        self.next_sequence = value.checked_add(1).expect("demand sequence exhausted");
        value
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        match &self.order {
            DemandOrderState::Empty => 0,
            DemandOrderState::Active { len, .. } => len.get(),
        }
    }

    #[cfg(test)]
    fn queued_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| matches!(&record.residence, DemandResidence::Queued { .. }))
            .count()
    }

    #[cfg(test)]
    fn delivering_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| matches!(&record.residence, DemandResidence::Delivering { .. }))
            .count()
    }

    /// Checks record residence, FIFO links, length, and head-fence relationships.
    fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        self.assert_consistent_debug();
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        let ordered_records = self
            .records
            .values()
            .filter(|record| record.residence.links().is_some())
            .count();
        match &self.order {
            DemandOrderState::Empty => {
                assert_eq!(0, ordered_records, "empty order retained ordered demand");
            }
            DemandOrderState::Active { head, tail, len } => {
                let mut current = Some(head.clone());
                let mut previous = None;
                let mut traversed = 0;
                let mut delivering = 0;
                while let Some(target) = current {
                    assert!(
                        traversed < self.records.len(),
                        "demand order contains a cycle"
                    );
                    let record = self
                        .records
                        .get(&target)
                        .expect("ordered demand disappeared");
                    let links = record
                        .residence
                        .links()
                        .expect("ordered demand lost its links");
                    assert_eq!(
                        previous, links.previous,
                        "demand order contains inconsistent backward links"
                    );
                    match &record.residence {
                        DemandResidence::Idle => unreachable!("ordered demand became idle"),
                        DemandResidence::Queued { demand, .. } => {
                            assert!(record.latest.is_active(), "queued demand became inactive");
                            assert_eq!(
                                record.latest.id, *demand,
                                "queued residence did not match its latest demand"
                            );
                        }
                        DemandResidence::Delivering { demand, .. } => {
                            delivering += 1;
                            assert_eq!(
                                target, *head,
                                "delivery fence moved away from the order head"
                            );
                            assert!(
                                record.latest.id >= *demand,
                                "delivery fence named a future demand"
                            );
                        }
                    }

                    traversed += 1;
                    previous = Some(target);
                    current = links.next.clone();
                }

                assert!(delivering <= 1, "more than one delivery fence was active");
                assert_eq!(Some(tail.clone()), previous, "order tail was not reachable");
                assert_eq!(
                    len.get(),
                    traversed,
                    "demand order length did not match its links"
                );
                assert_eq!(
                    ordered_records, traversed,
                    "ordered demand was not reachable from the order head"
                );
            }
        }
    }
}

/// Latest snapshot and scheduling residence for one stable cell.
#[derive(Debug)]
struct DemandRecord {
    /// Newest complete publication observed for the cell.
    latest: DemandSnapshot,
    /// Scheduling residence, including links while ordered.
    residence: DemandResidence,
}

/// Admission residence for one cell's latest demand.
#[derive(Clone, Debug)]
enum DemandResidence {
    /// The cell has no demand represented in scheduling.
    Idle,
    /// The demand is waiting in origin order.
    Queued {
        /// Demand episode represented by this residence.
        demand: DemandId,
        /// Origin-wide scheduling links and arrival sequence.
        links: OrderLinks,
    },
    /// One delivery guard owns capacity for this demand.
    Delivering {
        /// Demand episode fenced at the order head.
        demand: DemandId,
        /// Delivery allowed to complete this fence.
        delivery: DeliveryId,
        /// Origin-wide scheduling links retained until acknowledgement.
        links: OrderLinks,
    },
}

impl DemandResidence {
    /// Borrows order links while this record is queued or delivering.
    fn links(&self) -> Option<&OrderLinks> {
        match self {
            Self::Idle => None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => Some(links),
        }
    }

    /// Mutably borrows links from a record known to be ordered.
    ///
    /// # Panics
    ///
    /// Panics when called for an idle record.
    fn links_mut(&mut self) -> &mut OrderLinks {
        match self {
            Self::Idle => panic!("idle demand has no order links"),
            Self::Queued { links, .. } | Self::Delivering { links, .. } => links,
        }
    }

    /// Detaches links while moving a record out of origin-wide order.
    fn into_links(self) -> Option<OrderLinks> {
        match self {
            Self::Idle => None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => Some(links),
        }
    }
}

/// Whether origin-wide scheduling currently retains any demand.
#[derive(Debug, Default)]
enum DemandOrderState {
    /// No demand is ordered.
    #[default]
    Empty,
    /// A nonempty origin-wide order, possibly headed by a delivery fence.
    Active {
        /// Oldest ordered demand.
        head: CellId,
        /// Youngest ordered demand.
        tail: CellId,
        /// Number of records linked from `head` through `tail`.
        len: NonZeroUsize,
    },
}

/// Intrusive links retaining one live demand in origin order.
#[derive(Clone, Debug)]
struct OrderLinks {
    /// Older demand in origin order.
    previous: Option<CellId>,
    /// Newer demand in origin order.
    next: Option<CellId>,
    /// Monotonic arrival order shared with other scheduling views.
    sequence: u64,
}

/// Demand reserved at the order head for one capacity delivery.
struct ScheduledDemand {
    /// Selected destination cell.
    target: CellId,
    /// Demand identity current when the crossing was prepared.
    demand: DemandId,
    /// Demand's common arrival order for scheduling-view updates.
    sequence: u64,
}

/// Connection capacity extracted under admission lock before crossing to a cell.
#[derive(Debug)]
struct PreparedCapacityDelivery {
    /// Permit transferred into the delivery guard.
    permit: PermitId,
    /// Fence identity allocated for this crossing.
    delivery: DeliveryId,
    /// Selected destination cell.
    target: CellId,
    /// Demand identity current when the crossing was prepared.
    demand: DemandId,
    /// Demand's common arrival order for scheduling-view updates.
    sequence: u64,
}

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
struct AdmissionCounts {
    limit: usize,
    available: usize,
    ordered: usize,
    queued: usize,
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
        let target = CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );
        let current = demand(2);
        state.publish_demand(target.clone(), current.clone());
        state.publish_demand(
            target.clone(),
            DemandSnapshot::inactive(DemandId::from_u64(2), SnapshotVersion::INITIAL),
        );
        state.publish_demand(target.clone(), demand(1));

        assert_eq!(1, state.demand_schedule.len());
        assert_eq!(
            current,
            state.demand_schedule.records.get(&target).unwrap().latest
        );
    }

    #[test]
    fn cancellation_churn_does_not_retain_order_entries() {
        let mut state = AdmissionState::new(NonZeroUsize::new(1).unwrap());
        let held = state.take_available().unwrap();
        let target = CellId::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        );

        for id in 0..2_000 {
            let id = DemandId::from_u64(id);
            state.publish_demand(
                target.clone(),
                DemandSnapshot::active(
                    id,
                    SnapshotVersion::INITIAL,
                    ProtocolRequirement::H1Compatible,
                    EligibilityGroup::Pool,
                ),
            );
            state.publish_demand(
                target.clone(),
                DemandSnapshot::inactive(id, SnapshotVersion::INITIAL.next()),
            );
        }

        assert_eq!(0, state.demand_schedule.len());
        assert!(matches!(
            state.demand_schedule.order,
            DemandOrderState::Empty
        ));
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

        for (index, target) in targets[..4].iter().enumerate() {
            state.publish_demand(target.clone(), demand(index as u64 + 1));
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
            assert_eq!(expected, &pending.target);
            state.finish_delivery(
                pending.delivery,
                &pending.target,
                TargetAckResult::Accepted { successor: None },
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

        assert_eq!(second, state.schedule_one().unwrap().target);
    }

    #[test]
    fn delivery_currency_includes_the_demand_id() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let target = cell(&origin, 1);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, target.id().clone(), demand(1))
                .unwrap();
        assert!(delivery.is_current());

        origin
            .state
            .lock()
            .publish_demand(target.id().clone(), demand(2));
        assert!(!delivery.is_current());
        OriginAdmission::drive(delivery.reject(None));
    }

    #[test]
    fn stale_successor_cannot_leave_active_demand_idle() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let target = cell(&origin, 1);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, target.id().clone(), demand(1))
                .unwrap();
        origin.state.lock().publish_demand(
            target.id().clone(),
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL.next(),
                ProtocolRequirement::H1Compatible,
                EligibilityGroup::Pool,
            ),
        );
        OriginAdmission::drive(delivery.reject(Some(demand(1))));

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

        let first_lease = first
            .take_ready_lease(first_waiter)
            .expect("dropped delivery did not retry the original head");
        assert!(second.take_ready_lease(second_waiter).is_none());
        assert_eq!(1, origin.counts().ordered);

        drop(first_lease);
        let second_lease = second
            .take_ready_lease(second_waiter)
            .expect("younger demand did not run after the original head");
        drop(second_lease);
    }

    #[test]
    fn losing_registered_cell_returns_capacity_after_admission_unlocks() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let retained = cell(&origin, 1);
        let candidate = Arc::new(OriginCell::new(
            retained.id().partition(),
            retained.id().origin().clone(),
            EligibilityGroup::Pool,
            Some(origin.clone()),
        ));
        assert_eq!(retained.id(), candidate.id());

        let (_waiter, snapshot) =
            candidate.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, candidate.id().clone(), snapshot)
                .expect("candidate demand did not reserve capacity");
        assert!(OriginCell::receive_capacity(&candidate, delivery).is_none());
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
    fn expired_delivery_target_refunnels_capacity() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let target = cell(&origin, 1);
        let target_id = target.id().clone();
        let (_waiter, snapshot) =
            target.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let delivery =
            OriginAdmission::publish_without_driving(&origin, target_id.clone(), snapshot).unwrap();

        drop(target);
        assert!(origin.target(&target_id).is_none());
        OriginAdmission::drive(Some(delivery));

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
        assert_eq!(
            first_delivery
                .state
                .permit_id()
                .expect("first delivery lost permit"),
            second_delivery
                .state
                .permit_id()
                .expect("second delivery lost permit")
        );
    }

    impl CapacityDeliveryState {
        fn permit_id(&self) -> Option<u64> {
            match self {
                Self::Undelivered { permit, .. } => permit.map(|permit| permit.0),
                Self::Disarmed => None,
            }
        }
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
            let target = id();
            let delivery =
                OriginAdmission::publish_without_driving(&origin, target.clone(), demand())
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
                target.clone(),
                DemandSnapshot::inactive(DemandId::from_u64(1), SnapshotVersion::INITIAL.next()),
            );
            let duplicate = OriginAdmission::publish_without_driving(
                &origin,
                target,
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

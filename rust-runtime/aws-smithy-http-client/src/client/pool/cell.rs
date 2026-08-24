/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! State local to one partition and origin.
//!
//! An [`OriginCell`] owns its waiter order and delivered results. A bounded
//! cell publishes aggregate demand to [`OriginAdmission`] after releasing its
//! lock, and admission returns capacity through the same unlocked boundary.
//!
//! Each waiter follows one cell-local ownership path:
//!
//! ```text
//! Waiting --delivery reserved--> Receiving --delivery lands--> Ready --poll--> consumed
//!    |                              |                           |
//!    | cancel                       | cancel                    | cancel
//!    v                              v                           v
//! removed                 CancelledReceiving          removed + lease refunnelled
//!   |                              |
//!   | head: retire/replace demand  | delivery lands
//!   | non-head: demand unchanged   v
//!   `----------------------> removed + lease refunnelled
//! ```

mod waiters;

#[cfg(test)]
use self::waiters::CellSnapshot;
use self::waiters::{DeliveryReservation, LeaseInstallError, WaiterId, WaiterQueue};
#[cfg(test)]
use super::admission::DemandSnapshot;
use super::admission::{CapacityDelivery, CapacityLease, OriginAdmission, ProtocolRequirement};
use super::origin::OriginKey;
use super::partition::{EligibilityGroup, PartitionId};
use crate::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// Stable state shared by requests for one partition and canonical origin.
pub(crate) struct OriginCell {
    /// Complete identity used by partition and admission indexes.
    id: CellId,
    /// Partitions whose connections may satisfy this cell's demand.
    eligibility_group: EligibilityGroup,
    /// Origin-wide authority present only when capacity is bounded.
    admission: Option<Arc<OriginAdmission>>,
    /// Waiter order, aggregate demand, and delivered results.
    state: Mutex<WaiterQueue>,
}

impl OriginCell {
    /// Creates a cell before its one-time publication in a partition registry.
    pub(super) fn new(
        partition: PartitionId,
        origin: OriginKey,
        eligibility_group: EligibilityGroup,
        admission: Option<Arc<OriginAdmission>>,
    ) -> Self {
        Self {
            id: CellId::new(partition, origin),
            eligibility_group,
            admission,
            state: Mutex::new(WaiterQueue::default()),
        }
    }

    /// Returns this cell's stable partition-and-origin identity.
    pub(crate) fn id(&self) -> &CellId {
        &self.id
    }

    /// Returns the set of partitions eligible to use this cell's connections.
    pub(crate) fn eligibility_group(&self) -> &EligibilityGroup {
        &self.eligibility_group
    }

    /// Returns this cell's origin-wide admission authority, when bounded.
    pub(crate) fn admission(&self) -> Option<&Arc<OriginAdmission>> {
        self.admission.as_ref()
    }

    /// Registers one bounded-capacity waiter in cell-local arrival order.
    ///
    /// The waiter and its demand snapshot are committed under the cell lock.
    /// Publication to origin admission happens only after that lock is
    /// released.
    pub(super) fn register_waiter(&self, requirement: ProtocolRequirement) -> WaiterId {
        let admission = self
            .admission
            .as_ref()
            .expect("capacity waiter registered for an unbounded origin");
        let (waiter, snapshot) = {
            let mut state = self.state.lock();
            state.register_waiter(requirement, &self.eligibility_group)
        };

        if let Some(snapshot) = snapshot {
            OriginAdmission::publish_demand(admission, self.id.clone(), snapshot);
        }
        waiter
    }

    /// Cancels a waiter and refunnels any capacity already delivered to it.
    pub(super) fn cancel_waiter(&self, waiter: WaiterId) -> bool {
        let admission = self
            .admission
            .as_ref()
            .expect("capacity waiter cancelled for an unbounded origin");
        let Some(cancelled) = ({
            let mut state = self.state.lock();
            state.cancel_waiter(waiter, &self.eligibility_group)
        }) else {
            return false;
        };

        for snapshot in cancelled.demand_updates.into_iter().flatten() {
            OriginAdmission::publish_demand(admission, self.id.clone(), snapshot);
        }

        // A ready waiter owns its lease in cell state. Cancellation transfers
        // that ownership here so dropping it cannot nest cell and admission
        // locks.
        drop(cancelled.returned_lease);
        true
    }

    /// Polls the capacity reserved for one waiter.
    ///
    /// # Panics
    ///
    /// Panics if `waiter` is unknown, was cancelled, or is polled again after
    /// its ready lease was consumed.
    pub(super) fn poll_waiter(
        &self,
        waiter: WaiterId,
        cx: &mut Context<'_>,
    ) -> Poll<CapacityLease> {
        let mut state = self.state.lock();
        state.poll_waiter(waiter, cx)
    }

    /// Applies a capacity delivery after the admission-to-cell lock crossing.
    ///
    /// The returned action is driven by admission after this method has
    /// released the cell lock. No cell and admission locks are nested.
    pub(super) fn receive_capacity(
        cell: &Arc<Self>,
        delivery: CapacityDelivery,
    ) -> Option<CapacityDelivery> {
        let reservation = {
            let mut state = cell.state.lock();
            state.reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
        };

        let DeliveryReservation::Reserved { waiter, successor } = reservation else {
            delivery.reject(None);
            return None;
        };

        let (lease, acknowledgement) = delivery.commit(successor);
        let installation = {
            let mut state = cell.state.lock();
            state.install_delivered_lease(waiter, lease)
        };

        // Cancellation may win after the delivery was reserved but before its
        // lease was installed. Return that lease before closing the fence so a
        // successor is scheduled by the fence acknowledgement.
        drop(installation.returned_lease);
        let next = acknowledgement.finish();
        if let Some(waker) = installation.waker {
            waker.wake();
        }
        if let Some(error) = installation.error {
            match error {
                LeaseInstallError::MissingWaiter => {
                    panic!("reserved waiter disappeared before capacity delivery")
                }
                LeaseInstallError::UnexpectedState => {
                    panic!("reserved waiter entered an invalid capacity-delivery state")
                }
            }
        }
        next
    }

    #[cfg(test)]
    pub(super) fn take_ready_lease(&self, waiter: WaiterId) -> Option<CapacityLease> {
        self.state.lock().take_ready_lease(waiter)
    }

    #[cfg(test)]
    pub(super) fn register_waiter_without_publish(
        &self,
        requirement: ProtocolRequirement,
    ) -> (WaiterId, DemandSnapshot) {
        let (waiter, snapshot) = self
            .state
            .lock()
            .register_waiter(requirement, &self.eligibility_group);
        (
            waiter,
            snapshot.expect("first unpublished waiter did not create demand"),
        )
    }

    #[cfg(test)]
    fn snapshot(&self) -> CellSnapshot {
        self.state.lock().snapshot()
    }
}

impl std::fmt::Debug for OriginCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginCell")
            .field("id", &self.id)
            .field("eligibility_group", &self.eligibility_group)
            .field("admission", &self.admission)
            .field("state", &self.state)
            .finish()
    }
}

/// Stable identity of an [`OriginCell`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CellId {
    partition: PartitionId,
    origin: OriginKey,
}

impl CellId {
    /// Creates an identity from its complete partition and origin axes.
    pub(super) fn new(partition: PartitionId, origin: OriginKey) -> Self {
        Self { partition, origin }
    }

    /// Returns the partition axis.
    pub(crate) fn partition(&self) -> PartitionId {
        self.partition
    }

    /// Returns the canonical origin axis.
    pub(crate) fn origin(&self) -> &OriginKey {
        &self.origin
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use http_1x::uri::Scheme;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;
    use std::task::{Wake, Waker};

    struct WakeCounter(AtomicUsize);

    impl Wake for WakeCounter {
        fn wake(self: StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn wake_by_ref(self: &StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        let admission = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let candidate = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(admission.clone()),
        ));
        let cell = OriginAdmission::register_cell(&admission, candidate);
        (admission, cell)
    }

    fn saturated_bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>, CapacityLease) {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        (admission, cell, lease)
    }

    #[test]
    fn three_waiters_receive_capacity_in_fifo_order() {
        let (_admission, cell) = bounded_cell();
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);

        let first_lease = cell.take_ready_lease(first).unwrap();
        assert!(cell.take_ready_lease(second).is_none());
        assert!(cell.take_ready_lease(third).is_none());

        drop(first_lease);
        let second_lease = cell.take_ready_lease(second).unwrap();
        assert!(cell.take_ready_lease(third).is_none());

        drop(second_lease);
        let third_lease = cell.take_ready_lease(third).unwrap();
        drop(third_lease);
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_middle_and_tail_waiters_repairs_fifo_links() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H2Required);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let fourth = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let demand = cell.snapshot().demand;

        assert!(cell.cancel_waiter(second));
        assert!(cell.cancel_waiter(fourth));
        assert_eq!(demand, cell.snapshot().demand);
        assert_eq!(2, cell.snapshot().waiting);
        let fifth = cell.register_waiter(ProtocolRequirement::H1Compatible);
        assert_eq!(3, cell.snapshot().waiting);

        drop(held);
        let first_lease = cell.take_ready_lease(first).unwrap();
        assert!(cell.take_ready_lease(third).is_none());
        assert!(cell.take_ready_lease(fifth).is_none());
        drop(first_lease);

        let third_lease = cell.take_ready_lease(third).unwrap();
        assert!(cell.take_ready_lease(fifth).is_none());
        drop(third_lease);

        drop(cell.take_ready_lease(fifth).unwrap());
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_head_waiter_preserves_remaining_fifo_order() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let fourth = cell.register_waiter(ProtocolRequirement::H1Compatible);

        assert!(cell.cancel_waiter(first));
        drop(held);

        let second_lease = cell.take_ready_lease(second).unwrap();
        assert!(cell.take_ready_lease(third).is_none());
        assert!(cell.take_ready_lease(fourth).is_none());
        drop(second_lease);

        let third_lease = cell.take_ready_lease(third).unwrap();
        assert!(cell.take_ready_lease(fourth).is_none());
        drop(third_lease);

        drop(cell.take_ready_lease(fourth).unwrap());
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_ready_waiter_refunnels_capacity() {
        let (_admission, cell) = bounded_cell();
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);

        assert!(cell.cancel_waiter(first));
        let lease = cell.take_ready_lease(second).unwrap();
        drop(lease);
    }

    #[test]
    fn cancelling_during_delivery_refunnels_capacity_after_unlock() {
        let (admission, cell) = bounded_cell();
        let (waiter, demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let delivery =
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
                .expect("published demand did not reserve capacity");
        let reservation = {
            let mut state = cell.state.lock();
            state.reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
        };
        let DeliveryReservation::Reserved {
            waiter: reserved,
            successor,
        } = reservation
        else {
            panic!("current delivery was rejected");
        };
        assert_eq!(waiter, reserved);
        let (lease, acknowledgement) = delivery.commit(successor);

        assert!(cell.cancel_waiter(waiter));
        let installation = {
            let mut state = cell.state.lock();
            state.install_delivered_lease(waiter, lease)
        };
        assert!(installation.returned_lease.is_some());
        assert!(installation.error.is_none());
        assert_eq!(0, cell.snapshot().retained);

        drop(installation.returned_lease);
        assert_eq!(1, admission.available_capacity_for_test());
        assert!(acknowledgement.finish().is_none());
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn stale_delivery_is_rejected_after_head_cancellation() {
        let (admission, cell) = bounded_cell();
        let (first, first_demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let stale =
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), first_demand)
                .expect("first demand did not reserve capacity");

        assert!(cell.cancel_waiter(first));
        assert!(OriginCell::receive_capacity(&cell, stale).is_none());

        let lease = cell
            .take_ready_lease(second)
            .expect("replacement demand did not receive refunnelled capacity");
        drop(lease);
    }

    #[test]
    fn cancelling_the_only_waiter_retires_admission_demand() {
        let (admission, cell, held) = saturated_bounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        assert_eq!(1, admission.ordered_demand_count_for_test());

        assert!(cell.cancel_waiter(waiter));
        assert_eq!(0, admission.ordered_demand_count_for_test());
        drop(held);
    }

    #[test]
    fn unknown_waiter_cancellation_is_a_noop() {
        let (_admission, cell) = bounded_cell();
        assert!(!cell.cancel_waiter(WaiterId(99)));
    }

    #[test]
    #[should_panic(expected = "capacity waiter registered for an unbounded origin")]
    fn capacity_waiter_requires_bounded_admission() {
        let cell = OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
        );

        cell.register_waiter(ProtocolRequirement::H1Compatible);
    }

    #[test]
    #[should_panic(expected = "capacity waiter cancelled for an unbounded origin")]
    fn capacity_waiter_cancellation_requires_bounded_admission() {
        let cell = OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
        );

        cell.cancel_waiter(WaiterId(0));
    }

    #[test]
    fn polling_waiter_records_a_waker() {
        let (_admission, cell, _held) = saturated_bounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        assert!(cell.poll_waiter(waiter, &mut context).is_pending());
        assert!(cell.cancel_waiter(waiter));
    }

    #[test]
    fn delivered_waiter_is_woken_once() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let counter = StdArc::new(WakeCounter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut context = Context::from_waker(&waker);
        assert!(cell.poll_waiter(waiter, &mut context).is_pending());

        drop(held);
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
        drop(
            cell.take_ready_lease(waiter)
                .expect("woken waiter had no capacity"),
        );
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use http_1x::uri::Scheme;
    use std::num::NonZeroUsize;

    fn bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        let admission = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let candidate = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(admission.clone()),
        ));
        let cell = OriginAdmission::register_cell(&admission, candidate);
        (admission, cell)
    }

    #[test]
    fn delivery_and_cancellation_race_refunnels_capacity() {
        loom::model(|| {
            let (admission, cell) = bounded_cell();
            let (first, demand) =
                cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let delivery =
                OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
                    .unwrap();

            let delivery_cell = cell.clone();
            let deliver = loom::thread::spawn(move || {
                drop(OriginCell::receive_capacity(&delivery_cell, delivery));
            });
            let cancel_cell = cell.clone();
            let cancel = loom::thread::spawn(move || cancel_cell.cancel_waiter(first));
            deliver.join().unwrap();
            cancel.join().unwrap();

            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }
}

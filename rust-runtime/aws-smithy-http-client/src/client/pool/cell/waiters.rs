/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cell-local waiter order and delivered-capacity ownership.
//!
//! [`WaiterQueue`] is the complete mutable state behind an origin cell's lock.
//! It keeps the request FIFO, aggregate head demand, delivery crossing state,
//! and any delivered lease in one invariant domain. Values that require
//! admission locking or task wakeup are detached for the caller to process
//! only after releasing the cell lock.

use super::super::admission::{
    CapacityLease, DemandId, DemandSnapshot, ProtocolRequirement, SnapshotVersion,
};
use super::super::partition::EligibilityGroup;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::task::{Context, Poll, Waker};

/// Local waiter identity within one cell.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::client::pool) struct WaiterId(pub(in crate::client::pool) u64);

/// Cell-local acquisition waiters, their FIFO order, and aggregate demand.
///
/// At every completed transition:
///
/// - `records` owns every retained waiter.
/// - [`WaitingQueueState::Active`] contains exactly the [`WaiterState::Waiting`]
///   records.
/// - The active queue's demand describes its head waiter.
/// - Receiving, cancelled-receiving, and ready waiters remain in `records` but
///   not in the FIFO.
///
/// The queue never invokes admission or wakes a task. Transition results carry
/// that work across the cell lock boundary.
#[derive(Debug, Default)]
pub(super) struct WaiterQueue {
    /// Every waiter still queued, receiving, or holding delivered capacity.
    records: HashMap<WaiterId, WaiterRecord>,
    /// FIFO endpoints and demand for the records currently waiting.
    waiting: WaitingQueueState,
    /// Next cell-local waiter identity.
    next_waiter: u64,
    /// Next identity for a head-waiter demand episode.
    next_demand_id: u64,
}

impl WaiterQueue {
    /// Appends a waiter and returns demand when an empty queue becomes active.
    pub(super) fn register_waiter(
        &mut self,
        requirement: ProtocolRequirement,
        eligibility_group: &EligibilityGroup,
    ) -> (WaiterId, Option<DemandSnapshot>) {
        let waiter = self.take_waiter_id();
        let previous = match &self.waiting {
            WaitingQueueState::Empty => None,
            WaitingQueueState::Active { tail, .. } => Some(*tail),
        };
        let initial_demand = previous.is_none().then(|| self.new_demand(requirement));

        if let Some(previous) = previous {
            let previous = self
                .records
                .get_mut(&previous)
                .expect("waiting tail disappeared");
            let WaiterState::Waiting { next, .. } = &mut previous.state else {
                unreachable!("waiting tail left the waiting state");
            };
            debug_assert!(next.is_none());
            *next = Some(waiter);
        }

        let replaced = self.records.insert(
            waiter,
            WaiterRecord {
                requirement,
                state: WaiterState::Waiting {
                    previous,
                    next: None,
                    waker: None,
                },
            },
        );
        debug_assert!(replaced.is_none());

        let snapshot = match (&mut self.waiting, initial_demand) {
            (waiting @ WaitingQueueState::Empty, Some(demand)) => {
                let snapshot = demand.snapshot(eligibility_group);
                *waiting = WaitingQueueState::Active {
                    head: waiter,
                    tail: waiter,
                    len: NonZeroUsize::MIN,
                    demand,
                };
                Some(snapshot)
            }
            (WaitingQueueState::Active { tail, len, .. }, None) => {
                *tail = waiter;
                *len = len.checked_add(1).expect("waiter queue length exhausted");
                None
            }
            _ => unreachable!("waiter queue occupancy changed during registration"),
        };
        self.assert_consistent();
        (waiter, snapshot)
    }

    /// Cancels one waiter and detaches all cross-lock cleanup work.
    ///
    /// Removing the head retires its demand and starts a successor episode.
    /// Removing another waiting record leaves the active demand unchanged.
    /// Cancellation during delivery leaves a marker for lease installation to
    /// observe. A ready lease is returned to the caller for drop after unlock.
    pub(super) fn cancel_waiter(
        &mut self,
        waiter: WaiterId,
        eligibility_group: &EligibilityGroup,
    ) -> Option<WaiterCancellation> {
        let state = &self.records.get(&waiter)?.state;
        let cancellation = if matches!(state, WaiterState::Waiting { .. }) {
            let is_head = matches!(
                self.waiting,
                WaitingQueueState::Active { head, .. } if head == waiter
            );
            let demand_updates = if is_head {
                let removed = self.pop_head(eligibility_group);
                debug_assert_eq!(waiter, removed.waiter);
                let record = self
                    .records
                    .remove(&waiter)
                    .expect("cancelled head waiter disappeared");
                debug_assert!(matches!(record.state, WaiterState::Waiting { .. }));
                let retired =
                    DemandSnapshot::inactive(removed.demand.id, removed.demand.version.next());
                [Some(retired), removed.successor]
            } else {
                self.remove_non_head(waiter);
                [None, None]
            };
            WaiterCancellation {
                demand_updates,
                returned_lease: None,
            }
        } else if matches!(state, WaiterState::Receiving { .. }) {
            let record = self
                .records
                .get_mut(&waiter)
                .expect("receiving waiter disappeared");
            let WaiterState::Receiving { waker } = &mut record.state else {
                unreachable!("receiving waiter changed state under the cell lock");
            };
            let waker = waker.take();
            record.state = WaiterState::CancelledReceiving { waker };
            WaiterCancellation {
                demand_updates: [None, None],
                returned_lease: None,
            }
        } else if matches!(state, WaiterState::Ready(_)) {
            // Validate before detaching the lease. Once the lease is local,
            // this method must return without another panic-capable check.
            self.assert_consistent();
            let record = self.records.remove(&waiter)?;
            let WaiterState::Ready(lease) = record.state else {
                unreachable!("ready waiter changed state under the cell lock");
            };
            return Some(WaiterCancellation {
                demand_updates: [None, None],
                returned_lease: Some(lease),
            });
        } else {
            debug_assert!(matches!(state, WaiterState::CancelledReceiving { .. }));
            return None;
        };

        self.assert_consistent();
        Some(cancellation)
    }

    /// Returns a ready lease or records the latest waker for a pending waiter.
    ///
    /// # Panics
    ///
    /// Panics if `waiter` is unknown, was cancelled, or was already consumed
    /// by an earlier ready poll.
    pub(super) fn poll_waiter(
        &mut self,
        waiter: WaiterId,
        cx: &mut Context<'_>,
    ) -> Poll<CapacityLease> {
        if matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::Ready(_))
        ) {
            return Poll::Ready(
                self.take_ready_lease(waiter)
                    .expect("ready waiter lost its capacity lease"),
            );
        }

        let record = self
            .records
            .get_mut(&waiter)
            .expect("polled a cancelled, consumed, or unknown capacity waiter");
        let waker = match &mut record.state {
            WaiterState::Waiting { waker, .. } | WaiterState::Receiving { waker } => waker,
            WaiterState::CancelledReceiving { .. } => panic!("polled a cancelled capacity waiter"),
            WaiterState::Ready(_) => {
                unreachable!("ready waiter changed state under the cell lock")
            }
        };
        if waker
            .as_ref()
            .is_none_or(|waker| !waker.will_wake(cx.waker()))
        {
            *waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }

    /// Reserves the current head when a delivery still names its demand episode.
    ///
    /// The reserved waiter leaves the FIFO and changes in place to
    /// [`WaiterState::Receiving`]. Any remaining head receives a new demand
    /// episode returned for admission acknowledgement.
    pub(super) fn reserve_delivery_target(
        &mut self,
        demand: DemandId,
        eligibility_group: &EligibilityGroup,
    ) -> DeliveryReservation {
        let current = matches!(
            &self.waiting,
            WaitingQueueState::Active {
                demand: current,
                ..
            } if current.id == demand
        );
        if !current {
            return DeliveryReservation::Rejected;
        }

        let removed = self.pop_head(eligibility_group);
        debug_assert_eq!(removed.demand.id, demand);
        let record = self
            .records
            .get_mut(&removed.waiter)
            .expect("reserved queue head disappeared");
        let WaiterState::Waiting { waker, .. } = &mut record.state else {
            unreachable!("reserved queue head left the waiting state");
        };
        let waker = waker.take();
        record.state = WaiterState::Receiving { waker };

        self.assert_consistent();
        DeliveryReservation::Reserved {
            waiter: removed.waiter,
            successor: removed.successor,
        }
    }

    /// Completes the unlocked delivery crossing without panicking while the
    /// incoming lease remains a droppable local.
    ///
    /// A live receiver takes ownership of the lease in [`WaiterState::Ready`]
    /// before its state is checked. If cancellation won, or the reserved
    /// record is invalid, the lease is returned without another panic-capable
    /// operation. The caller refunnels it and reports invalid state only after
    /// unlocking.
    pub(super) fn install_delivered_lease(
        &mut self,
        waiter: WaiterId,
        lease: CapacityLease,
    ) -> LeaseInstallResult {
        let Some(record) = self.records.get_mut(&waiter) else {
            return LeaseInstallResult::invalid(lease, LeaseInstallError::MissingWaiter);
        };

        match &mut record.state {
            WaiterState::Receiving { waker } => {
                let waker = waker.take();
                record.state = WaiterState::Ready(lease);
                // The lease is state-owned before this check can panic.
                self.assert_consistent();
                LeaseInstallResult {
                    returned_lease: None,
                    waker,
                    error: None,
                }
            }
            WaiterState::CancelledReceiving { waker } => {
                let waker = waker.take();
                // The state was revalidated above while the same cell lock was
                // held, so removal cannot fail. Avoid an assertion here:
                // `lease` must cross the lock boundary even if state is broken.
                self.records.remove(&waiter);
                LeaseInstallResult {
                    returned_lease: Some(lease),
                    waker,
                    error: None,
                }
            }
            WaiterState::Waiting { .. } | WaiterState::Ready(_) => {
                LeaseInstallResult::invalid(lease, LeaseInstallError::UnexpectedState)
            }
        }
    }

    /// Removes a ready waiter and transfers ownership of its lease.
    pub(super) fn take_ready_lease(&mut self, waiter: WaiterId) -> Option<CapacityLease> {
        if !matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::Ready(_))
        ) {
            return None;
        }

        // Validate before detaching the lease. Returning it must be the last
        // operation performed while the cell lock is held.
        self.assert_consistent();
        let record = self.records.remove(&waiter)?;
        let WaiterState::Ready(lease) = record.state else {
            unreachable!("ready waiter changed state under the cell lock");
        };
        Some(lease)
    }

    /// Unlinks the FIFO head and installs any successor demand.
    ///
    /// The head record remains in `records`; the caller either changes it to
    /// `Receiving` or removes it for cancellation.
    fn pop_head(&mut self, eligibility_group: &EligibilityGroup) -> RemovedHead {
        let waiting = std::mem::take(&mut self.waiting);
        let WaitingQueueState::Active {
            head,
            tail,
            len,
            demand,
        } = waiting
        else {
            unreachable!("removed a head from an empty waiter queue");
        };
        let next = match &self
            .records
            .get(&head)
            .expect("waiting head disappeared")
            .state
        {
            WaiterState::Waiting { previous, next, .. } => {
                debug_assert!(previous.is_none());
                *next
            }
            _ => unreachable!("waiting head left the waiting state"),
        };

        let successor = match next {
            Some(next) => {
                let requirement = {
                    let next_record = self
                        .records
                        .get_mut(&next)
                        .expect("next waiter disappeared");
                    let WaiterState::Waiting { previous, .. } = &mut next_record.state else {
                        unreachable!("next waiter left the waiting state");
                    };
                    debug_assert_eq!(*previous, Some(head));
                    *previous = None;
                    next_record.requirement
                };

                let next_demand = self.new_demand(requirement);
                let snapshot = next_demand.snapshot(eligibility_group);
                let len = NonZeroUsize::new(
                    len.get()
                        .checked_sub(1)
                        .expect("waiter queue length underflowed"),
                )
                .expect("nonempty waiter queue lost its length");
                self.waiting = WaitingQueueState::Active {
                    head: next,
                    tail,
                    len,
                    demand: next_demand,
                };
                Some(snapshot)
            }
            None => {
                debug_assert_eq!(head, tail);
                debug_assert_eq!(len, NonZeroUsize::MIN);
                None
            }
        };

        RemovedHead {
            waiter: head,
            demand,
            successor,
        }
    }

    /// Removes a non-head waiting record without changing aggregate demand.
    fn remove_non_head(&mut self, waiter: WaiterId) -> WaiterRecord {
        let record = self
            .records
            .remove(&waiter)
            .expect("removed waiter disappeared");
        let (previous, next) = match &record.state {
            WaiterState::Waiting { previous, next, .. } => (*previous, *next),
            _ => unreachable!("removed waiter left the waiting state"),
        };
        let previous = previous.expect("non-head waiter had no predecessor");

        let previous_record = self
            .records
            .get_mut(&previous)
            .expect("previous waiter disappeared");
        let WaiterState::Waiting {
            next: previous_next,
            ..
        } = &mut previous_record.state
        else {
            unreachable!("previous waiter left the waiting state");
        };
        debug_assert_eq!(*previous_next, Some(waiter));
        *previous_next = next;

        if let Some(next) = next {
            let next_record = self
                .records
                .get_mut(&next)
                .expect("next waiter disappeared");
            let WaiterState::Waiting {
                previous: next_previous,
                ..
            } = &mut next_record.state
            else {
                unreachable!("next waiter left the waiting state");
            };
            debug_assert_eq!(*next_previous, Some(waiter));
            *next_previous = Some(previous);
        }

        let WaitingQueueState::Active {
            head, tail, len, ..
        } = &mut self.waiting
        else {
            unreachable!("removed a waiter from an empty queue");
        };
        debug_assert_ne!(*head, waiter);
        if next.is_none() {
            debug_assert_eq!(*tail, waiter);
            *tail = previous;
        }
        *len = NonZeroUsize::new(
            len.get()
                .checked_sub(1)
                .expect("waiter queue length underflowed"),
        )
        .expect("removing a non-head waiter emptied the queue");
        record
    }

    /// Allocates an identity that is never reused within this cell.
    fn take_waiter_id(&mut self) -> WaiterId {
        let value = self.next_waiter;
        self.next_waiter = value.checked_add(1).expect("waiter identity exhausted");
        WaiterId(value)
    }

    /// Starts a demand episode for a waiter that became the FIFO head.
    fn new_demand(&mut self, requirement: ProtocolRequirement) -> DemandTicket {
        let value = self.next_demand_id;
        self.next_demand_id = value.checked_add(1).expect("demand identity exhausted");
        DemandTicket {
            id: DemandId::from_u64(value),
            version: SnapshotVersion::INITIAL,
            requirement,
        }
    }

    /// Checks map, FIFO, and demand relationships in debug and test builds.
    pub(super) fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        self.assert_consistent_debug();
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        let waiting_records = self
            .records
            .values()
            .filter(|record| matches!(record.state, WaiterState::Waiting { .. }))
            .count();
        match &self.waiting {
            WaitingQueueState::Empty => {
                assert_eq!(0, waiting_records, "empty queue retained waiting records");
            }
            WaitingQueueState::Active {
                head,
                tail,
                len,
                demand,
            } => {
                let head_record = self.records.get(head).expect("waiting head disappeared");
                assert_eq!(
                    demand.requirement, head_record.requirement,
                    "aggregate demand did not describe the waiting head"
                );

                let mut current = Some(*head);
                let mut previous = None;
                let mut traversed = 0;
                while let Some(waiter) = current {
                    assert!(
                        traversed < self.records.len(),
                        "waiter queue contains a cycle"
                    );
                    let record = self
                        .records
                        .get(&waiter)
                        .expect("linked waiter disappeared");
                    let WaiterState::Waiting {
                        previous: linked_previous,
                        next,
                        ..
                    } = &record.state
                    else {
                        panic!("linked waiter left the waiting state");
                    };
                    assert_eq!(
                        previous, *linked_previous,
                        "waiter queue contains inconsistent backward links"
                    );
                    traversed += 1;
                    previous = Some(waiter);
                    current = *next;
                }

                assert_eq!(Some(*tail), previous, "waiting tail was not reachable");
                assert_eq!(
                    len.get(),
                    traversed,
                    "waiter queue length did not match its links"
                );
                assert_eq!(
                    waiting_records, traversed,
                    "waiting record was not reachable from the queue head"
                );
            }
        }
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> CellSnapshot {
        let (waiting, demand) = match &self.waiting {
            WaitingQueueState::Empty => (0, None),
            WaitingQueueState::Active { len, demand, .. } => (len.get(), Some(demand.id)),
        };
        CellSnapshot {
            waiting,
            retained: self.records.len(),
            demand,
        }
    }
}

/// Whether the cell has an active FIFO and aggregate head demand.
#[derive(Debug, Default)]
enum WaitingQueueState {
    /// No waiter is linked and no demand is active.
    #[default]
    Empty,
    /// A nonempty FIFO represented by its endpoints, length, and head demand.
    Active {
        /// Oldest waiting record.
        head: WaiterId,
        /// Youngest waiting record.
        tail: WaiterId,
        /// Number of records linked from `head` through `tail`.
        len: NonZeroUsize,
        /// Demand episode representing `head`.
        demand: DemandTicket,
    },
}

/// One request retained while waiting for or owning delivered capacity.
#[derive(Debug)]
struct WaiterRecord {
    /// Protocol requirement published while this waiter is the head.
    requirement: ProtocolRequirement,
    /// Queue residence, delivery state, and any owned lease.
    state: WaiterState,
}

/// Authoritative cell-local ownership state for one waiter.
#[derive(Debug)]
enum WaiterState {
    /// The waiter is linked in the cell-local FIFO.
    Waiting {
        /// Older waiting record, or `None` at the head.
        previous: Option<WaiterId>,
        /// Newer waiting record, or `None` at the tail.
        next: Option<WaiterId>,
        /// Latest task waiting for capacity.
        waker: Option<Waker>,
    },
    /// A delivery selected the waiter but has not installed its lease.
    Receiving {
        /// Latest task waiting for the crossing delivery.
        waker: Option<Waker>,
    },
    /// Cancellation won while a delivery was crossing without locks held.
    CancelledReceiving {
        /// Task detached for wake after the delivery is refunnelled.
        waker: Option<Waker>,
    },
    /// The waiter owns a delivered capacity lease.
    Ready(CapacityLease),
}

/// Cell-local ownership of the aggregate head demand.
#[derive(Debug)]
struct DemandTicket {
    /// Identity of this head-waiter episode.
    id: DemandId,
    /// Version of the next complete publication.
    version: SnapshotVersion,
    /// Protocol capability required by this head waiter.
    requirement: ProtocolRequirement,
}

impl DemandTicket {
    /// Creates the complete active state published for this head episode.
    fn snapshot(&self, eligibility_group: &EligibilityGroup) -> DemandSnapshot {
        DemandSnapshot::active(
            self.id,
            self.version,
            self.requirement,
            eligibility_group.clone(),
        )
    }
}

/// Result of reserving a target for one unlocked capacity delivery.
pub(super) enum DeliveryReservation {
    /// The current head was reserved and may have a successor demand.
    Reserved {
        waiter: WaiterId,
        successor: Option<DemandSnapshot>,
    },
    /// The delivery no longer matches live cell demand.
    Rejected,
}

/// Values detached from cell state by cancellation.
pub(super) struct WaiterCancellation {
    /// Demand retirement and optional successor published after unlocking.
    pub(super) demand_updates: [Option<DemandSnapshot>; 2],
    /// Delivered capacity returned after unlocking.
    pub(super) returned_lease: Option<CapacityLease>,
}

/// Invalid state observed while a committed lease was crossing to the cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeaseInstallError {
    /// The waiter reserved for the delivery no longer exists.
    MissingWaiter,
    /// The waiter exists but cannot receive the reserved delivery.
    UnexpectedState,
}

/// Values produced after installing or refunnelling a committed delivery.
pub(super) struct LeaseInstallResult {
    /// Capacity rejected by cancellation and returned after unlocking.
    pub(super) returned_lease: Option<CapacityLease>,
    /// Waiting task woken after the delivery fence closes.
    pub(super) waker: Option<Waker>,
    /// Invalid state reported only after the returned lease is refunnelled.
    pub(super) error: Option<LeaseInstallError>,
}

impl LeaseInstallResult {
    /// Preserves the incoming lease for unlocked cleanup after invalid state.
    fn invalid(lease: CapacityLease, error: LeaseInstallError) -> Self {
        Self {
            returned_lease: Some(lease),
            waker: None,
            error: Some(error),
        }
    }
}

/// FIFO state detached while advancing the active head.
struct RemovedHead {
    /// Identity of the unlinked waiter.
    waiter: WaiterId,
    /// Retired demand episode that represented this head.
    demand: DemandTicket,
    /// New demand published if another waiter became the head.
    successor: Option<DemandSnapshot>,
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct CellSnapshot {
    pub(super) waiting: usize,
    pub(super) retained: usize,
    pub(super) demand: Option<DemandId>,
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;

    #[test]
    fn head_cancellation_uses_the_successors_protocol_requirement() {
        let mut queue = WaiterQueue::default();
        let (head, initial) =
            queue.register_waiter(ProtocolRequirement::H2Required, &EligibilityGroup::Pool);
        let (_successor, no_new_demand) =
            queue.register_waiter(ProtocolRequirement::H1Compatible, &EligibilityGroup::Pool);
        assert!(initial.is_some());
        assert!(no_new_demand.is_none());

        let cancelled = queue
            .cancel_waiter(head, &EligibilityGroup::Pool)
            .expect("head waiter was not cancelled");
        assert_eq!(
            Some(DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H1Compatible,
                EligibilityGroup::Pool,
            )),
            cancelled.demand_updates[1]
        );
    }

    #[test]
    fn retired_demand_cannot_reserve_the_successor() {
        let mut queue = WaiterQueue::default();
        let (head, _initial) =
            queue.register_waiter(ProtocolRequirement::H1Compatible, &EligibilityGroup::Pool);
        queue.register_waiter(ProtocolRequirement::H1Compatible, &EligibilityGroup::Pool);
        queue
            .cancel_waiter(head, &EligibilityGroup::Pool)
            .expect("head waiter was not cancelled");

        assert!(matches!(
            queue.reserve_delivery_target(DemandId::from_u64(0), &EligibilityGroup::Pool),
            DeliveryReservation::Rejected
        ));
    }
}

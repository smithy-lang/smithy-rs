/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cell-local acquisition order and delivered-result ownership.
//!
//! [`WaiterQueue`] is the complete waiter state behind an origin cell's lock.
//! It keeps bounded-capacity order, attempts racing a returned H1, delivery
//! crossings, and terminal results in one invariant domain. Admission updates,
//! resource fallbacks, and task wakes are detached for the caller to process
//! after releasing the cell lock.

use super::super::admission::{DemandId, DemandSnapshot, ProtocolRequirement, SnapshotVersion};
use super::super::partition::EligibilityGroup;
use super::{AcquisitionEvent, AcquisitionResult, EstablishmentPermit};
use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::task::{Context, Poll, Waker};

/// Local waiter identity within one cell.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::client::pool) struct WaiterId(pub(in crate::client::pool) u64);

/// Cell-local acquisition episodes, bounded-capacity order, and H1 races.
///
/// At every completed transition:
///
/// - `records` owns every retained waiter.
/// - [`WaitingQueueState::Active`] contains exactly the [`WaiterState::Waiting`]
///   records.
/// - The active queue's demand describes its head waiter.
/// - `h1_candidates` contains each compatible receiving, ready-to-establish,
///   or launching episode.
/// - Receiving, ready-to-establish, launching, cancelled-receiving, and ready
///   episodes remain in `records` but not in the bounded-capacity FIFO.
///
/// The queue never invokes admission or wakes a task. Transition results carry
/// that work across the cell lock boundary.
#[derive(Debug, Default)]
pub(super) struct WaiterQueue {
    /// Every episode still waiting, launching, or holding a terminal result.
    records: HashMap<WaiterId, WaiterRecord>,
    /// FIFO endpoints and demand for the records currently waiting.
    waiting: WaitingQueueState,
    /// H1-compatible attempts still competing with a returned sender.
    h1_candidates: BTreeSet<WaiterId>,
    /// Next cell-local waiter identity.
    next_waiter: u64,
    /// Next identity for a head-waiter demand episode.
    next_demand_id: u64,
}

impl WaiterQueue {
    /// Registers one acquisition episode.
    ///
    /// An unbounded episode starts ready to establish. A bounded episode joins
    /// the capacity FIFO and returns a snapshot only when it becomes the head.
    pub(super) fn register_waiter(
        &mut self,
        requirement: ProtocolRequirement,
        eligibility_group: &EligibilityGroup,
        bounded: bool,
    ) -> (WaiterId, Option<DemandSnapshot>) {
        let waiter = self.take_waiter_id();
        if !bounded {
            self.records.insert(
                waiter,
                WaiterRecord {
                    requirement,
                    state: WaiterState::ReadyToEstablish {
                        permit: EstablishmentPermit::unbounded(),
                    },
                },
            );
            if requirement == ProtocolRequirement::H1Compatible {
                self.h1_candidates.insert(waiter);
            }
            self.assert_consistent();
            return (waiter, None);
        }

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

    /// Returns the oldest acquisition episode that can accept HTTP/1.
    ///
    /// A bounded waiter remains a candidate after receiving capacity so a
    /// returned sender can still beat its lazy or pool-owned establishment
    /// attempt. Comparing waiter identities preserves arrival order between
    /// that set and the current capacity-waiting head.
    fn oldest_h1_candidate(&self) -> Option<WaiterId> {
        let waiting = match &self.waiting {
            WaitingQueueState::Active { head, .. }
                if self.records[head].requirement == ProtocolRequirement::H1Compatible =>
            {
                Some(*head)
            }
            WaitingQueueState::Empty | WaitingQueueState::Active { .. } => None,
        };
        match (waiting, self.h1_candidates.first().copied()) {
            (Some(waiting), Some(launching)) => Some(waiting.min(launching)),
            (Some(waiting), None) => Some(waiting),
            (None, launching) => launching,
        }
    }

    /// Returns whether a returned H1 can satisfy a live acquisition episode.
    pub(super) fn can_accept_h1(&self) -> bool {
        self.oldest_h1_candidate().is_some()
    }

    /// Returns whether a previously admitted local attempt still accepts H1.
    ///
    /// These candidates left the capacity FIFO before its current head and
    /// therefore precede a claim aimed at that newer aggregate demand.
    pub(super) fn has_prior_h1_candidate(&self) -> bool {
        !self.h1_candidates.is_empty()
    }

    /// Commits a returned HTTP/1 sender to the oldest compatible episode.
    ///
    /// A waiter still awaiting capacity leaves the demand FIFO. A waiter that
    /// already owns establishment capacity returns that permit after unlock.
    /// If capacity is crossing from admission, the H1 result remains pending
    /// until that crossing returns its permit and closes its delivery fence.
    pub(super) fn install_returned_h1(
        &mut self,
        result: AcquisitionResult,
        eligibility_group: &EligibilityGroup,
    ) -> H1Install {
        let Some(waiter) = self.oldest_h1_candidate() else {
            return H1Install::rejected(result);
        };

        if matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::Waiting { .. })
        ) {
            let removed = self.pop_head(eligibility_group);
            debug_assert_eq!(removed.waiter, waiter);
            let record = self
                .records
                .get_mut(&waiter)
                .expect("HTTP/1 target waiter disappeared");
            let WaiterState::Waiting { waker, .. } = &mut record.state else {
                unreachable!("HTTP/1 target waiter left the waiting state");
            };
            let waker = waker.take();
            record.state = WaiterState::Ready(result);
            let retired =
                DemandSnapshot::inactive(removed.demand.id, removed.demand.version.next());
            self.assert_consistent();
            return H1Install {
                demand_updates: [Some(retired), removed.successor],
                returned_event: None,
                waker,
            };
        }

        let record = self
            .records
            .get_mut(&waiter)
            .expect("HTTP/1 candidate waiter disappeared");
        let (returned_event, waker) = match &mut record.state {
            WaiterState::Receiving {
                waker: _,
                pending_h1,
            } => {
                debug_assert!(pending_h1.is_none());
                *pending_h1 = Some(result);
                (None, None)
            }
            WaiterState::ReadyToEstablish { .. } | WaiterState::Launching { .. } => {
                let previous = std::mem::replace(&mut record.state, WaiterState::Ready(result));
                match previous {
                    WaiterState::ReadyToEstablish { permit } => {
                        (Some(AcquisitionEvent::Establish(permit)), None)
                    }
                    WaiterState::Launching { waker, .. } => (None, waker),
                    _ => unreachable!("HTTP/1 candidate changed state under the cell lock"),
                }
            }
            WaiterState::Waiting { .. }
            | WaiterState::CancelledReceiving { .. }
            | WaiterState::Ready(_) => {
                unreachable!("HTTP/1 candidate was not eligible for a returned sender")
            }
        };
        self.h1_candidates.remove(&waiter);
        self.assert_consistent();
        H1Install {
            demand_updates: [None, None],
            returned_event,
            waker,
        }
    }

    /// Cancels one waiter and detaches all cross-lock cleanup work.
    ///
    /// Removing the head retires its demand and starts a successor episode.
    /// Removing another waiting record leaves the active demand unchanged.
    /// Cancellation during delivery leaves a marker for result installation to
    /// observe. A ready result is returned to the caller for cleanup after unlock.
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
                returned_events: [None, None],
            }
        } else if matches!(state, WaiterState::Receiving { .. }) {
            self.h1_candidates.remove(&waiter);
            let record = self.records.get_mut(&waiter)?;
            let WaiterState::Receiving { waker, pending_h1 } = &mut record.state else {
                unreachable!("receiving waiter changed state under the cell lock");
            };
            let waker = waker.take();
            let pending_h1 = pending_h1.take();
            record.state = WaiterState::CancelledReceiving { waker, pending_h1 };
            WaiterCancellation {
                demand_updates: [None, None],
                returned_events: [None, None],
            }
        } else if matches!(
            state,
            WaiterState::ReadyToEstablish { .. } | WaiterState::Ready(_)
        ) {
            // Validate before detaching the event. Once it is local,
            // this method must return without another panic-capable check.
            self.assert_consistent();
            let record = self.records.remove(&waiter)?;
            self.h1_candidates.remove(&waiter);
            let event = match record.state {
                WaiterState::ReadyToEstablish { permit } => AcquisitionEvent::Establish(permit),
                WaiterState::Ready(result) => AcquisitionEvent::Complete(result),
                _ => unreachable!("ready waiter changed state under the cell lock"),
            };
            return Some(WaiterCancellation {
                demand_updates: [None, None],
                returned_events: [Some(event), None],
            });
        } else if matches!(state, WaiterState::Launching { .. }) {
            self.assert_consistent();
            self.h1_candidates.remove(&waiter);
            self.records.remove(&waiter)?;
            return Some(WaiterCancellation {
                demand_updates: [None, None],
                returned_events: [None, None],
            });
        } else {
            debug_assert!(matches!(state, WaiterState::CancelledReceiving { .. }));
            return None;
        };

        self.assert_consistent();
        Some(cancellation)
    }

    /// Returns the next event or records the latest waker for a pending waiter.
    ///
    /// # Panics
    ///
    /// Panics if `waiter` is unknown, was cancelled, or was already consumed
    /// by an earlier ready poll.
    pub(super) fn poll_waiter(
        &mut self,
        waiter: WaiterId,
        cx: &mut Context<'_>,
    ) -> Poll<AcquisitionEvent> {
        if matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::ReadyToEstablish { .. })
        ) {
            self.assert_consistent();
            let record = self
                .records
                .get_mut(&waiter)
                .expect("ready establishment waiter disappeared");
            let previous = std::mem::replace(
                &mut record.state,
                WaiterState::Launching {
                    started: false,
                    waker: None,
                },
            );
            let WaiterState::ReadyToEstablish { permit } = previous else {
                unreachable!("ready establishment waiter changed state under the cell lock");
            };
            return Poll::Ready(AcquisitionEvent::Establish(permit));
        }

        if matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::Ready(_))
        ) {
            return Poll::Ready(
                self.take_ready_result(waiter)
                    .map(AcquisitionEvent::Complete)
                    .expect("ready waiter lost its acquisition result"),
            );
        }

        let record = self
            .records
            .get_mut(&waiter)
            .expect("polled a cancelled, consumed, or unknown acquisition waiter");
        let waker = match &mut record.state {
            WaiterState::Waiting { waker, .. }
            | WaiterState::Receiving { waker, .. }
            | WaiterState::Launching { waker, .. } => waker,
            WaiterState::CancelledReceiving { .. } => {
                panic!("polled a cancelled acquisition waiter")
            }
            WaiterState::ReadyToEstablish { .. } | WaiterState::Ready(_) => {
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

    /// Claims pool ownership immediately before an attempt's first poll.
    ///
    /// A returned H1 or cancellation that wins first removes the lazy
    /// attempt's authority, allowing its unpolled future and permit to drop.
    pub(super) fn start_establishment(&mut self, waiter: WaiterId) -> bool {
        let Some(record) = self.records.get_mut(&waiter) else {
            return false;
        };
        match &mut record.state {
            WaiterState::Launching { started, .. } if !*started => {
                *started = true;
                self.assert_consistent();
                true
            }
            WaiterState::Ready(AcquisitionResult::H1(_)) => false,
            WaiterState::Launching { started: true, .. } => {
                panic!("establishment attempt was started more than once")
            }
            _ => {
                panic!("establishment start did not name a launching waiter")
            }
        }
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
        let accepts_h1 = record.requirement == ProtocolRequirement::H1Compatible;
        record.state = WaiterState::Receiving {
            waker,
            pending_h1: None,
        };
        if accepts_h1 {
            self.h1_candidates.insert(removed.waiter);
        }

        self.assert_consistent();
        DeliveryReservation::Reserved {
            waiter: removed.waiter,
            successor: removed.successor,
        }
    }

    /// Completes the unlocked capacity crossing without panicking while the
    /// incoming permit remains a droppable local.
    ///
    /// A live receiver takes ownership of the permit before another
    /// panic-capable operation. If an H1 return already won, both resources
    /// leave the lock as separate events and the permit is refunnelled.
    pub(super) fn install_capacity(
        &mut self,
        waiter: WaiterId,
        permit: EstablishmentPermit,
    ) -> ResultInstall {
        let Some(record) = self.records.get_mut(&waiter) else {
            return ResultInstall::invalid(
                AcquisitionEvent::Establish(permit),
                ResultInstallError::MissingWaiter,
            );
        };

        match &mut record.state {
            WaiterState::Receiving { waker, pending_h1 } => {
                let waker = waker.take();
                if let Some(h1) = pending_h1.take() {
                    record.state = WaiterState::Ready(h1);
                    self.h1_candidates.remove(&waiter);
                    self.assert_consistent();
                    return ResultInstall {
                        returned_events: [Some(AcquisitionEvent::Establish(permit)), None],
                        waker,
                        error: None,
                        accepted: false,
                    };
                }
                record.state = WaiterState::ReadyToEstablish { permit };
                // The permit is state-owned before this check can panic.
                self.assert_consistent();
                ResultInstall {
                    returned_events: [None, None],
                    waker,
                    error: None,
                    accepted: true,
                }
            }
            WaiterState::CancelledReceiving { waker, pending_h1 } => {
                let waker = waker.take();
                let pending_h1 = pending_h1.take();
                self.records.remove(&waiter);
                ResultInstall {
                    returned_events: [
                        pending_h1.map(AcquisitionEvent::Complete),
                        Some(AcquisitionEvent::Establish(permit)),
                    ],
                    waker,
                    error: None,
                    accepted: false,
                }
            }
            WaiterState::Waiting { .. }
            | WaiterState::ReadyToEstablish { .. }
            | WaiterState::Launching { .. }
            | WaiterState::Ready(_) => ResultInstall::invalid(
                AcquisitionEvent::Establish(permit),
                ResultInstallError::UnexpectedState,
            ),
        }
    }

    /// Installs a borrowed HTTP/1 result after its target was reserved.
    ///
    /// A local return may have won while the borrowed sender crossed its
    /// source and target locks. In that case the local result stays ready and
    /// the borrowed result leaves the lock for ordinary source return.
    pub(super) fn install_borrowed_h1(
        &mut self,
        waiter: WaiterId,
        result: AcquisitionResult,
    ) -> ResultInstall {
        let Some(record) = self.records.get_mut(&waiter) else {
            return ResultInstall::invalid(
                AcquisitionEvent::Complete(result),
                ResultInstallError::MissingWaiter,
            );
        };

        match &mut record.state {
            WaiterState::Receiving { waker, pending_h1 } => {
                let waker = waker.take();
                if let Some(local_h1) = pending_h1.take() {
                    record.state = WaiterState::Ready(local_h1);
                    self.h1_candidates.remove(&waiter);
                    self.assert_consistent();
                    return ResultInstall {
                        returned_events: [Some(AcquisitionEvent::Complete(result)), None],
                        waker,
                        error: None,
                        accepted: false,
                    };
                }
                record.state = WaiterState::Ready(result);
                self.h1_candidates.remove(&waiter);
                self.assert_consistent();
                ResultInstall {
                    returned_events: [None, None],
                    waker,
                    error: None,
                    accepted: true,
                }
            }
            WaiterState::CancelledReceiving { waker, pending_h1 } => {
                let waker = waker.take();
                let pending_h1 = pending_h1.take();
                self.records.remove(&waiter);
                ResultInstall {
                    returned_events: [
                        pending_h1.map(AcquisitionEvent::Complete),
                        Some(AcquisitionEvent::Complete(result)),
                    ],
                    waker,
                    error: None,
                    accepted: false,
                }
            }
            WaiterState::Waiting { .. }
            | WaiterState::ReadyToEstablish { .. }
            | WaiterState::Launching { .. }
            | WaiterState::Ready(_) => ResultInstall::invalid(
                AcquisitionEvent::Complete(result),
                ResultInstallError::UnexpectedState,
            ),
        }
    }

    /// Commits one establishment outcome if its launching waiter is still live.
    pub(super) fn install_establishment_result(
        &mut self,
        waiter: WaiterId,
        result: AcquisitionResult,
    ) -> ResultInstall {
        let Some(record) = self.records.get_mut(&waiter) else {
            return ResultInstall::rejected(AcquisitionEvent::Complete(result));
        };
        if matches!(record.state, WaiterState::Ready(AcquisitionResult::H1(_))) {
            return ResultInstall::rejected(AcquisitionEvent::Complete(result));
        }
        let WaiterState::Launching { started, waker } = &mut record.state else {
            return ResultInstall::invalid(
                AcquisitionEvent::Complete(result),
                ResultInstallError::UnexpectedState,
            );
        };
        debug_assert!(*started, "unstarted establishment attempt completed");
        let waker = waker.take();
        record.state = WaiterState::Ready(result);
        self.h1_candidates.remove(&waiter);
        self.assert_consistent();
        ResultInstall {
            returned_events: [None, None],
            waker,
            error: None,
            accepted: true,
        }
    }

    /// Removes a ready waiter and transfers ownership of its result.
    pub(super) fn take_ready_result(&mut self, waiter: WaiterId) -> Option<AcquisitionResult> {
        if !matches!(
            self.records.get(&waiter).map(|record| &record.state),
            Some(WaiterState::Ready(_))
        ) {
            return None;
        }

        // Validate before detaching the result. Returning it must be the last
        // operation performed while the cell lock is held.
        self.assert_consistent();
        let record = self.records.remove(&waiter)?;
        let WaiterState::Ready(result) = record.state else {
            unreachable!("ready waiter changed state under the cell lock");
        };
        Some(result)
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

        let expected_h1_candidates = self
            .records
            .iter()
            .filter_map(|(waiter, record)| {
                (record.requirement == ProtocolRequirement::H1Compatible
                    && matches!(
                        record.state,
                        WaiterState::Receiving {
                            pending_h1: None,
                            ..
                        } | WaiterState::ReadyToEstablish { .. }
                            | WaiterState::Launching { .. }
                    ))
                .then_some(*waiter)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            expected_h1_candidates, self.h1_candidates,
            "HTTP/1 acquisition candidates did not match waiter state"
        );
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

/// One request retained while waiting for or owning an acquisition result.
#[derive(Debug)]
struct WaiterRecord {
    /// Protocol requirement published while this waiter is the head.
    requirement: ProtocolRequirement,
    /// Queue residence, delivery state, and any owned result.
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
        /// Latest task waiting for an acquisition result.
        waker: Option<Waker>,
    },
    /// Capacity delivery selected the waiter but has not installed its permit.
    Receiving {
        /// Latest task waiting for the crossing delivery.
        waker: Option<Waker>,
        /// H1 return that won while capacity was crossing.
        pending_h1: Option<AcquisitionResult>,
    },
    /// Cancellation won while a delivery was crossing without locks held.
    CancelledReceiving {
        /// Task detached for wake after the delivery is refunnelled.
        waker: Option<Waker>,
        /// H1 result returned after the crossing closes, if one arrived first.
        pending_h1: Option<AcquisitionResult>,
    },
    /// Capacity is ready to start an establishment attempt.
    ReadyToEstablish {
        /// Optional bounded-origin capacity owned until connection install.
        permit: EstablishmentPermit,
    },
    /// Establishment may be lazy or pool-owned and races a returned H1.
    Launching {
        /// Whether the owner partition claimed the first connector poll.
        started: bool,
        /// Latest task waiting for the attempt or returned H1 to complete.
        waker: Option<Waker>,
    },
    /// The waiter owns its terminal acquisition result.
    Ready(AcquisitionResult),
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

/// Result of reserving a target for one unlocked one-to-one delivery.
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
    /// Intermediate and terminal events returned for cleanup after unlocking.
    pub(super) returned_events: [Option<AcquisitionEvent>; 2],
}

/// Values detached after attempting cell-local HTTP/1 service.
pub(super) struct H1Install {
    /// Demand retirement and optional successor published after unlocking.
    pub(super) demand_updates: [Option<DemandSnapshot>; 2],
    /// Establishment permit or rejected H1 returned after unlocking.
    pub(super) returned_event: Option<AcquisitionEvent>,
    /// Target task woken after demand publication.
    pub(super) waker: Option<Waker>,
}

impl H1Install {
    /// Preserves a result when no live episode can accept HTTP/1.
    fn rejected(result: AcquisitionResult) -> Self {
        Self {
            demand_updates: [None, None],
            returned_event: Some(AcquisitionEvent::Complete(result)),
            waker: None,
        }
    }
}

/// Invalid state observed while a committed event was crossing to the cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResultInstallError {
    /// The waiter reserved for the delivery no longer exists.
    MissingWaiter,
    /// The waiter exists but cannot receive the reserved delivery.
    UnexpectedState,
}

/// Values produced after installing or rejecting a committed event.
pub(super) struct ResultInstall {
    /// Events rejected by cancellation and returned after unlocking.
    ///
    /// When both are present, the H1 fallback precedes the capacity fallback
    /// so successor demand can reuse the connection before starting another.
    pub(super) returned_events: [Option<AcquisitionEvent>; 2],
    /// Waiting task woken after the delivery fence closes.
    pub(super) waker: Option<Waker>,
    /// Invalid state reported only after the returned result runs its fallback.
    pub(super) error: Option<ResultInstallError>,
    /// Whether target state became authoritative for the incoming event.
    pub(super) accepted: bool,
}

impl ResultInstall {
    /// Preserves the incoming result for unlocked cleanup after invalid state.
    fn invalid(event: AcquisitionEvent, error: ResultInstallError) -> Self {
        Self {
            returned_events: [Some(event), None],
            waker: None,
            error: Some(error),
            accepted: false,
        }
    }

    /// Preserves a losing result for its ordinary unlocked fallback.
    fn rejected(event: AcquisitionEvent) -> Self {
        Self {
            returned_events: [Some(event), None],
            waker: None,
            error: None,
            accepted: false,
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
        let (head, initial) = queue.register_waiter(
            ProtocolRequirement::H2Required,
            &EligibilityGroup::Pool,
            true,
        );
        let (_successor, no_new_demand) = queue.register_waiter(
            ProtocolRequirement::H1Compatible,
            &EligibilityGroup::Pool,
            true,
        );
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
        let (head, _initial) = queue.register_waiter(
            ProtocolRequirement::H1Compatible,
            &EligibilityGroup::Pool,
            true,
        );
        queue.register_waiter(
            ProtocolRequirement::H1Compatible,
            &EligibilityGroup::Pool,
            true,
        );
        queue
            .cancel_waiter(head, &EligibilityGroup::Pool)
            .expect("head waiter was not cancelled");

        assert!(matches!(
            queue.reserve_delivery_target(DemandId::from_u64(0), &EligibilityGroup::Pool),
            DeliveryReservation::Rejected
        ));
    }
}

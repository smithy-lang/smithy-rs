/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One caller processes an origin's pending demand and detached admission actions.
//!
//! [`OriginAdmission`] owns [`AdmissionProcessing`]: one stable
//! [`DemandPublication`] slot per partition, an action queue, and the atomic
//! baton state. Each cell caches its slot. Publishers record complete snapshots
//! there; one [`AdmissionBaton`] holder applies pending snapshots under the
//! origin admission lock and executes [`AdmissionAction`] steps outside locks.
//! Capacity, supply, and exact assignments remain authoritative in admission.
//!
//! # Local completion and processing
//!
//! Recording returns a baton only when this caller becomes the holder. That
//! caller publishes required supply/cleanup updates and signals its local
//! waiter before calling [`AdmissionBaton::process_until_quiescent`]. Otherwise,
//! an existing holder can process the publication concurrently with local
//! completion. Recording does not make the publisher wait for that drain.
//!
//! Detached actions, including guard cleanup and action successors, join the
//! same queue. The holder checks pending publications between action steps.
//! An empty recording can also acquire the baton: a caller may need to defer
//! actions produced by cleanup even when it has no demand snapshot to publish.
//!
//! # Publication and release
//!
//! A publisher records its slot before setting `HELD | PENDING` in one atomic
//! operation. `HELD` elects one holder; `PENDING` requests another scheduling
//! pass. Releasing requires changing exactly `HELD` to zero. A publication
//! racing with release therefore either keeps the current holder processing or
//! elects a new one. Empty recordings set only `HELD`.
//!
//! Slot extraction clears its dirty flag under the slot mutex. The holder
//! releases directory and slot locks before taking admission, and releases
//! admission and action-queue locks before executing an action. These locks are
//! never nested.
//!
//! # Progress and unwinding
//!
//! Processing is synchronous on the holder, with no background task or work
//! budget. Sustained publication can delay quiescence and occupy a current-thread
//! runtime for a long time. Other supply and settlement paths can still take the
//! admission lock.
//!
//! Dropping a baton processes already recorded demand and queued actions, also
//! during callback unwinding. Processing retains the first callback panic,
//! finishes recorded work, releases the baton, and resumes that panic. It does
//! not complete an interrupted caller's remaining cleanup or recover arbitrary
//! invariant failures or destructor double-panic aborts.

use super::{AdmissionAction, DemandSnapshot, OriginAdmission, PartitionId};
use crate::sync::{Arc, AtomicBool, AtomicUsize, Mutex, Ordering};
use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

/// Shared pending-demand slot for one partition of a bounded origin.
///
/// Concurrent cell construction shares this slot, and the retained cell caches
/// it. The slot holds snapshots only: no cell, connection, or capacity ownership.
#[derive(Debug)]
pub(in crate::client::pool) struct DemandPublication {
    dirty: AtomicBool,
    pending: Mutex<PendingDemand>,
}

impl Default for DemandPublication {
    fn default() -> Self {
        Self {
            dirty: AtomicBool::new(false),
            pending: Mutex::new(PendingDemand::default()),
        }
    }
}

/// At most the newest retirement and the newest complete replacement snapshot.
///
/// Retirement can remove scheduling positions or cancel a retained H1 match.
/// Keeping only the latest active snapshot would omit those effects. Extraction
/// applies retirement before current state, omitting a duplicate inactive value.
#[derive(Debug, Default)]
struct PendingDemand {
    retirement: Option<DemandSnapshot>,
    current: Option<DemandSnapshot>,
}

impl PendingDemand {
    fn record(&mut self, snapshot: DemandSnapshot) {
        if !snapshot.is_active()
            && self
                .retirement
                .as_ref()
                .is_none_or(|old| snapshot.is_newer_than(old))
        {
            self.retirement = Some(snapshot.clone());
        }
        if self
            .current
            .as_ref()
            .is_none_or(|old| snapshot.is_newer_than(old))
        {
            self.current = Some(snapshot);
        }
    }

    fn snapshots(self) -> impl Iterator<Item = DemandSnapshot> {
        let retirement = self
            .retirement
            .filter(|retirement| self.current.as_ref() != Some(retirement));
        [retirement, self.current].into_iter().flatten()
    }
}

impl DemandPublication {
    /// Folds snapshots into pending state and reports whether any were submitted.
    ///
    /// Duplicate or stale submissions still mark the slot dirty so they request
    /// another scheduling pass. Empty submissions leave pending state unchanged.
    fn record(&self, snapshots: impl IntoIterator<Item = DemandSnapshot>) -> bool {
        let mut pending = self.pending.lock();
        let mut submitted = false;
        for snapshot in snapshots {
            pending.record(snapshot);
            submitted = true;
        }
        if submitted {
            // Duplicate/stale submissions also request another scheduling pass.
            self.dirty.store(true, Ordering::SeqCst);
        }
        submitted
    }

    /// Extracts pending snapshots, clearing notification under the same lock.
    ///
    /// The first dirty load is a skip hint. The locked recheck and clear keep an
    /// extraction from erasing a concurrent publisher's notification.
    fn take(&self) -> Option<PendingDemand> {
        if !self.dirty.load(Ordering::SeqCst) {
            return None;
        }
        let mut pending = self.pending.lock();
        // Serialize clearing with record(): an older pass must not clear a
        // newer publication's notification after an ownership handoff.
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return None;
        }
        Some(std::mem::take(&mut *pending))
    }
}

/// Pending demand, detached actions, and baton ownership for one origin.
///
/// This state coordinates processing. The authoritative demand, supply, and
/// capacity ledger remains under `OriginAdmission::state`.
pub(super) struct AdmissionProcessing {
    /// `HELD | PENDING`; publication and ownership share one modification order.
    state: AtomicUsize,
    /// Stable per-partition slots, shared with cells without owning those cells.
    slots: Mutex<BTreeMap<PartitionId, Arc<DemandPublication>>>,
    /// Detached resource-owning actions, including successors and guard cleanup.
    actions: Mutex<VecDeque<AdmissionAction>>,
}

const HELD: usize = 1;
const PENDING: usize = 2;

impl Default for AdmissionProcessing {
    fn default() -> Self {
        Self {
            state: AtomicUsize::new(0),
            slots: Mutex::new(BTreeMap::new()),
            actions: Mutex::new(VecDeque::new()),
        }
    }
}

impl AdmissionProcessing {
    /// Returns the stable slot for a partition, creating it on first use.
    pub(super) fn slot(&self, partition: PartitionId) -> Arc<DemandPublication> {
        self.slots.lock().entry(partition).or_default().clone()
    }

    /// Elects this caller if unheld and optionally requests another scheduling pass.
    ///
    /// A losing caller leaves the request for the existing holder and returns
    /// immediately. Only the caller that observed `HELD` clear receives a guard.
    fn claim(admission: &Arc<OriginAdmission>, request_pass: bool) -> Option<AdmissionBaton> {
        let processing = &admission.processing;
        let flags = HELD | if request_pass { PENDING } else { 0 };
        let previous = processing.state.fetch_or(flags, Ordering::SeqCst);
        (previous & HELD == 0).then(|| AdmissionBaton {
            admission: admission.clone(),
            held: true,
            cursor: 0,
        })
    }

    /// Records snapshots before election, returning a guard for local completion.
    ///
    /// This method does not drain. Even an empty recording can acquire a guard
    /// so a new holder finishes its local transition before processing cleanup.
    pub(super) fn record(
        admission: &Arc<OriginAdmission>,
        slot: &DemandPublication,
        snapshots: impl IntoIterator<Item = DemandSnapshot>,
    ) -> Option<AdmissionBaton> {
        let submitted = slot.record(snapshots);
        // Even a transition without demand changes can drop a permit or publish
        // supply. Hold its baton scope until that transition finishes, without
        // requesting an otherwise unnecessary scheduling pass.
        Self::claim(admission, submitted)
    }

    /// Queues an action and processes it only if this caller acquires the baton.
    ///
    /// Under an existing holder, execution is deferred to that caller.
    pub(super) fn enqueue_action(admission: &Arc<OriginAdmission>, action: AdmissionAction) {
        admission.processing.actions.lock().push_back(action);
        if let Some(baton) = Self::claim(admission, true) {
            baton.process_until_quiescent();
        }
    }
}

/// A caller's exclusive guard for processing one origin's recorded work.
///
/// Acquisition precedes local completion; explicit processing follows it. The
/// guard owns an origin reference, not an executor or a spawned task. Drop also
/// drains recorded work, including during supported callback unwinding, so a
/// caller that exits before explicit processing does not strand that work.
///
/// The holder can occupy its runtime until publication becomes quiescent. The
/// unwind guarantee covers recorded work, not the remainder of an interrupted
/// caller cleanup closure; see the module's progress and unwinding notes.
pub(in crate::client::pool) struct AdmissionBaton {
    admission: Arc<OriginAdmission>,
    held: bool,
    cursor: usize,
}

impl AdmissionBaton {
    /// Processes recorded work until checked release, then resumes any callback panic.
    ///
    /// Call after publishing local supply/cleanup updates and signalling waiters.
    /// Signalling does not make this runtime poll them before processing returns.
    pub(in crate::client::pool) fn process_until_quiescent(mut self) {
        let panic = self.drain();
        if let Some(panic) = panic {
            resume_unwind(panic);
        }
    }

    /// Applies a batch of pending demand, then runs at most one detached action.
    ///
    /// Registration is refreshed and the starting partition rotated each round.
    /// The caller checks new publications before advancing the next action.
    fn round(&mut self, scheduling_requested: bool) {
        // Refresh registration each pass. A publisher may register a new cell
        // while an existing caller holds the baton.
        let mut slots: Vec<_> = self
            .admission
            .processing
            .slots
            .lock()
            .iter()
            .map(|(id, slot)| (*id, slot.clone()))
            .collect();
        if !slots.is_empty() {
            let offset = self.cursor % slots.len();
            slots.rotate_left(offset);
            self.cursor = offset + 1;
        }
        let publications: Vec<_> = slots
            .into_iter()
            .filter_map(|(id, slot)| slot.take().map(|pending| (id, pending)))
            .collect();

        if scheduling_requested || !publications.is_empty() {
            let action = {
                let mut state = self.admission.state.lock();
                for (partition, pending) in publications {
                    for snapshot in pending.snapshots() {
                        state.apply_demand_snapshot(partition, snapshot);
                    }
                }
                OriginAdmission::prepare_action(&self.admission, &mut state)
            };
            if let Some(action) = action {
                self.admission.processing.actions.lock().push_back(action);
            }
        }

        let action = self.admission.processing.actions.lock().pop_front();
        if let Some(action) = action {
            if let Some(next) = action.run_once() {
                self.admission.processing.actions.lock().push_back(next);
            }
        }
    }

    /// Processes until no scheduling pass or action remains and release succeeds.
    ///
    /// Returns the first callback panic only after releasing ownership. A
    /// concurrent `PENDING` signal prevents release and makes this holder retry.
    fn drain(&mut self) -> Option<Box<dyn Any + Send>> {
        let mut first_panic = None;
        loop {
            let requested = self
                .admission
                .processing
                .state
                .fetch_and(!PENDING, Ordering::SeqCst)
                & PENDING
                != 0;
            let actions = !self.admission.processing.actions.lock().is_empty();
            if requested || actions {
                // User wakers can panic during an action. Its ownership guards
                // enqueue cleanup before unwinding. Finish that work and pending
                // publications, then propagate the original panic.
                if let Err(panic) = catch_unwind(AssertUnwindSafe(|| self.round(requested))) {
                    if first_panic.is_none() {
                        first_panic = Some(panic);
                    } else {
                        discard_panic(panic);
                    }
                }
                continue;
            }

            // Disarm before release: an old guard must not clear a new holder.
            self.held = false;
            if self
                .admission
                .processing
                .state
                .compare_exchange(HELD, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return first_panic;
            }
            // A publisher set PENDING before release; this holder stays elected.
            self.held = true;
        }
    }
}

impl Drop for AdmissionBaton {
    fn drop(&mut self) {
        if self.held {
            let already_panicking = std::thread::panicking();
            let panic = self.drain();
            if !already_panicking {
                if let Some(panic) = panic {
                    resume_unwind(panic);
                }
            } else if let Some(panic) = panic {
                discard_panic(panic);
            }
        }
    }
}

/// A secondary payload may itself have a panicking destructor. Preserve the
/// original panic; forget only a payload whose own disposal panics recursively.
fn discard_panic(panic: Box<dyn Any + Send>) {
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(panic))) {
        std::mem::forget(panic);
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
pub(super) fn fold_for_test(
    snapshots: impl IntoIterator<Item = DemandSnapshot>,
) -> Vec<DemandSnapshot> {
    let mut pending = PendingDemand::default();
    for snapshot in snapshots {
        pending.record(snapshot);
    }
    pending.snapshots().collect()
}

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::client::pool::admission::{DemandId, ProtocolRequirement, SnapshotVersion};
    use crate::client::pool::partition::EligibilityGroup;
    use std::num::NonZeroUsize;

    pub(super) fn origin() -> Arc<OriginAdmission> {
        OriginAdmission::for_test(NonZeroUsize::new(1).unwrap())
    }

    pub(super) fn active(id: u64) -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(id),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Compatible,
            EligibilityGroup::Pool,
        )
    }

    pub(super) fn inactive(id: u64) -> DemandSnapshot {
        DemandSnapshot::inactive(DemandId::from_u64(id), SnapshotVersion::INITIAL.next())
    }

    pub(super) fn assert_quiescent(origin: &OriginAdmission) {
        assert_eq!(0, origin.processing.state.load(Ordering::SeqCst));
        assert!(origin.processing.actions.lock().is_empty());
        let slots: Vec<_> = origin.processing.slots.lock().values().cloned().collect();
        for slot in slots {
            assert!(!slot.dirty.load(Ordering::SeqCst));
            let pending = slot.pending.lock();
            assert!(pending.current.is_none());
            assert!(pending.retirement.is_none());
        }
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::client::pool::admission::demand::{
        DemandAssignmentId, DemandAssignmentOutcome, DemandSchedule,
    };
    use crate::client::pool::admission::{DemandId, ProtocolRequirement, SnapshotVersion};
    use crate::client::pool::cell::{AcquisitionStep, OriginCell};
    use crate::client::pool::partition::EligibilityGroup;
    use std::task::{Context, Wake, Waker};

    #[test]
    fn nonholder_publishes_without_waiting_for_admission() {
        let origin = origin();
        let id = PartitionId::from_index(1);
        let slot = origin.processing.slot(id);
        let baton = AdmissionProcessing::record(&origin, &slot, [inactive(0)]).unwrap();
        // Holding admission here proves a non-holder does not attempt its lock.
        let state = origin.state.lock();
        let (send, receive) = std::sync::mpsc::channel();
        let publisher = {
            let origin = origin.clone();
            let slot = slot.clone();
            std::thread::spawn(move || {
                send.send(AdmissionProcessing::record(&origin, &slot, [inactive(1)]).is_none())
                    .unwrap();
            })
        };
        let result = receive.recv_timeout(std::time::Duration::from_secs(2));
        drop(state);
        publisher.join().unwrap();
        assert_eq!(Ok(true), result);
        baton.process_until_quiescent();
        assert_eq!(
            Some(&inactive(1)),
            origin.state.lock().demand.latest_for_test(&id)
        );
        assert_quiescent(&origin);
    }

    #[test]
    fn dropping_last_baton_processes_work_without_another_producer() {
        let origin = origin();
        let id = PartitionId::from_index(1);
        let slot = origin.processing.slot(id);
        let baton = AdmissionProcessing::record(&origin, &slot, [inactive(0)]).unwrap();
        assert!(AdmissionProcessing::record(&origin, &slot, [inactive(2)]).is_none());
        drop(baton);
        assert_eq!(
            Some(&inactive(2)),
            origin.state.lock().demand.latest_for_test(&id)
        );
        assert_quiescent(&origin);
    }

    #[test]
    fn folding_preserves_fifo_and_future_assignment_settlement() {
        let cell = PartitionId::from_index(1);
        let peer = PartitionId::from_index(2);
        let histories = [
            vec![inactive(0), active(1), inactive(1), active(2)],
            vec![active(1), inactive(1)],
            vec![inactive(0), active(1), inactive(1), active(2), inactive(2)],
            // H2 route acknowledgement then reactivation of the same demand.
            vec![
                inactive(0),
                DemandSnapshot::active(
                    DemandId::from_u64(0),
                    SnapshotVersion::INITIAL.next().next(),
                    ProtocolRequirement::H1Compatible,
                    EligibilityGroup::Pool,
                ),
            ],
        ];
        for assigned in [false, true] {
            for history in &histories {
                let mut ordered = DemandSchedule::default();
                let mut folded = DemandSchedule::default();
                for schedule in [&mut ordered, &mut folded] {
                    schedule.apply_snapshot(cell, active(0));
                    schedule.apply_snapshot(peer, active(0));
                }
                let assignments = assigned.then(|| {
                    (
                        ordered
                            .prepare_origin_assignment(DemandAssignmentId(0))
                            .unwrap(),
                        folded
                            .prepare_origin_assignment(DemandAssignmentId(0))
                            .unwrap(),
                    )
                });
                let mut pending = PendingDemand::default();
                for snapshot in history {
                    ordered.apply_snapshot(cell, snapshot.clone());
                    pending.record(snapshot.clone());
                }
                for snapshot in pending.snapshots() {
                    folded.apply_snapshot(cell, snapshot);
                }
                if let Some((left, right)) = assignments {
                    ordered.settle_assignment(&left, DemandAssignmentOutcome::RetrySamePosition);
                    folded.settle_assignment(&right, DemandAssignmentOutcome::RetrySamePosition);
                }
                // Compare future scheduling, including demand behind the peer.
                for ordinal in 1..=3 {
                    let left = ordered.prepare_origin_assignment(DemandAssignmentId(ordinal));
                    let right = folded.prepare_origin_assignment(DemandAssignmentId(ordinal));
                    assert_eq!(left, right, "history: {history:?}");
                    if let Some(assignment) = left {
                        ordered.settle_assignment(
                            &assignment,
                            DemandAssignmentOutcome::Accepted { successor: None },
                        );
                    }
                    if let Some(assignment) = right {
                        folded.settle_assignment(
                            &assignment,
                            DemandAssignmentOutcome::Accepted { successor: None },
                        );
                    }
                }
                assert_eq!(
                    ordered.latest_for_test(&cell),
                    folded.latest_for_test(&cell)
                );
                assert_eq!(ordered.len(), folded.len());
            }
        }
    }

    struct PanicWake(&'static str);

    impl Wake for PanicWake {
        fn wake(self: std::sync::Arc<Self>) {
            panic!("{}", self.0);
        }
    }

    fn cell(origin: &Arc<OriginAdmission>, id: usize) -> Arc<OriginCell> {
        OriginAdmission::register_cell(
            origin,
            Arc::new(OriginCell::new(
                PartitionId::from_index(id),
                origin.origin().clone(),
                EligibilityGroup::Pool,
                Some(origin.clone()),
                None,
            )),
        )
    }

    #[test]
    fn unchanged_snapshot_still_drives_available_capacity() {
        let origin = origin();
        let cell = cell(&origin, 1);
        let held = origin.state.lock().take_permit().unwrap();
        let (waiter, snapshot) =
            cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        AdmissionProcessing::record(&origin, &cell.demand_publication, [snapshot.clone()])
            .unwrap()
            .process_until_quiescent();
        origin.state.lock().return_permit(held);
        AdmissionProcessing::record(&origin, &cell.demand_publication, [snapshot])
            .unwrap()
            .process_until_quiescent();
        assert!(cell
            .poll_waiter(waiter, &mut Context::from_waker(Waker::noop()))
            .is_ready());
        assert_eq!(1, origin.available_capacity_for_test());
        assert_quiescent(&origin);
    }

    #[test]
    fn repeated_action_panics_preserve_first_and_release_queued_resources() {
        let origin = OriginAdmission::for_test(std::num::NonZeroUsize::new(2).unwrap());
        let first = cell(&origin, 1);
        let second = cell(&origin, 2);
        let (a, demand_a) =
            first.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let (b, demand_b) =
            second.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        for (cell, waiter, label) in [(&first, a, "first"), (&second, b, "second")] {
            let waker = Waker::from(std::sync::Arc::new(PanicWake(label)));
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(&waker))
                .is_pending());
        }
        let baton =
            AdmissionProcessing::record(&origin, &first.demand_publication, [demand_a]).unwrap();
        assert!(
            AdmissionProcessing::record(&origin, &second.demand_publication, [demand_b]).is_none()
        );
        let panic = catch_unwind(AssertUnwindSafe(|| baton.process_until_quiescent())).unwrap_err();
        assert_eq!(Some(&"first".to_owned()), panic.downcast_ref::<String>());
        assert_quiescent(&origin);
        for (cell, waiter) in [(&first, a), (&second, b)] {
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(Waker::noop()))
                .is_ready());
        }
        assert_eq!(2, origin.available_capacity_for_test());
        let weak = std::sync::Arc::downgrade(&origin);
        drop(first);
        drop(second);
        drop(origin);
        assert!(
            weak.upgrade().is_none(),
            "queued actions retained the origin"
        );
    }

    #[test]
    fn reentrant_publication_is_applied_before_next_action_wake() {
        struct PublishWake {
            origin: Arc<OriginAdmission>,
            slot: Arc<DemandPublication>,
        }
        impl Wake for PublishWake {
            fn wake(self: std::sync::Arc<Self>) {
                assert!(
                    AdmissionProcessing::record(&self.origin, &self.slot, [inactive(9)]).is_none()
                );
            }
        }
        struct CheckWake {
            origin: Arc<OriginAdmission>,
            partition: PartitionId,
        }
        impl Wake for CheckWake {
            fn wake(self: std::sync::Arc<Self>) {
                assert_eq!(
                    Some(&inactive(9)),
                    self.origin
                        .state
                        .lock()
                        .demand
                        .latest_for_test(&self.partition),
                    "next action bypassed pending peer publication",
                );
            }
        }
        let origin = OriginAdmission::for_test(std::num::NonZeroUsize::new(2).unwrap());
        let first = cell(&origin, 1);
        let second = cell(&origin, 2);
        let quiet = PartitionId::from_index(3);
        let quiet_slot = origin.processing.slot(quiet);
        let (a, demand_a) =
            first.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let (b, demand_b) =
            second.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let wake_a = Waker::from(std::sync::Arc::new(PublishWake {
            origin: origin.clone(),
            slot: quiet_slot,
        }));
        let wake_b = Waker::from(std::sync::Arc::new(CheckWake {
            origin: origin.clone(),
            partition: quiet,
        }));
        assert!(first
            .poll_waiter(a, &mut Context::from_waker(&wake_a))
            .is_pending());
        assert!(second
            .poll_waiter(b, &mut Context::from_waker(&wake_b))
            .is_pending());
        let baton =
            AdmissionProcessing::record(&origin, &first.demand_publication, [demand_a]).unwrap();
        assert!(
            AdmissionProcessing::record(&origin, &second.demand_publication, [demand_b]).is_none()
        );
        baton.process_until_quiescent();
        for (cell, waiter) in [(&first, a), (&second, b)] {
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(Waker::noop()))
                .is_ready());
        }
        assert_quiescent(&origin);
    }

    #[test]
    fn callback_panics_drain_successors_and_preserve_first_panic() {
        let origin = origin();
        let first = cell(&origin, 1);
        let second = cell(&origin, 2);
        let (a, demand_a) =
            first.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let (b, demand_b) =
            second.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        for (cell, waiter, label) in [(&first, a, "first"), (&second, b, "second")] {
            let waker = Waker::from(std::sync::Arc::new(PanicWake(label)));
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(&waker))
                .is_pending());
        }
        let baton =
            AdmissionProcessing::record(&origin, &first.demand_publication, [demand_a]).unwrap();
        assert!(
            AdmissionProcessing::record(&origin, &second.demand_publication, [demand_b]).is_none()
        );
        let panic = catch_unwind(AssertUnwindSafe(|| baton.process_until_quiescent())).unwrap_err();
        assert_eq!(Some(&"first".to_owned()), panic.downcast_ref::<String>());
        let ready_a = first.poll_waiter(a, &mut Context::from_waker(Waker::noop()));
        let std::task::Poll::Ready(AcquisitionStep::StartEstablishment(permit)) = ready_a else {
            panic!("first delivery was lost");
        };
        // Returning its permit prepares the second delivery; its panic is caught
        // and propagated after its admission work has finished.
        let second_panic = catch_unwind(AssertUnwindSafe(|| drop(permit))).unwrap_err();
        assert_eq!(
            Some(&"second".to_owned()),
            second_panic.downcast_ref::<String>()
        );
        let ready_b = second.poll_waiter(b, &mut Context::from_waker(Waker::noop()));
        drop(ready_b);
        assert_eq!(1, origin.available_capacity_for_test());
        assert_quiescent(&origin);
    }

    #[test]
    fn panic_before_post_processing_does_not_strand_retirement() {
        let origin = origin();
        let id = PartitionId::from_index(1);
        let slot = origin.processing.slot(id);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _baton = AdmissionProcessing::record(&origin, &slot, [inactive(7)]).unwrap();
            panic!("local wake");
        }));
        assert!(result.is_err());
        assert_eq!(
            Some(&inactive(7)),
            origin.state.lock().demand.latest_for_test(&id)
        );
        assert_quiescent(&origin);
    }

    #[test]
    fn processing_does_not_require_an_owner_runtime_to_be_driven() {
        let paused = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        paused.block_on(async {});
        let active_runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let origin = origin();
        let id = PartitionId::from_index(1);
        active_runtime.block_on(async {
            let slot = origin.processing.slot(id);
            AdmissionProcessing::record(&origin, &slot, [inactive(3)])
                .unwrap()
                .process_until_quiescent();
        });
        assert_quiescent(&origin);
        drop(paused);
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::test_support::*;
    use super::*;
    use crate::client::pool::admission::demand::DemandAssignmentOutcome;

    #[test]
    fn baton_release_races_last_publication_and_cancellation() {
        loom::model(|| {
            let origin = origin();
            let id = PartitionId::from_index(1);
            let slot = origin.processing.slot(id);
            let held = origin.state.lock().take_permit().unwrap();
            let baton = AdmissionProcessing::record(&origin, &slot, [active(0)]).unwrap();
            let holder = loom::thread::spawn(move || baton.process_until_quiescent());
            let writer = {
                let origin = origin.clone();
                let slot = slot.clone();
                loom::thread::spawn(move || {
                    if let Some(baton) =
                        AdmissionProcessing::record(&origin, &slot, [active(1), inactive(1)])
                    {
                        baton.process_until_quiescent();
                    }
                })
            };
            holder.join().unwrap();
            writer.join().unwrap();
            assert_eq!(
                Some(&inactive(1)),
                origin.state.lock().demand.latest_for_test(&id)
            );
            assert_eq!(0, origin.ordered_demand_count_for_test());
            origin.state.lock().return_permit(held);
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn registration_and_duplicate_kick_race_processing() {
        loom::model(|| {
            let origin = origin();
            let first = PartitionId::from_index(1);
            let second = PartitionId::from_index(2);
            let slot = origin.processing.slot(first);
            let baton = AdmissionProcessing::record(&origin, &slot, [inactive(0)]).unwrap();
            let holder = loom::thread::spawn(move || baton.process_until_quiescent());
            let writer = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    let slot = origin.processing.slot(second);
                    if let Some(baton) =
                        AdmissionProcessing::record(&origin, &slot, [inactive(0), inactive(0)])
                    {
                        baton.process_until_quiescent();
                    }
                })
            };
            holder.join().unwrap();
            writer.join().unwrap();
            assert_eq!(
                Some(&inactive(0)),
                origin.state.lock().demand.latest_for_test(&second)
            );
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn deferred_successor_races_old_assignment_settlement() {
        loom::model(|| {
            let origin = origin();
            let id = PartitionId::from_index(1);
            let slot = origin.processing.slot(id);
            let prepared = {
                let mut state = origin.state.lock();
                state.apply_demand_snapshot(id, active(0));
                state.prepare_capacity_delivery().unwrap()
            };
            let baton =
                AdmissionProcessing::record(&origin, &slot, [inactive(0), active(1)]).unwrap();
            let holder = loom::thread::spawn(move || baton.process_until_quiescent());
            let assignment = prepared.assignment.clone();
            let settle = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    origin.state.lock().settle_assignment(
                        &assignment,
                        DemandAssignmentOutcome::Refused { successor: None },
                    );
                })
            };
            holder.join().unwrap();
            settle.join().unwrap();
            assert_eq!(
                Some(&active(1)),
                origin.state.lock().demand.latest_for_test(&id)
            );
            assert_eq!(1, origin.ordered_demand_count_for_test());
            origin.state.lock().return_permit(prepared.permit);
            assert_quiescent(&origin);
        });
    }
}

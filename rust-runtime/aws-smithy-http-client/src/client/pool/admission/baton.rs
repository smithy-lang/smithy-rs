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
//! [`BatonScope`] records demand and retains an initially elected guard through
//! local cleanup and wakes. Its `process` closes that local phase. An elected
//! holder processes; a ready nonholder may nominate itself against an eligible
//! holder. Nested local completion and callbacks inside any admission processor
//! cannot nominate, including across origins. An existing holder can apply
//! publications concurrently with the publisher's local completion.
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
//! After three complete rounds, a holder enables nomination. One ready caller
//! accepts continuation and waits outside all locks. Before the next round,
//! the holder grants it a new ownership generation and the scan cursor, keeping
//! pending notifications and resource-owning actions with admission. Acceptance
//! and the boundary decision use SeqCst RMWs: a nomination just after a decision
//! requires at most that one committed round before grant. This bounds work
//! after acceptance, not callback duration, lock waits, or thread descheduling.
//! A successor must complete its own batching threshold before becoming eligible.
//!
//! Without a successor the holder continues to checked quiet release. Initial
//! election and short drains skip the handoff mutex. Eligible release,
//! nomination, withdrawal, and grant serialize through that mutex; an accepted
//! successor wins against release even when there is currently no work.
//! There is no background executor or promise of fairness among all contenders.
//!
//! Dropping a baton processes already recorded demand and queued actions, also
//! during callback unwinding. Processing retains the first callback panic,
//! declines an ungranted nominee, disables further handoff, finishes recorded
//! work, releases the baton, and resumes that panic on the original caller. A
//! granted nominee's guard covers fallback if it unwinds after transfer. Recovery
//! retains synchronous draining and has no normal handoff bound. It does
//! not complete an interrupted caller's remaining cleanup or recover arbitrary
//! invariant failures or destructor double-panic aborts.

use super::{AdmissionAction, DemandSnapshot, OriginAdmission, PartitionId};
use crate::sync::{
    handoff_backoff, Arc, AtomicBool, AtomicUsize, LocalCompletion, Mutex, Ordering,
    ProcessingScope,
};
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
    /// Ownership, publication, and handoff hints share one modification order.
    state: AtomicUsize,
    /// Exact handoff identity. Initial election and short drains skip this lock.
    handoff: Mutex<HandoffState>,
    /// Stable per-partition slots, shared with cells without owning those cells.
    slots: Mutex<BTreeMap<PartitionId, Arc<DemandPublication>>>,
    /// Detached resource-owning actions, including successors and guard cleanup.
    actions: Mutex<VecDeque<AdmissionAction>>,
}

const HELD: usize = 1;
const PENDING: usize = 2;
const ELIGIBLE: usize = 4;
const SUCCESSOR: usize = 8;

// Batching policy, not a correctness or wall-clock bound.
const HANDOFF_AFTER_ROUNDS: usize = 3;

#[derive(Default)]
struct HandoffState {
    generation: usize,
    nominee: Option<Arc<Nominee>>,
}

const WAITING: usize = 0;
const GRANTED: usize = 1;
const DECLINED: usize = 2;

/// One accepted contender; only status publication makes transfer data visible.
struct Nominee {
    status: AtomicUsize,
    generation: AtomicUsize,
    cursor: AtomicUsize,
}

impl Default for AdmissionProcessing {
    fn default() -> Self {
        Self {
            state: AtomicUsize::new(0),
            handoff: Mutex::new(HandoffState::default()),
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
            generation: None,
            eligible: false,
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
            baton.process();
        }
    }

    /// Accept one ready successor, revalidating the hint under exact control.
    fn nominate(admission: &Arc<OriginAdmission>) -> Option<Nomination<'_>> {
        let processing = &admission.processing;
        if processing.state.load(Ordering::SeqCst) & (HELD | ELIGIBLE | SUCCESSOR)
            != HELD | ELIGIBLE
        {
            return None;
        }
        let mut handoff = processing.handoff.lock();
        if processing.state.load(Ordering::SeqCst) & (HELD | ELIGIBLE | SUCCESSOR)
            != HELD | ELIGIBLE
        {
            return None;
        }
        assert!(handoff.nominee.is_none());
        let ticket = Arc::new(Nominee {
            status: AtomicUsize::new(WAITING),
            generation: AtomicUsize::new(0),
            cursor: AtomicUsize::new(0),
        });
        handoff.nominee = Some(ticket.clone());
        // Acceptance linearizes here. The holder's boundary RMW observes this
        // modification or commits at most one round before observing it.
        processing.state.fetch_or(SUCCESSOR, Ordering::SeqCst);
        Some(Nomination {
            admission,
            ticket: Some(ticket),
        })
    }
}

/// A bounded publisher's local transition followed by optional baton processing.
///
/// Losing publishers borrow admission without allocating or cloning its Arc.
/// Finish cleanup and wakes before `process`: only an outermost ready caller,
/// outside every admission processor, may nominate and synchronously wait.
/// Drop retains initial-holder fallback but never nominates during unwinding.
#[must_use = "finish local cleanup and wakes, then call process"]
pub(in crate::client::pool) struct BatonScope<'a> {
    admission: &'a Arc<OriginAdmission>,
    baton: Option<AdmissionBaton>,
    completion: Option<LocalCompletion>,
}

impl<'a> BatonScope<'a> {
    pub(super) fn record(
        admission: &'a Arc<OriginAdmission>,
        publication: &DemandPublication,
        snapshots: impl IntoIterator<Item = DemandSnapshot>,
    ) -> Self {
        let mut scope = Self {
            admission,
            baton: None,
            completion: Some(LocalCompletion::enter()),
        };
        scope.baton = AdmissionProcessing::record(admission, publication, snapshots);
        scope
    }

    pub(in crate::client::pool) fn process(mut self) {
        let ready = self
            .completion
            .take()
            .expect("unfinished local scope")
            .finish();
        if let Some(baton) = self.baton.take() {
            baton.process();
        } else if ready {
            if let Some(nomination) = AdmissionProcessing::nominate(self.admission) {
                if let Some(baton) = nomination.wait() {
                    baton.process();
                }
            } else if self.admission.processing.state.load(Ordering::SeqCst) & HELD == 0 {
                // Eligible quiet release may have beaten this ready contender.
                // An ordinary claim still closes any last-publication race.
                if let Some(baton) = AdmissionProcessing::claim(self.admission, false) {
                    baton.process();
                }
            }
        }
    }
}

// Field order matters: the elected guard drains while local-completion tracking
// still excludes callbacks from nomination.
impl Drop for BatonScope<'_> {
    fn drop(&mut self) {
        drop(self.baton.take());
    }
}

/// Accepted continuation, with withdrawal or granted-holder fallback on drop.
struct Nomination<'a> {
    admission: &'a Arc<OriginAdmission>,
    ticket: Option<Arc<Nominee>>,
}

impl Nomination<'_> {
    fn granted(&self, ticket: &Nominee) -> AdmissionBaton {
        AdmissionBaton {
            admission: self.admission.clone(),
            held: true,
            cursor: ticket.cursor.load(Ordering::SeqCst),
            generation: Some(ticket.generation.load(Ordering::SeqCst)),
            eligible: false,
        }
    }

    fn wait(mut self) -> Option<AdmissionBaton> {
        let mut attempts = 0usize;
        loop {
            let ticket = self.ticket.as_ref().expect("accepted nomination");
            match ticket.status.load(Ordering::SeqCst) {
                GRANTED => {
                    let baton = self.granted(ticket);
                    self.ticket = None;
                    return Some(baton);
                }
                DECLINED => {
                    self.ticket = None;
                    return None;
                }
                WAITING => {
                    handoff_backoff(attempts);
                    attempts = attempts.saturating_add(1);
                }
                _ => unreachable!("invalid nomination status"),
            }
        }
    }
}

impl Drop for Nomination<'_> {
    fn drop(&mut self) {
        let Some(ticket) = self.ticket.take() else {
            return;
        };
        let granted = {
            let mut handoff = self.admission.processing.handoff.lock();
            match ticket.status.load(Ordering::SeqCst) {
                WAITING => {
                    assert!(handoff
                        .nominee
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &ticket)));
                    handoff.nominee = None;
                    self.admission
                        .processing
                        .state
                        .fetch_and(!SUCCESSOR, Ordering::SeqCst);
                    ticket.status.store(DECLINED, Ordering::SeqCst);
                    false
                }
                GRANTED => true,
                DECLINED => false,
                _ => unreachable!("invalid nomination status"),
            }
        };
        if granted {
            // Grant won against withdrawal. Cleanup now belongs to this caller,
            // and must happen outside the handoff lock even during unwinding.
            drop(self.granted(&ticket));
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
/// A holder returns after checked release or a grant to an accepted successor.
/// Without a ready contender, sustained publication can still occupy its runtime.
/// Unwind recovery covers recorded work, not the remainder of an interrupted
/// caller cleanup closure; see the module's progress and unwinding notes.
pub(in crate::client::pool) struct AdmissionBaton {
    admission: Arc<OriginAdmission>,
    held: bool,
    cursor: usize,
    /// Lazy identity: ordinary election avoids handoff metadata altogether.
    generation: Option<usize>,
    /// Local mirror: short ownership turns need no handoff boundary RMW.
    eligible: bool,
}

impl AdmissionBaton {
    /// Processes until checked release or transfer, then resumes any callback panic.
    ///
    /// Call after publishing local supply/cleanup updates and signalling waiters.
    /// Signalling does not make this runtime poll them before processing returns.
    pub(in crate::client::pool) fn process(mut self) {
        let panic = self.drain();
        if let Some(panic) = panic {
            resume_unwind(panic);
        }
    }

    /// Enable contender nomination only after this ownership turn did work.
    fn enable_handoff(&mut self) {
        let processing = &self.admission.processing;
        let mut handoff = processing.handoff.lock();
        if let Some(generation) = self.generation {
            assert_eq!(generation, handoff.generation);
        } else {
            handoff.generation = handoff.generation.wrapping_add(1);
            self.generation = Some(handoff.generation);
        }
        processing.state.fetch_or(ELIGIBLE, Ordering::SeqCst);
        self.eligible = true;
    }

    /// Grant while exact control is locked. The predecessor disarms before the
    /// ticket becomes visible; PENDING and the action queue stay with admission.
    fn grant(
        processing: &AdmissionProcessing,
        generation: Option<usize>,
        held: &mut bool,
        cursor: usize,
        handoff: &mut HandoffState,
    ) -> bool {
        assert_eq!(generation, Some(handoff.generation));
        let Some(ticket) = handoff.nominee.take() else {
            return false;
        };
        handoff.generation = handoff.generation.wrapping_add(1);
        *held = false;
        processing
            .state
            .fetch_and(!(ELIGIBLE | SUCCESSOR), Ordering::SeqCst);
        ticket.cursor.store(cursor, Ordering::SeqCst);
        ticket
            .generation
            .store(handoff.generation, Ordering::SeqCst);
        ticket.status.store(GRANTED, Ordering::SeqCst);
        true
    }

    /// Boundary decision precedes consuming any scheduling notification.
    fn transfer_at_boundary(&mut self) -> bool {
        if !self.eligible {
            return false;
        }
        let processing = &self.admission.processing;
        if processing.state.fetch_or(0, Ordering::SeqCst) & SUCCESSOR == 0 {
            return false;
        }
        let mut handoff = processing.handoff.lock();
        Self::grant(
            processing,
            self.generation,
            &mut self.held,
            self.cursor,
            &mut handoff,
        )
    }

    /// Recovery keeps this owner and explicitly releases a waiting contender.
    fn decline_nominee(&mut self) {
        if self.generation.is_none() {
            return;
        }
        let processing = &self.admission.processing;
        let mut handoff = processing.handoff.lock();
        assert_eq!(self.generation, Some(handoff.generation));
        processing
            .state
            .fetch_and(!(ELIGIBLE | SUCCESSOR), Ordering::SeqCst);
        if let Some(ticket) = handoff.nominee.take() {
            ticket.status.store(DECLINED, Ordering::SeqCst);
        }
        self.eligible = false;
    }

    /// Exact quiet release shares its serialization point with nomination.
    fn release(&mut self) -> bool {
        if self.generation.is_some() {
            let processing = &self.admission.processing;
            let mut handoff = processing.handoff.lock();
            if Self::grant(
                processing,
                self.generation,
                &mut self.held,
                self.cursor,
                &mut handoff,
            ) {
                return true;
            }
            processing.state.fetch_and(!ELIGIBLE, Ordering::SeqCst);
            if processing
                .state
                .compare_exchange(HELD, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.held = false;
                return true;
            }
            if self.eligible {
                processing.state.fetch_or(ELIGIBLE, Ordering::SeqCst);
            }
            false
        } else if self
            .admission
            .processing
            .state
            .compare_exchange(HELD, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.held = false;
            true
        } else {
            false
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

    /// Processes until checked release or transfer to an accepted successor.
    ///
    /// Returns the first callback panic only after synchronous recovery/release. A
    /// concurrent `PENDING` signal prevents release and makes this holder retry.
    fn drain(&mut self) -> Option<Box<dyn Any + Send>> {
        self.drain_after(HANDOFF_AFTER_ROUNDS)
    }

    fn drain_after(&mut self, eligibility_rounds: usize) -> Option<Box<dyn Any + Send>> {
        let _processing = ProcessingScope::enter();
        let mut first_panic = None;
        let mut rounds = 0usize;
        let mut recovering = std::thread::panicking();
        if recovering {
            self.decline_nominee();
        }
        loop {
            if !recovering && self.transfer_at_boundary() {
                return None;
            }
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
                        recovering = true;
                        self.decline_nominee();
                    } else {
                        discard_panic(panic);
                    }
                }
                rounds = rounds.saturating_add(1);
                if !recovering && !self.eligible && rounds >= eligibility_rounds {
                    self.enable_handoff();
                }
                continue;
            }

            if self.release() {
                return first_panic;
            }
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
        assert!(origin.processing.handoff.lock().nominee.is_none());
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

    // A real watchdog detects thread/runtime stalls. An injected pool clock
    // cannot advance while the very runtime whose progress we test is occupied.
    #[allow(clippy::disallowed_methods)]
    fn wait_for(ready: impl Fn() -> bool, failure: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !ready() {
            assert!(std::time::Instant::now() < deadline, "{failure}");
            std::thread::yield_now();
        }
    }

    fn eligible_baton(origin: &Arc<OriginAdmission>) -> AdmissionBaton {
        let slot = origin.processing.slot(PartitionId::from_index(1));
        let mut baton = AdmissionProcessing::record(origin, &slot, [inactive(0)]).unwrap();
        origin
            .processing
            .state
            .fetch_and(!PENDING, Ordering::SeqCst);
        baton.round(true);
        baton.enable_handoff();
        baton
    }

    #[test]
    fn nomination_before_boundary_transfers_pending_work_and_cursor() {
        let origin = origin();
        origin.processing.slot(PartitionId::from_index(2));
        let mut predecessor = eligible_baton(&origin);
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        let cursor = predecessor.cursor;
        let old_generation = predecessor.generation;
        let slot = origin.processing.slot(PartitionId::from_index(2));
        assert!(AdmissionProcessing::record(&origin, &slot, [inactive(7)]).is_none());
        assert!(predecessor.transfer_at_boundary());
        assert!(!predecessor.held);
        assert_ne!(0, origin.processing.state.load(Ordering::SeqCst) & PENDING);
        let successor = nominee.wait().unwrap();
        assert_eq!(cursor, successor.cursor);
        assert_ne!(old_generation, successor.generation);
        assert_eq!(0, origin.processing.state.load(Ordering::SeqCst) & ELIGIBLE);
        successor.process();
        // A successor can finish before the old guard is destroyed.
        drop(predecessor);
        assert_eq!(
            Some(&inactive(7)),
            origin
                .state
                .lock()
                .demand
                .latest_for_test(&PartitionId::from_index(2))
        );
        assert_quiescent(&origin);
    }

    #[test]
    fn nomination_after_boundary_commits_only_one_more_round() {
        let origin = origin();
        let mut predecessor = eligible_baton(&origin);
        // The actual production RMW commits to the next round.
        assert!(!predecessor.transfer_at_boundary());
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        let slot = origin.processing.slot(PartitionId::from_index(1));
        assert!(AdmissionProcessing::record(&origin, &slot, [inactive(9)]).is_none());
        let requested = origin
            .processing
            .state
            .fetch_and(!PENDING, Ordering::SeqCst)
            & PENDING
            != 0;
        predecessor.round(requested);
        assert!(predecessor.transfer_at_boundary());
        nominee.wait().unwrap().process();
        drop(predecessor);
        assert_quiescent(&origin);
    }

    #[test]
    fn one_nominee_withdraws_and_another_can_take_its_position() {
        let origin = origin();
        let mut predecessor = eligible_baton(&origin);
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        assert!(AdmissionProcessing::nominate(&origin).is_none());
        drop(nominee);
        let next = AdmissionProcessing::nominate(&origin).unwrap();
        assert!(predecessor.transfer_at_boundary());
        // The granted nominee's drop must drain, even without another event.
        drop(next);
        drop(predecessor);
        assert_quiescent(&origin);
    }

    #[test]
    fn nominee_unwind_withdraws_or_drains_after_grant() {
        let origin = origin();
        let mut predecessor = eligible_baton(&origin);
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(move || {
            let _nominee = nominee;
            panic!("before grant");
        }))
        .is_err());
        assert!(predecessor.held);
        assert_eq!(
            0,
            origin.processing.state.load(Ordering::SeqCst) & SUCCESSOR
        );
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        let slot = origin.processing.slot(PartitionId::from_index(1));
        assert!(AdmissionProcessing::record(&origin, &slot, [inactive(9)]).is_none());
        assert!(predecessor.transfer_at_boundary());
        assert!(catch_unwind(AssertUnwindSafe(move || {
            let _nominee = nominee;
            panic!("after grant");
        }))
        .is_err());
        drop(predecessor);
        assert_eq!(
            Some(&inactive(9)),
            origin
                .state
                .lock()
                .demand
                .latest_for_test(&PartitionId::from_index(1))
        );
        assert_quiescent(&origin);
    }

    #[test]
    fn new_holder_release_race_does_not_enable_early_nomination() {
        let origin = origin();
        let mut predecessor = eligible_baton(&origin);
        let nominee = AdmissionProcessing::nominate(&origin).unwrap();
        assert!(predecessor.transfer_at_boundary());
        let mut successor = nominee.wait().unwrap();
        let slot = origin.processing.slot(PartitionId::from_index(1));
        assert!(AdmissionProcessing::record(&origin, &slot, [inactive(9)]).is_none());
        assert!(!successor.release());
        assert!(!successor.eligible);
        assert_eq!(0, origin.processing.state.load(Ordering::SeqCst) & ELIGIBLE);
        assert!(AdmissionProcessing::nominate(&origin).is_none());
        successor.process();
        drop(predecessor);
        assert_quiescent(&origin);
    }

    #[test]
    fn one_round_policy_enables_only_after_a_complete_round() {
        struct CheckEligibility {
            origin: Arc<OriginAdmission>,
            slot: Arc<DemandPublication>,
            wakes: std::sync::atomic::AtomicUsize,
        }
        impl Wake for CheckEligibility {
            fn wake(self: std::sync::Arc<Self>) {
                let completed = self.wakes.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    completed > 0,
                    self.origin.processing.state.load(Ordering::SeqCst) & ELIGIBLE != 0
                );
                BatonScope::record(&self.origin, &self.slot, [inactive(completed as u64)])
                    .process();
            }
        }
        let origin = OriginAdmission::for_test(std::num::NonZeroUsize::new(2).unwrap());
        let check = std::sync::Arc::new(CheckEligibility {
            origin: origin.clone(),
            slot: origin.processing.slot(PartitionId::from_index(3)),
            wakes: std::sync::atomic::AtomicUsize::new(0),
        });
        let waker = Waker::from(check.clone());
        let mut baton = None;
        let mut waiters = Vec::new();
        for id in 1..=2 {
            let cell = cell(&origin, id);
            let (waiter, snapshot) =
                cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(&waker))
                .is_pending());
            if let Some(elected) =
                AdmissionProcessing::record(&origin, &cell.demand_publication, [snapshot])
            {
                baton = Some(elected);
            }
            waiters.push((cell, waiter));
        }
        assert!(baton.as_mut().unwrap().drain_after(1).is_none());
        drop(baton);
        assert_eq!(2, check.wakes.load(Ordering::SeqCst));
        for (cell, waiter) in waiters {
            drop(cell.poll_waiter(waiter, &mut Context::from_waker(Waker::noop())));
        }
        assert_eq!(2, origin.available_capacity_for_test());
        assert_quiescent(&origin);
    }

    #[test]
    fn initial_election_and_short_processing_skip_handoff_metadata() {
        let origin = origin();
        let slot = origin.processing.slot(PartitionId::from_index(1));
        let baton = AdmissionProcessing::record(&origin, &slot, [inactive(0)]).unwrap();
        let control = origin.processing.handoff.lock();
        assert!(baton.generation.is_none());
        // The no-nested-lock checker proves claim did not take handoff control.
        assert!(AdmissionProcessing::claim(&origin, false).is_none());
        drop(control);
        baton.process();
        assert_eq!(0, origin.processing.handoff.lock().generation);
        assert_quiescent(&origin);
    }

    #[test]
    fn nested_cleanup_and_any_origin_processor_suppress_nomination() {
        let first = origin();
        let second = origin();
        let predecessor = eligible_baton(&first);
        let other = eligible_baton(&second);
        let slot = first.processing.slot(PartitionId::from_index(1));
        let outer = BatonScope::record(&first, &slot, []);
        let inner = BatonScope::record(&first, &slot, []);
        inner.process(); // Would wait forever if it nominated.
        assert!(first.processing.handoff.lock().nominee.is_none());
        drop(outer);
        let processing = ProcessingScope::enter();
        let other_slot = second.processing.slot(PartitionId::from_index(1));
        BatonScope::record(&second, &other_slot, []).process();
        assert!(second.processing.handoff.lock().nominee.is_none());
        drop(processing);
        drop(predecessor);
        drop(other);
        assert_quiescent(&first);
        assert_quiescent(&second);
    }

    #[test]
    fn panic_declines_accepted_nominee_and_keeps_recovery_on_original_caller() {
        struct NominateThenPanic {
            start: std::sync::mpsc::Sender<()>,
            accepted: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl Wake for NominateThenPanic {
            fn wake(self: std::sync::Arc<Self>) {
                self.start.send(()).unwrap();
                self.accepted
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                panic!("original");
            }
        }
        let origin = origin();
        // Keep this live demand distinct from the quiet setup slot.
        let first = cell(&origin, 2);
        let (waiter, demand) =
            first.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let (start, started) = std::sync::mpsc::channel();
        let (accepted, acceptance) = std::sync::mpsc::channel();
        let waker = Waker::from(std::sync::Arc::new(NominateThenPanic {
            start,
            accepted: std::sync::Mutex::new(acceptance),
        }));
        assert!(first
            .poll_waiter(waiter, &mut Context::from_waker(&waker))
            .is_pending());
        let predecessor = eligible_baton(&origin);
        let contender = {
            let origin = origin.clone();
            std::thread::spawn(move || {
                started
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                let nominee = AdmissionProcessing::nominate(&origin).unwrap();
                accepted.send(()).unwrap();
                assert!(
                    nominee.wait().is_none(),
                    "recovery must decline rather than transfer"
                );
            })
        };
        assert!(
            AdmissionProcessing::record(&origin, &first.demand_publication, [demand]).is_none()
        );
        let panic = catch_unwind(AssertUnwindSafe(|| predecessor.process())).unwrap_err();
        contender.join().unwrap();
        assert_eq!(Some(&"original"), panic.downcast_ref::<&str>());
        drop(first.poll_waiter(waiter, &mut Context::from_waker(Waker::noop())));
        assert_eq!(1, origin.available_capacity_for_test());
        assert_quiescent(&origin);
    }

    #[test]
    fn scope_cleanup_finishes_before_successor_wait_and_processing() {
        let origin = origin();
        let mut predecessor = eligible_baton(&origin);
        let cleanup = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let contender = {
            let origin = origin.clone();
            let cleanup = cleanup.clone();
            std::thread::spawn(move || {
                let slot = origin.processing.slot(PartitionId::from_index(2));
                let scope = BatonScope::record(&origin, &slot, [inactive(8)]);
                cleanup.store(true, Ordering::SeqCst);
                scope.process();
            })
        };
        wait_for(
            || origin.processing.state.load(Ordering::SeqCst) & SUCCESSOR != 0,
            "nomination did not arrive",
        );
        assert!(cleanup.load(Ordering::SeqCst));
        assert!(predecessor.transfer_at_boundary());
        contender.join().unwrap();
        drop(predecessor);
        assert_quiescent(&origin);
    }

    #[test]
    fn handoff_resumes_current_thread_runtime_while_peer_keeps_processing() {
        struct ProgressWake {
            origin: Arc<OriginAdmission>,
            slot: Arc<DemandPublication>,
            wakes: std::sync::atomic::AtomicUsize,
            start: std::sync::mpsc::Sender<()>,
            heartbeat: std::sync::atomic::AtomicBool,
        }
        impl Wake for ProgressWake {
            fn wake(self: std::sync::Arc<Self>) {
                let completed = self.wakes.fetch_add(1, Ordering::SeqCst) + 1;
                // Each action republishes, keeping another scheduling pass due.
                BatonScope::record(&self.origin, &self.slot, [inactive(completed as u64)])
                    .process();
                if completed == HANDOFF_AFTER_ROUNDS + 1 {
                    self.start.send(()).unwrap();
                    wait_for(
                        || self.origin.processing.state.load(Ordering::SeqCst) & SUCCESSOR != 0,
                        "successor did not nominate",
                    );
                } else if completed == HANDOFF_AFTER_ROUNDS + 2 {
                    // The successor is running the next action. Its predecessor
                    // must now be able to poll another task on its own runtime.
                    wait_for(
                        || self.heartbeat.load(Ordering::SeqCst),
                        "predecessor runtime stayed occupied",
                    );
                }
            }
        }
        let origin = OriginAdmission::for_test(std::num::NonZeroUsize::new(10).unwrap());
        let (start, started) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(ProgressWake {
            origin: origin.clone(),
            slot: origin.processing.slot(PartitionId::from_index(11)),
            wakes: std::sync::atomic::AtomicUsize::new(0),
            start,
            heartbeat: std::sync::atomic::AtomicBool::new(false),
        });
        let waker = Waker::from(progress.clone());
        let mut first_baton = None;
        let mut waiters = Vec::new();
        for id in 1..=10 {
            let cell = cell(&origin, id);
            let (waiter, snapshot) =
                cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            assert!(cell
                .poll_waiter(waiter, &mut Context::from_waker(&waker))
                .is_pending());
            if let Some(baton) =
                AdmissionProcessing::record(&origin, &cell.demand_publication, [snapshot])
            {
                assert!(first_baton.is_none());
                first_baton = Some(baton);
            }
            waiters.push((cell, waiter));
        }
        let contender = {
            let origin = origin.clone();
            std::thread::spawn(move || {
                started
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap()
                    .block_on(async {
                        let slot = origin.processing.slot(PartitionId::from_index(12));
                        BatonScope::record(&origin, &slot, [inactive(0)]).process();
                    });
            })
        };
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                first_baton.unwrap().process();
                tokio::task::yield_now().await;
                progress.heartbeat.store(true, Ordering::SeqCst);
            });
        contender.join().unwrap();
        assert_eq!(10, progress.wakes.load(Ordering::SeqCst));
        assert!(progress.heartbeat.load(Ordering::SeqCst));
        assert_eq!(0, origin.available_capacity_for_test());
        for (cell, waiter) in waiters {
            let ready = cell.poll_waiter(waiter, &mut Context::from_waker(Waker::noop()));
            assert!(ready.is_ready(), "handoff lost a capacity delivery");
            drop(ready);
        }
        assert_eq!(10, origin.available_capacity_for_test());
        assert_quiescent(&origin);
    }

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
        baton.process();
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
            .process();
        origin.state.lock().return_permit(held);
        AdmissionProcessing::record(&origin, &cell.demand_publication, [snapshot])
            .unwrap()
            .process();
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
        let panic = catch_unwind(AssertUnwindSafe(|| baton.process())).unwrap_err();
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
        baton.process();
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
        let panic = catch_unwind(AssertUnwindSafe(|| baton.process())).unwrap_err();
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
                .process();
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

    fn eligible_baton(origin: &Arc<OriginAdmission>) -> AdmissionBaton {
        let slot = origin.processing.slot(PartitionId::from_index(1));
        let mut baton = AdmissionProcessing::record(origin, &slot, [inactive(0)]).unwrap();
        origin
            .processing
            .state
            .fetch_and(!PENDING, Ordering::SeqCst);
        baton.round(true);
        baton.enable_handoff();
        baton
    }

    #[test]
    fn nomination_races_quiet_release_and_last_publication() {
        loom::model(|| {
            let origin = origin();
            let baton = eligible_baton(&origin);
            let holder = loom::thread::spawn(move || baton.process());
            let writer = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    let slot = origin.processing.slot(PartitionId::from_index(1));
                    BatonScope::record(&origin, &slot, [inactive(7)]).process();
                })
            };
            holder.join().unwrap();
            writer.join().unwrap();
            assert_eq!(
                Some(&inactive(7)),
                origin
                    .state
                    .lock()
                    .demand
                    .latest_for_test(&PartitionId::from_index(1))
            );
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn competing_ready_callers_accept_only_one_successor() {
        loom::model(|| {
            let origin = origin();
            let baton = eligible_baton(&origin);
            let accepted = Arc::new(AtomicUsize::new(0));
            let spawn_contender = || {
                let origin = origin.clone();
                let accepted = accepted.clone();
                loom::thread::spawn(move || {
                    if let Some(nominee) = AdmissionProcessing::nominate(&origin) {
                        accepted.fetch_add(1, Ordering::SeqCst);
                        nominee.wait().unwrap().process();
                    }
                })
            };
            let first = spawn_contender();
            let second = spawn_contender();
            baton.process();
            first.join().unwrap();
            second.join().unwrap();
            assert!(accepted.load(Ordering::SeqCst) <= 1);
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn withdrawal_races_grant_and_last_cleanup() {
        loom::model(|| {
            let origin = origin();
            let baton = eligible_baton(&origin);
            let mut nomination = AdmissionProcessing::nominate(&origin).unwrap();
            let ticket = nomination.ticket.take().unwrap();
            let withdrawal = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    // Reconstruct the same accepted guard in its contender.
                    let nomination = Nomination {
                        admission: &origin,
                        ticket: Some(ticket),
                    };
                    let slot = origin.processing.slot(PartitionId::from_index(1));
                    assert!(AdmissionProcessing::record(&origin, &slot, [inactive(8)]).is_none());
                    drop(nomination);
                })
            };
            baton.process();
            withdrawal.join().unwrap();
            assert_eq!(
                Some(&inactive(8)),
                origin
                    .state
                    .lock()
                    .demand
                    .latest_for_test(&PartitionId::from_index(1))
            );
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn decline_races_withdrawal_without_abandoning_ownership() {
        loom::model(|| {
            let origin = origin();
            let mut baton = eligible_baton(&origin);
            let mut nomination = AdmissionProcessing::nominate(&origin).unwrap();
            let ticket = nomination.ticket.take().unwrap();
            let withdrawal = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    drop(Nomination {
                        admission: &origin,
                        ticket: Some(ticket),
                    })
                })
            };
            baton.decline_nominee();
            assert!(baton.held);
            assert_eq!(
                0,
                origin.processing.state.load(Ordering::SeqCst) & (ELIGIBLE | SUCCESSOR)
            );
            baton.process();
            withdrawal.join().unwrap();
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn transfer_preserves_last_signal_and_old_guard_cannot_release_successor() {
        loom::model(|| {
            let origin = origin();
            let mut predecessor = eligible_baton(&origin);
            let nomination = AdmissionProcessing::nominate(&origin).unwrap();
            let writer = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    let slot = origin.processing.slot(PartitionId::from_index(1));
                    if let Some(baton) = AdmissionProcessing::record(&origin, &slot, [inactive(9)])
                    {
                        baton.process();
                    }
                })
            };
            assert!(predecessor.transfer_at_boundary());
            let successor = nomination.wait().unwrap();
            assert!(!predecessor.held);
            let cursor = predecessor.cursor;
            assert_eq!(cursor, successor.cursor);
            successor.process();
            // Deliberately let successor finish before predecessor teardown.
            drop(predecessor);
            writer.join().unwrap();
            assert_eq!(
                Some(&inactive(9)),
                origin
                    .state
                    .lock()
                    .demand
                    .latest_for_test(&PartitionId::from_index(1))
            );
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn nested_completion_and_cross_origin_processing_cannot_wait() {
        loom::model(|| {
            let origin = origin();
            let baton = eligible_baton(&origin);
            let slot = origin.processing.slot(PartitionId::from_index(1));
            let outer = BatonScope::record(&origin, &slot, []);
            BatonScope::record(&origin, &slot, []).process();
            assert!(origin.processing.handoff.lock().nominee.is_none());
            drop(outer);
            // A processor on this thread belongs to any origin.
            let processing = ProcessingScope::enter();
            BatonScope::record(&origin, &slot, []).process();
            assert!(origin.processing.handoff.lock().nominee.is_none());
            let contender = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    // This other modeled thread must not inherit our TLS.
                    BatonScope::record(&origin, &slot, []).process();
                })
            };
            drop(processing);
            baton.process();
            contender.join().unwrap();
            assert_quiescent(&origin);
        });
    }

    #[test]
    fn baton_release_races_last_publication_and_cancellation() {
        loom::model(|| {
            let origin = origin();
            let id = PartitionId::from_index(1);
            let slot = origin.processing.slot(id);
            let held = origin.state.lock().take_permit().unwrap();
            let baton = AdmissionProcessing::record(&origin, &slot, [active(0)]).unwrap();
            let holder = loom::thread::spawn(move || baton.process());
            let writer = {
                let origin = origin.clone();
                let slot = slot.clone();
                loom::thread::spawn(move || {
                    if let Some(baton) =
                        AdmissionProcessing::record(&origin, &slot, [active(1), inactive(1)])
                    {
                        baton.process();
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
            let holder = loom::thread::spawn(move || baton.process());
            let writer = {
                let origin = origin.clone();
                loom::thread::spawn(move || {
                    let slot = origin.processing.slot(second);
                    if let Some(baton) =
                        AdmissionProcessing::record(&origin, &slot, [inactive(0), inactive(0)])
                    {
                        baton.process();
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
            let holder = loom::thread::spawn(move || baton.process());
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

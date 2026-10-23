/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Applies operation sequences to one production acquisition queue.
//!
//! The harness owns one [`AcquisitionQueue`] and plays every caller: the
//! acquisition futures that register, poll, and cancel; admission deliveries
//! that reserve and then commit after an unlocked gap; returned HTTP/1
//! senders and HTTP/2 activations; and establishment tasks that start and
//! report. It keeps its own model of each attempt from the values those calls
//! return, never from queue internals, and after every operation compares
//! that model with the queue's state.
//!
//! The comparison covers what the queue's own `assert_consistent` cannot:
//! every outcome handed to the queue is in exactly one place; permits are
//! conserved by count; demand generations retire and succeed in order; a
//! returned sender or activation goes to the oldest compatible attempt; and
//! an attempt that polled `Pending` gets its waker back on the transition
//! that would make its next poll ready.
//!
//! [`Harness::quiesce`] drives every live attempt to a delivered outcome the
//! way production would and asserts the queue is empty afterwards.

use super::super::{
    AcquisitionQueue, CellCommitOutcome, DeliveryReservation, EstablishmentPhase, WaiterId,
    WaiterResolution, WaiterState, WaitingQueueState,
};
use super::operation_sequence::{
    CutoffSelector, DemandSelector, Lane, Operation, Requirement, CROSSINGS, SLOTS,
};
use crate::client::pool::admission::{
    DemandId, DemandSnapshot, ProtocolRequirement, SnapshotVersion,
};
use crate::client::pool::cell::{AcquisitionOutcome, AcquisitionStep, EstablishmentPermit};
use crate::client::pool::partition::EligibilityGroup;
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

/// Upper bound on quiesce steps before the harness reports a livelock.
const QUIESCE_STEP_LIMIT: usize = 1_024;

/// Identity carried through the queue inside an otherwise opaque outcome.
///
/// The queue never inspects an outcome, so a failed outcome stands in for
/// every variant. The token rides in the connector error's source.
#[derive(Debug)]
struct Token(u64);

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "harness token {}", self.0)
    }
}

impl Error for Token {}

fn tagged_outcome(token: u64) -> AcquisitionOutcome {
    AcquisitionOutcome::Failed(ConnectorError::other(Box::new(Token(token)), None))
}

fn token_of(outcome: &AcquisitionOutcome) -> u64 {
    let AcquisitionOutcome::Failed(error) = outcome else {
        panic!("harness only injects failed outcomes, saw {outcome:?}");
    };
    error
        .source()
        .and_then(|source| source.downcast_ref::<Token>())
        .expect("outcome did not carry a harness token")
        .0
}

/// Counts wakes so the harness can prove a returned waker was this slot's.
#[derive(Debug, Default)]
struct SlotWaker {
    wakes: AtomicUsize,
}

impl Wake for SlotWaker {
    fn wake(self: Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

/// What the harness believes one attempt's queue residence is.
///
/// Derived only from what the queue returned to the harness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    /// Linked in the FIFO.
    Waiting,
    /// Reserved by a delivery that has not committed.
    Reserved,
    /// Reserved, and a local result arrived while the delivery was crossing.
    ReservedWithPending,
    /// Cancelled while reserved; the delivery must still commit.
    CancelledReserved,
    /// Holds a permit; the next poll starts establishment.
    Admitted,
    /// Poll handed out the permit; the task has not started.
    LaunchSubmitted,
    /// The establishment task started.
    LaunchStarted,
    /// Holds a terminal outcome; the next poll delivers it.
    HasResult,
    /// The record is gone but an establishment task is still outstanding.
    Detached,
}

/// Whether an establishment task exists for a slot and what it has reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Launch {
    None,
    Submitted,
    Started,
    Finished,
}

#[derive(Debug)]
struct Slot {
    waiter: WaiterId,
    requirement: ProtocolRequirement,
    phase: Phase,
    launch: Launch,
    waker: Arc<SlotWaker>,
    /// Polled `Pending` and not yet woken. The transition that makes the
    /// next poll ready must return this slot's waker; nothing else may.
    awaiting_wake: bool,
    /// Local result installed while a delivery was crossing.
    pending_token: Option<u64>,
}

impl Slot {
    fn new(waiter: WaiterId, requirement: ProtocolRequirement, phase: Phase) -> Self {
        Self {
            waiter,
            requirement,
            phase,
            launch: Launch::None,
            waker: Arc::default(),
            awaiting_wake: false,
            pending_token: None,
        }
    }

    /// Whether the record is in the queue.
    fn has_record(&self) -> bool {
        self.phase != Phase::Detached
    }

    /// Whether an establishment task will still call back.
    fn task_outstanding(&self) -> bool {
        matches!(self.launch, Launch::Submitted | Launch::Started)
    }

    /// Whether the queue's compatible-protocol indexes should hold this slot.
    fn is_indexed(&self) -> bool {
        matches!(
            self.phase,
            Phase::Reserved | Phase::Admitted | Phase::LaunchSubmitted | Phase::LaunchStarted
        )
    }

    /// Whether `pending_count` should include this slot.
    fn is_pending(&self) -> bool {
        matches!(
            self.phase,
            Phase::Waiting
                | Phase::Reserved
                | Phase::ReservedWithPending
                | Phase::Admitted
                | Phase::LaunchSubmitted
                | Phase::LaunchStarted
        )
    }
}

/// Where each injected outcome is right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenPlace {
    /// Handed to the queue during the current operation; residence unknown.
    Injected,
    /// Held in a record.
    StateOwned,
    /// Returned to the harness as a detached step.
    Returned,
    /// Delivered to the attempt through poll.
    Delivered,
}

#[derive(Debug, Default)]
struct TokenLedger {
    next: u64,
    places: BTreeMap<u64, TokenPlace>,
}

impl TokenLedger {
    fn allocate(&mut self) -> u64 {
        let token = self.next;
        self.next += 1;
        token
    }

    fn inject(&mut self, token: u64) {
        let replaced = self.places.insert(token, TokenPlace::Injected);
        assert_eq!(None, replaced, "token {token} injected twice");
    }

    fn returned(&mut self, token: u64) {
        let place = self.places.get_mut(&token).expect("returned unknown token");
        assert!(
            matches!(*place, TokenPlace::Injected | TokenPlace::StateOwned),
            "token {token} returned from {place:?}"
        );
        *place = TokenPlace::Returned;
    }

    fn delivered(&mut self, token: u64) {
        let place = self
            .places
            .get_mut(&token)
            .expect("delivered unknown token");
        assert_eq!(
            TokenPlace::StateOwned,
            *place,
            "token {token} delivered from {place:?}"
        );
        *place = TokenPlace::Delivered;
    }

    /// Everything injected this operation and not returned is state-owned.
    fn settle(&mut self) {
        for place in self.places.values_mut() {
            if *place == TokenPlace::Injected {
                *place = TokenPlace::StateOwned;
            }
        }
    }

    fn state_owned(&self) -> BTreeSet<u64> {
        self.places
            .iter()
            .filter(|(_, place)| **place == TokenPlace::StateOwned)
            .map(|(token, _)| *token)
            .collect()
    }

    fn any_injected(&self) -> bool {
        self.places
            .values()
            .any(|place| *place == TokenPlace::Injected)
    }
}

#[derive(Debug)]
struct ActiveDemand {
    id: DemandId,
    version: SnapshotVersion,
    /// Slot the queue should describe as the FIFO head.
    head: usize,
    requirement: ProtocolRequirement,
}

/// The harness's copy of the demand generation sequence.
#[derive(Debug, Default)]
struct DemandLedger {
    active: Option<ActiveDemand>,
    retired: Vec<DemandId>,
    next_id: u64,
}

impl DemandLedger {
    /// Starts the generation for a new FIFO head and returns its snapshot.
    fn begin(
        &mut self,
        head: usize,
        requirement: ProtocolRequirement,
        group: &EligibilityGroup,
    ) -> DemandSnapshot {
        assert!(self.active.is_none(), "began a demand while one was active");
        let id = DemandId::from_u64(self.next_id);
        self.next_id += 1;
        self.active = Some(ActiveDemand {
            id,
            version: SnapshotVersion::INITIAL,
            head,
            requirement,
        });
        DemandSnapshot::active(id, SnapshotVersion::INITIAL, requirement, group.clone())
    }

    /// Ends the active generation and returns the snapshot that retires it.
    fn retire(&mut self) -> DemandSnapshot {
        let active = self.active.take().expect("retired without active demand");
        self.retired.push(active.id);
        DemandSnapshot::inactive(active.id, active.version.next())
    }

    fn current_snapshot(&self, group: &EligibilityGroup) -> Option<DemandSnapshot> {
        self.active.as_ref().map(|active| {
            DemandSnapshot::active(active.id, active.version, active.requirement, group.clone())
        })
    }

    /// A generation the queue must refuse: the most recently retired one, or
    /// one that was never issued.
    fn stale_id(&self) -> DemandId {
        self.retired
            .last()
            .copied()
            .unwrap_or(DemandId::from_u64(u64::MAX))
    }
}

/// One reserved delivery that has not yet committed.
#[derive(Debug)]
struct Crossing {
    slot: usize,
}

/// Which kind of protocol result an offer carried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Offer {
    H1,
    H2,
}

macro_rules! report_fields {
    ($($field:ident),* $(,)?) => {
        /// Counts of what one sequence exercised, for coverage reporting.
        #[derive(Debug, Default, Eq, PartialEq)]
        pub(in crate::client::pool::cell::waiters) struct SequenceReport {
            $(pub(in crate::client::pool::cell::waiters) $field: usize,)*
        }

        impl SequenceReport {
            pub(in crate::client::pool::cell::waiters) fn absorb(&mut self, other: &Self) {
                $(self.$field += other.$field;)*
            }
        }
    };
}

report_fields! {
    operations,
    skipped,
    registrations,
    cancel_waiting_head,
    cancel_waiting_other,
    cancel_reserved,
    cancel_admitted,
    cancel_ready,
    cancel_launching,
    cancel_repeated,
    polls_pending,
    polls_start,
    polls_resolved,
    start_true,
    start_false,
    establish_committed_submitted,
    establish_committed_started,
    establish_refused,
    reservations,
    reservations_rejected,
    capacity_committed,
    capacity_refused_pending,
    capacity_refused_cancelled,
    borrowed_committed,
    borrowed_refused_pending,
    borrowed_refused_cancelled,
    h1_to_waiting,
    h1_to_reserved,
    h1_to_admitted,
    h1_to_launching,
    h1_refused,
    h2_to_waiting,
    h2_to_reserved,
    h2_to_admitted,
    h2_to_launching,
    h2_none,
    supersede_true,
    supersede_false,
    wakes,
    quiesce_runs,
    quiesce_steps,
}

/// Production queue plus the harness's model of every caller.
pub(in crate::client::pool::cell::waiters) struct Harness {
    lane: Lane,
    group: EligibilityGroup,
    queue: AcquisitionQueue,
    slots: Vec<Option<Slot>>,
    crossings: Vec<Crossing>,
    tokens: TokenLedger,
    /// Permits the queue currently holds in `ReadyToEstablish` records.
    permits_in_queue: usize,
    demand: DemandLedger,
    last_registered: Option<WaiterId>,
    report: SequenceReport,
}

/// Runs one sequence and returns what it exercised.
pub(in crate::client::pool::cell::waiters) fn run_sequence(
    lane: Lane,
    operations: &[Operation],
) -> SequenceReport {
    let mut harness = Harness::new(lane);
    for operation in operations {
        harness.apply(operation);
    }
    harness.quiesce();
    harness.finish()
}

impl Harness {
    pub(in crate::client::pool::cell::waiters) fn new(lane: Lane) -> Self {
        Self {
            lane,
            group: EligibilityGroup::Pool,
            queue: AcquisitionQueue::default(),
            slots: (0..SLOTS).map(|_| None).collect(),
            crossings: Vec::new(),
            tokens: TokenLedger::default(),
            permits_in_queue: 0,
            demand: DemandLedger::default(),
            last_registered: None,
            report: SequenceReport::default(),
        }
    }

    pub(in crate::client::pool::cell::waiters) fn finish(self) -> SequenceReport {
        self.report
    }

    /// Applies one operation, skipping it if its target cannot take it.
    pub(in crate::client::pool::cell::waiters) fn apply(&mut self, operation: &Operation) {
        self.report.operations += 1;
        let applied = match operation.clone() {
            Operation::Register { slot, requirement } => self.register(slot, requirement),
            Operation::Cancel { slot } => self.cancel(slot),
            Operation::Poll { slot } => self.poll(slot),
            Operation::StartEstablishment { slot } => self.start_establishment(slot),
            Operation::CommitEstablishment { slot } => self.commit_establishment(slot),
            Operation::ReserveDelivery { demand } => self.reserve_delivery(demand),
            Operation::CommitCapacity { crossing } => self.commit_capacity(crossing),
            Operation::CommitBorrowedH1 { crossing } => self.commit_borrowed_h1(crossing),
            Operation::OfferReturnedH1 => {
                self.offer_returned_h1();
                true
            }
            Operation::OfferH2Activation { cutoff } => self.offer_h2_activation(cutoff),
            Operation::SupersedeDemand { demand } => self.supersede_demand(demand),
            Operation::Quiesce => {
                self.quiesce();
                true
            }
        };
        if !applied {
            self.report.skipped += 1;
        }
        self.audit();
    }

    fn slot_index(slot: u8) -> usize {
        (slot % SLOTS) as usize
    }

    /// Resolves a generated slot number to an occupied slot the operation
    /// can act on: the named slot if it qualifies, otherwise the `slot`-th
    /// qualifying slot. Keeps generated sequences productive without making
    /// deterministic sequences ambiguous.
    fn resolve_live(&self, slot: u8, applicable: impl Fn(&Slot) -> bool) -> Option<usize> {
        let index = Self::slot_index(slot);
        if self.slots[index].as_ref().is_some_and(&applicable) {
            return Some(index);
        }
        let candidates: Vec<usize> = self
            .live_slots()
            .filter(|(_, slot)| applicable(slot))
            .map(|(index, _)| index)
            .collect();
        (!candidates.is_empty()).then(|| candidates[index % candidates.len()])
    }

    /// Resolves a generated slot number to an empty slot, as [`Self::resolve_live`].
    fn resolve_empty(&self, slot: u8) -> Option<usize> {
        let index = Self::slot_index(slot);
        if self.slots[index].is_none() {
            return Some(index);
        }
        let candidates: Vec<usize> = (0..self.slots.len())
            .filter(|index| self.slots[*index].is_none())
            .collect();
        (!candidates.is_empty()).then(|| candidates[index % candidates.len()])
    }

    fn live_slots(&self) -> impl Iterator<Item = (usize, &Slot)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|slot| (index, slot)))
    }

    fn slot_waiter(&self, index: usize) -> WaiterId {
        self.slots[index]
            .as_ref()
            .expect("named an empty slot")
            .waiter
    }

    /// The oldest waiting slot, which the queue must treat as the FIFO head.
    fn waiting_head(&self) -> Option<(usize, &Slot)> {
        self.live_slots()
            .filter(|(_, slot)| slot.phase == Phase::Waiting)
            .min_by_key(|(_, slot)| slot.waiter)
    }

    /// The slot a protocol offer must select: the oldest attempt that accepts
    /// the protocol among the waiting head and every indexed attempt.
    fn expected_pick(&self, accepts: fn(ProtocolRequirement) -> bool) -> Option<(usize, WaiterId)> {
        let head = self
            .waiting_head()
            .filter(|(_, slot)| accepts(slot.requirement));
        let indexed = self
            .live_slots()
            .filter(|(_, slot)| slot.is_indexed() && accepts(slot.requirement));
        head.into_iter()
            .chain(indexed)
            .map(|(index, slot)| (index, slot.waiter))
            .min_by_key(|(_, waiter)| *waiter)
    }

    fn expected_route_cutoff(&self) -> Option<WaiterId> {
        self.live_slots()
            .any(|(_, slot)| slot.has_record())
            .then(|| {
                self.last_registered
                    .expect("records exist without a registration")
            })
    }

    /// Consumes a step the queue returned for handling outside the lock.
    fn absorb_step(&mut self, step: AcquisitionStep) {
        match step {
            AcquisitionStep::StartEstablishment(_permit) => {
                assert!(
                    self.permits_in_queue > 0,
                    "queue returned a permit it did not hold"
                );
                self.permits_in_queue -= 1;
            }
            AcquisitionStep::Resolved(outcome) => {
                self.tokens.returned(token_of(&outcome));
            }
        }
    }

    fn absorb_steps(&mut self, steps: [Option<AcquisitionStep>; 2]) {
        for step in steps.into_iter().flatten() {
            self.absorb_step(step);
        }
    }

    /// Checks a returned waker against the slot's poll history and wakes it.
    ///
    /// `expected` is whether the transition made the slot's next poll ready
    /// while a poll was outstanding. The queue must return exactly that
    /// slot's waker then, and no waker otherwise.
    fn settle_waker(&mut self, index: usize, waker: Option<Waker>, expected: bool) {
        let slot = self.slots[index]
            .as_mut()
            .expect("settled waker for empty slot");
        assert_eq!(
            expected,
            waker.is_some(),
            "slot {index} ({:?}) awaiting_wake={} but waker.is_some()={}",
            slot.phase,
            slot.awaiting_wake,
            waker.is_some()
        );
        if let Some(waker) = waker {
            let before = slot.waker.wakes.load(Ordering::SeqCst);
            waker.wake();
            let after = slot.waker.wakes.load(Ordering::SeqCst);
            assert_eq!(
                before + 1,
                after,
                "returned waker did not belong to slot {index}"
            );
            slot.awaiting_wake = false;
            self.report.wakes += 1;
        }
    }

    /// Records that the FIFO head left and returns the snapshots the queue
    /// must have emitted: the retirement and any successor.
    ///
    /// Call after the departing slot's phase has changed so the successor
    /// search finds the next waiting slot.
    fn expect_head_departure(&mut self) -> (DemandSnapshot, Option<DemandSnapshot>) {
        let retired = self.demand.retire();
        let next_head = self
            .waiting_head()
            .map(|(index, slot)| (index, slot.requirement));
        let successor = next_head
            .map(|(index, requirement)| self.demand.begin(index, requirement, &self.group));
        (retired, successor)
    }

    /// Removes a slot whose record is gone, keeping it only while an
    /// establishment task still has to report.
    fn retire_slot(&mut self, index: usize) {
        let slot = self.slots[index].as_mut().expect("retired empty slot");
        if slot.task_outstanding() {
            slot.phase = Phase::Detached;
            slot.awaiting_wake = false;
            slot.pending_token = None;
        } else {
            self.slots[index] = None;
        }
    }

    fn register(&mut self, slot: u8, requirement: Requirement) -> bool {
        let Some(index) = self.resolve_empty(slot) else {
            return false;
        };
        let requirement = requirement.into_protocol();
        let bounded = self.lane.is_bounded();
        let fifo_was_empty = self.waiting_head().is_none();

        let (waiter, snapshot) = self
            .queue
            .register_waiter(requirement, &self.group, bounded);

        assert!(
            self.last_registered.is_none_or(|last| waiter > last),
            "waiter identity {waiter:?} did not exceed {:?}",
            self.last_registered
        );
        self.last_registered = Some(waiter);

        let phase = if bounded {
            Phase::Waiting
        } else {
            self.permits_in_queue += 1;
            Phase::Admitted
        };
        self.slots[index] = Some(Slot::new(waiter, requirement, phase));

        let expected =
            (bounded && fifo_was_empty).then(|| self.demand.begin(index, requirement, &self.group));
        assert_eq!(expected, snapshot, "registration snapshot");
        self.report.registrations += 1;
        true
    }

    fn cancel(&mut self, slot: u8) -> bool {
        let Some(index) = self.resolve_live(slot, |_| true) else {
            return false;
        };
        let current = self.slots[index].as_ref().expect("resolved slot is live");
        let waiter = current.waiter;
        let phase = current.phase;

        match phase {
            Phase::CancelledReserved | Phase::Detached => {
                let repeated = self.queue.cancel_waiter(waiter, &self.group);
                assert!(repeated.is_none(), "second cancellation returned work");
                self.report.cancel_repeated += 1;
            }
            Phase::Waiting => {
                let is_head = self.waiting_head().is_some_and(|(head, _)| head == index);
                let cancelled = self
                    .queue
                    .cancel_waiter(waiter, &self.group)
                    .expect("waiting attempt cancels");
                // Change phase before computing the successor.
                self.slots[index] = None;
                let expected_updates = if is_head {
                    let (retired, successor) = self.expect_head_departure();
                    self.report.cancel_waiting_head += 1;
                    [Some(retired), successor]
                } else {
                    self.report.cancel_waiting_other += 1;
                    [None, None]
                };
                assert_eq!(
                    expected_updates, cancelled.demand_updates,
                    "cancel demand updates"
                );
                assert!(
                    cancelled.returned_steps.iter().all(Option::is_none),
                    "waiting cancel returned steps"
                );
            }
            Phase::Reserved | Phase::ReservedWithPending => {
                let cancelled = self
                    .queue
                    .cancel_waiter(waiter, &self.group)
                    .expect("reserved attempt cancels");
                assert!(cancelled.demand_updates.iter().all(Option::is_none));
                assert!(cancelled.returned_steps.iter().all(Option::is_none));
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::CancelledReserved;
                // awaiting_wake and pending_token carry into the marker.
                self.report.cancel_reserved += 1;
            }
            Phase::Admitted => {
                let cancelled = self
                    .queue
                    .cancel_waiter(waiter, &self.group)
                    .expect("admitted attempt cancels");
                assert!(cancelled.demand_updates.iter().all(Option::is_none));
                let [first, second] = cancelled.returned_steps;
                assert!(
                    matches!(first, Some(AcquisitionStep::StartEstablishment(_))),
                    "admitted cancel did not return its permit"
                );
                assert!(second.is_none());
                self.absorb_steps([first, None]);
                self.retire_slot(index);
                self.report.cancel_admitted += 1;
            }
            Phase::HasResult => {
                let cancelled = self
                    .queue
                    .cancel_waiter(waiter, &self.group)
                    .expect("ready attempt cancels");
                assert!(cancelled.demand_updates.iter().all(Option::is_none));
                let [first, second] = cancelled.returned_steps;
                assert!(
                    matches!(first, Some(AcquisitionStep::Resolved(_))),
                    "ready cancel did not return its outcome"
                );
                assert!(second.is_none());
                self.absorb_steps([first, None]);
                self.retire_slot(index);
                self.report.cancel_ready += 1;
            }
            Phase::LaunchSubmitted | Phase::LaunchStarted => {
                let cancelled = self
                    .queue
                    .cancel_waiter(waiter, &self.group)
                    .expect("launching attempt cancels");
                assert!(cancelled.demand_updates.iter().all(Option::is_none));
                assert!(cancelled.returned_steps.iter().all(Option::is_none));
                self.retire_slot(index);
                self.report.cancel_launching += 1;
            }
        }
        true
    }

    fn poll(&mut self, slot: u8) -> bool {
        // Polling a cancelled or consumed attempt is a caller-contract panic.
        let Some(index) = self.resolve_live(slot, |slot| {
            !matches!(slot.phase, Phase::CancelledReserved | Phase::Detached)
        }) else {
            return false;
        };
        self.poll_slot(index);
        true
    }

    fn poll_slot(&mut self, index: usize) {
        let current = self.slots[index].as_ref().expect("polled empty slot");
        let waiter = current.waiter;
        let phase = current.phase;
        let waker = Waker::from(current.waker.clone());
        let mut cx = Context::from_waker(&waker);

        let polled = self.queue.poll_waiter(waiter, &mut cx);
        match (phase, polled) {
            (Phase::Admitted, Poll::Ready(step @ AcquisitionStep::StartEstablishment(_))) => {
                self.absorb_step(step);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::LaunchSubmitted;
                current.launch = Launch::Submitted;
                current.awaiting_wake = false;
                self.report.polls_start += 1;
            }
            (Phase::HasResult, Poll::Ready(AcquisitionStep::Resolved(outcome))) => {
                self.tokens.delivered(token_of(&outcome));
                let current = self.slots[index].as_mut().expect("slot present");
                current.awaiting_wake = false;
                self.retire_slot(index);
                self.report.polls_resolved += 1;
            }
            (
                Phase::Waiting
                | Phase::Reserved
                | Phase::ReservedWithPending
                | Phase::LaunchSubmitted
                | Phase::LaunchStarted,
                Poll::Pending,
            ) => {
                let current = self.slots[index].as_mut().expect("slot present");
                current.awaiting_wake = true;
                self.report.polls_pending += 1;
            }
            (phase, polled) => panic!("slot {index} in {phase:?} polled {polled:?}"),
        }
    }

    fn start_establishment(&mut self, slot: u8) -> bool {
        let Some(index) = self.resolve_live(slot, |slot| slot.launch == Launch::Submitted) else {
            return false;
        };
        let current = self.slots[index].as_ref().expect("resolved slot is live");
        let waiter = current.waiter;
        let expected = current.phase == Phase::LaunchSubmitted;

        let started = self.queue.start_establishment(waiter);
        assert_eq!(expected, started, "start_establishment for slot {index}");

        let current = self.slots[index].as_mut().expect("slot present");
        current.launch = Launch::Started;
        if started {
            current.phase = Phase::LaunchStarted;
            self.report.start_true += 1;
        } else {
            self.report.start_false += 1;
        }
        true
    }

    fn commit_establishment(&mut self, slot: u8) -> bool {
        // Prefer a task that has started, as production tasks normally do;
        // an unstarted task reports only when no started one is outstanding,
        // which keeps the submitted phase alive long enough for offers,
        // cancellation, and start to reach it.
        let Some(index) = self
            .resolve_live(slot, |slot| slot.launch == Launch::Started)
            .or_else(|| self.resolve_live(slot, Slot::task_outstanding))
        else {
            return false;
        };
        self.commit_establishment_slot(index);
        true
    }

    fn commit_establishment_slot(&mut self, index: usize) {
        let current = self.slots[index].as_ref().expect("committed empty slot");
        let waiter = current.waiter;
        let phase = current.phase;
        let launch = current.launch;
        let awaiting = current.awaiting_wake;

        let token = self.tokens.allocate();
        self.tokens.inject(token);
        let outcome = self
            .queue
            .commit_establishment(waiter, tagged_outcome(token));

        match (phase, outcome) {
            (
                Phase::LaunchSubmitted | Phase::LaunchStarted,
                CellCommitOutcome::Committed { waker },
            ) => {
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                current.launch = Launch::Finished;
                match launch {
                    Launch::Submitted => self.report.establish_committed_submitted += 1,
                    Launch::Started => self.report.establish_committed_started += 1,
                    Launch::None | Launch::Finished => unreachable!("task was outstanding"),
                }
            }
            (
                Phase::HasResult | Phase::Detached,
                CellCommitOutcome::Refused { returned, waker },
            ) => {
                assert!(waker.is_none(), "refused establishment carried a waker");
                let [first, second] = returned;
                match &first {
                    Some(AcquisitionStep::Resolved(outcome)) => {
                        assert_eq!(token, token_of(outcome), "wrong outcome returned")
                    }
                    other => panic!("refused establishment returned {other:?}"),
                }
                assert!(second.is_none());
                self.absorb_steps([first, None]);
                let current = self.slots[index].as_mut().expect("slot present");
                current.launch = Launch::Finished;
                if phase == Phase::Detached {
                    self.slots[index] = None;
                }
                self.report.establish_refused += 1;
            }
            (phase, CellCommitOutcome::Committed { .. }) => {
                panic!("establishment committed to slot {index} in {phase:?}")
            }
            (phase, CellCommitOutcome::Refused { .. }) => {
                panic!("establishment refused for slot {index} in {phase:?}")
            }
            (_, CellCommitOutcome::Invalid { error, .. }) => {
                panic!("establishment commit reported invalid state: {error:?}")
            }
        }
        self.tokens.settle();
    }

    fn reserve_delivery(&mut self, demand: DemandSelector) -> bool {
        if !self.lane.is_bounded() || self.crossings.len() >= CROSSINGS {
            return false;
        }
        let id = match demand {
            DemandSelector::Current => match &self.demand.active {
                Some(active) => active.id,
                None => return false,
            },
            DemandSelector::Stale => self.demand.stale_id(),
        };

        let reservation = self.queue.reserve_delivery_waiter(id, &self.group);
        match demand {
            DemandSelector::Stale => {
                assert!(
                    matches!(reservation, DeliveryReservation::Rejected),
                    "stale demand {id:?} was reserved"
                );
                self.report.reservations_rejected += 1;
            }
            DemandSelector::Current => {
                let DeliveryReservation::Reserved { waiter, successor } = reservation else {
                    panic!("current demand {id:?} was rejected");
                };
                self.reserve_head(waiter, successor);
                self.report.reservations += 1;
            }
        }
        true
    }

    /// Records a reservation the queue accepted for the current head.
    fn reserve_head(&mut self, waiter: WaiterId, successor: Option<DemandSnapshot>) {
        let (index, head) = self.waiting_head().expect("reserved without a head");
        assert_eq!(head.waiter, waiter, "reservation did not name the head");
        let current = self.slots[index].as_mut().expect("slot present");
        current.phase = Phase::Reserved;
        let (_retired, expected_successor) = self.expect_head_departure();
        // Admission retires the reserved generation itself; the queue emits
        // only the successor here.
        assert_eq!(expected_successor, successor, "reservation successor");
        self.crossings.push(Crossing { slot: index });
    }

    fn commit_capacity(&mut self, crossing: u8) -> bool {
        if self.crossings.is_empty() {
            return false;
        }
        let position = crossing as usize % self.crossings.len();
        let Crossing { slot } = self.crossings.remove(position);
        self.commit_capacity_slot(slot);
        true
    }

    fn commit_capacity_slot(&mut self, index: usize) {
        let current = self.slots[index]
            .as_ref()
            .expect("crossing named an empty slot");
        let waiter = current.waiter;
        let phase = current.phase;
        let awaiting = current.awaiting_wake;
        let pending = current.pending_token;

        self.permits_in_queue += 1;
        let outcome = self
            .queue
            .commit_capacity(waiter, EstablishmentPermit::unbounded());

        match (phase, outcome) {
            (Phase::Reserved, CellCommitOutcome::Committed { waker }) => {
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::Admitted;
                self.report.capacity_committed += 1;
            }
            (Phase::ReservedWithPending, CellCommitOutcome::Refused { returned, waker }) => {
                let [first, second] = returned;
                assert!(
                    matches!(first, Some(AcquisitionStep::StartEstablishment(_))),
                    "refused capacity did not return the permit"
                );
                assert!(second.is_none());
                self.absorb_steps([first, None]);
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                current.pending_token = None;
                self.report.capacity_refused_pending += 1;
            }
            (Phase::CancelledReserved, CellCommitOutcome::Refused { returned, waker }) => {
                let [first, second] = returned;
                match (&pending, &first) {
                    (Some(token), Some(AcquisitionStep::Resolved(outcome))) => {
                        assert_eq!(*token, token_of(outcome), "wrong pending result returned")
                    }
                    (None, None) => {}
                    (pending, first) => {
                        panic!("cancelled commit returned {first:?} for pending {pending:?}")
                    }
                }
                assert!(
                    matches!(second, Some(AcquisitionStep::StartEstablishment(_))),
                    "cancelled capacity commit did not return the permit"
                );
                self.absorb_steps([first, second]);
                self.settle_waker(index, waker, awaiting);
                self.slots[index] = None;
                self.report.capacity_refused_cancelled += 1;
            }
            (phase, CellCommitOutcome::Committed { .. }) => {
                panic!("capacity committed to slot {index} in {phase:?}")
            }
            (phase, CellCommitOutcome::Refused { .. }) => {
                panic!("capacity refused for slot {index} in {phase:?}")
            }
            (_, CellCommitOutcome::Invalid { error, .. }) => {
                panic!("capacity commit reported invalid state: {error:?}")
            }
        }
    }

    fn commit_borrowed_h1(&mut self, crossing: u8) -> bool {
        if self.crossings.is_empty() {
            return false;
        }
        let position = crossing as usize % self.crossings.len();
        let Crossing { slot: index } = self.crossings.remove(position);

        let current = self.slots[index]
            .as_ref()
            .expect("crossing named an empty slot");
        let waiter = current.waiter;
        let phase = current.phase;
        let awaiting = current.awaiting_wake;
        let pending = current.pending_token;

        let token = self.tokens.allocate();
        self.tokens.inject(token);
        let outcome = self.queue.commit_borrowed_h1(waiter, tagged_outcome(token));

        match (phase, outcome) {
            (Phase::Reserved, CellCommitOutcome::Committed { waker }) => {
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                self.report.borrowed_committed += 1;
            }
            (Phase::ReservedWithPending, CellCommitOutcome::Refused { returned, waker }) => {
                let [first, second] = returned;
                match &first {
                    Some(AcquisitionStep::Resolved(outcome)) => {
                        assert_eq!(token, token_of(outcome), "borrowed result not returned")
                    }
                    other => panic!("refused borrowed commit returned {other:?}"),
                }
                assert!(second.is_none());
                self.absorb_steps([first, None]);
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                current.pending_token = None;
                self.report.borrowed_refused_pending += 1;
            }
            (Phase::CancelledReserved, CellCommitOutcome::Refused { returned, waker }) => {
                let [first, second] = returned;
                match (&pending, &first) {
                    (Some(token), Some(AcquisitionStep::Resolved(outcome))) => {
                        assert_eq!(*token, token_of(outcome), "wrong pending result returned")
                    }
                    (None, None) => {}
                    (pending, first) => {
                        panic!("cancelled commit returned {first:?} for pending {pending:?}")
                    }
                }
                match &second {
                    Some(AcquisitionStep::Resolved(outcome)) => {
                        assert_eq!(token, token_of(outcome), "borrowed result not returned")
                    }
                    other => panic!("cancelled borrowed commit returned {other:?}"),
                }
                self.absorb_steps([first, second]);
                self.settle_waker(index, waker, awaiting);
                self.slots[index] = None;
                self.report.borrowed_refused_cancelled += 1;
            }
            (phase, CellCommitOutcome::Committed { .. }) => {
                panic!("borrowed sender committed to slot {index} in {phase:?}")
            }
            (phase, CellCommitOutcome::Refused { .. }) => {
                panic!("borrowed sender refused for slot {index} in {phase:?}")
            }
            (_, CellCommitOutcome::Invalid { error, .. }) => {
                panic!("borrowed commit reported invalid state: {error:?}")
            }
        }
        self.tokens.settle();
        true
    }

    fn offer_returned_h1(&mut self) {
        let expected = self.expected_pick(ProtocolRequirement::accepts_h1);
        let token = self.tokens.allocate();
        self.tokens.inject(token);

        let (waiter, resolution) = self
            .queue
            .offer_returned_h1(|| tagged_outcome(token), &self.group);

        assert_eq!(
            expected.map(|(_, waiter)| waiter),
            waiter,
            "returned H1 went to the wrong attempt"
        );
        self.apply_resolution(
            Offer::H1,
            expected.map(|(index, _)| index),
            token,
            resolution,
        );
        self.tokens.settle();
    }

    fn offer_h2_activation(&mut self, cutoff: CutoffSelector) -> bool {
        let cutoff = match cutoff {
            CutoffSelector::None => None,
            CutoffSelector::RouteCutoff => {
                let cutoff = self.queue.route_cutoff();
                assert_eq!(self.expected_route_cutoff(), cutoff, "route cutoff");
                cutoff
            }
            CutoffSelector::Slot(slot) => self.slots[Self::slot_index(slot)]
                .as_ref()
                .map(|slot| slot.waiter),
        };
        let expected = self.expected_pick(ProtocolRequirement::accepts_h2);

        // Production offers through a cutoff only after checking that a
        // compatible attempt at or before it exists; otherwise the gate opens
        // or the offer waits. Mirror that guard so the queue's own deferral
        // branch stays unreachable here as it is in production.
        if let Some(cutoff) = cutoff {
            let through = expected.is_some_and(|(_, waiter)| waiter <= cutoff);
            assert_eq!(
                through,
                self.queue.has_h2_compatible_waiter_through(cutoff),
                "H2 waiter through {cutoff:?}"
            );
            if !through {
                return false;
            }
        }
        let token = self.tokens.allocate();

        let (waiter, resolution) =
            self.queue
                .offer_h2_activation(cutoff, move |_| tagged_outcome(token), &self.group);

        assert_eq!(
            expected.map(|(_, waiter)| waiter),
            waiter,
            "H2 activation went to the wrong attempt (cutoff {cutoff:?})"
        );
        if waiter.is_some() {
            self.tokens.inject(token);
        } else {
            self.report.h2_none += 1;
        }
        self.apply_resolution(
            Offer::H2,
            expected.map(|(index, _)| index),
            token,
            resolution,
        );
        self.tokens.settle();
        true
    }

    /// Applies what a protocol offer returned to the model of the selected slot.
    fn apply_resolution(
        &mut self,
        offer: Offer,
        selected: Option<usize>,
        token: u64,
        resolution: WaiterResolution,
    ) {
        let WaiterResolution {
            demand_updates,
            returned_step,
            waker,
        } = resolution;

        let Some(index) = selected else {
            assert!(demand_updates.iter().all(Option::is_none));
            assert!(waker.is_none());
            match (offer, returned_step) {
                (Offer::H1, Some(AcquisitionStep::Resolved(outcome))) => {
                    assert_eq!(
                        token,
                        token_of(&outcome),
                        "refused H1 returned wrong outcome"
                    );
                    self.tokens.returned(token);
                    self.report.h1_refused += 1;
                }
                (Offer::H2, None) => {
                    // Counted by the caller as absent.
                }
                (offer, step) => panic!("unselected {offer:?} offer returned {step:?}"),
            }
            return;
        };

        let current = self.slots[index].as_ref().expect("selected empty slot");
        let phase = current.phase;
        let awaiting = current.awaiting_wake;
        match phase {
            Phase::Waiting => {
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                let (retired, successor) = self.expect_head_departure();
                assert_eq!(
                    [Some(retired), successor],
                    demand_updates,
                    "offer demand updates"
                );
                assert!(
                    returned_step.is_none(),
                    "offer to waiting head returned a step"
                );
                self.settle_waker(index, waker, awaiting);
                match offer {
                    Offer::H1 => self.report.h1_to_waiting += 1,
                    Offer::H2 => self.report.h2_to_waiting += 1,
                }
            }
            Phase::Reserved => {
                assert!(demand_updates.iter().all(Option::is_none));
                assert!(returned_step.is_none());
                // The result waits for the crossing; the poll stays pending.
                self.settle_waker(index, waker, false);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::ReservedWithPending;
                current.pending_token = Some(token);
                match offer {
                    Offer::H1 => self.report.h1_to_reserved += 1,
                    Offer::H2 => self.report.h2_to_reserved += 1,
                }
            }
            Phase::Admitted => {
                assert!(demand_updates.iter().all(Option::is_none));
                assert!(
                    matches!(returned_step, Some(AcquisitionStep::StartEstablishment(_))),
                    "offer to admitted attempt did not return its permit"
                );
                self.absorb_steps([returned_step, None]);
                assert!(!awaiting, "admitted attempt had an outstanding poll");
                self.settle_waker(index, waker, false);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                match offer {
                    Offer::H1 => self.report.h1_to_admitted += 1,
                    Offer::H2 => self.report.h2_to_admitted += 1,
                }
            }
            Phase::LaunchSubmitted | Phase::LaunchStarted => {
                assert!(demand_updates.iter().all(Option::is_none));
                assert!(returned_step.is_none());
                self.settle_waker(index, waker, awaiting);
                let current = self.slots[index].as_mut().expect("slot present");
                current.phase = Phase::HasResult;
                match offer {
                    Offer::H1 => self.report.h1_to_launching += 1,
                    Offer::H2 => self.report.h2_to_launching += 1,
                }
            }
            phase => panic!("offer selected slot {index} in {phase:?}"),
        }
    }

    fn supersede_demand(&mut self, demand: DemandSelector) -> bool {
        if !self.lane.is_bounded() {
            return false;
        }
        let id = match demand {
            DemandSelector::Current => match &self.demand.active {
                Some(active) => active.id,
                None => return false,
            },
            DemandSelector::Stale => self.demand.stale_id(),
        };

        let superseded = self.queue.supersede_demand_snapshot(id);
        match demand {
            DemandSelector::Current => {
                assert!(superseded, "current demand {id:?} was not superseded");
                let active = self.demand.active.as_mut().expect("active demand");
                active.version = active.version.next().next();
                self.report.supersede_true += 1;
            }
            DemandSelector::Stale => {
                assert!(!superseded, "stale demand {id:?} was superseded");
                self.report.supersede_false += 1;
            }
        }
        true
    }

    /// Drives every live attempt to a delivered outcome, then asserts the
    /// queue holds nothing.
    ///
    /// Order follows production: open crossings commit, the FIFO drains
    /// through deliveries, outstanding establishment tasks report, and each
    /// remaining attempt is polled to completion.
    pub(in crate::client::pool::cell::waiters) fn quiesce(&mut self) {
        fn count_step(steps: &mut usize) {
            *steps += 1;
            assert!(
                *steps <= QUIESCE_STEP_LIMIT,
                "quiesce exceeded {QUIESCE_STEP_LIMIT} steps"
            );
        }

        self.report.quiesce_runs += 1;
        let mut steps = 0usize;

        while let Some(Crossing { slot }) = self.crossings.pop() {
            count_step(&mut steps);
            self.commit_capacity_slot(slot);
            self.audit();
        }

        while let Some(id) = self.demand.active.as_ref().map(|active| active.id) {
            count_step(&mut steps);
            let DeliveryReservation::Reserved { waiter, successor } =
                self.queue.reserve_delivery_waiter(id, &self.group)
            else {
                panic!("quiesce could not reserve the current head {id:?}");
            };
            self.reserve_head(waiter, successor);
            let Crossing { slot } = self.crossings.pop().expect("reservation opened a crossing");
            self.commit_capacity_slot(slot);
            self.audit();
        }

        for index in 0..self.slots.len() {
            if self.slots[index]
                .as_ref()
                .is_some_and(Slot::task_outstanding)
            {
                count_step(&mut steps);
                self.commit_establishment_slot(index);
                self.audit();
            }
        }

        for index in 0..self.slots.len() {
            while let Some((phase, launch)) = self.slots[index]
                .as_ref()
                .map(|slot| (slot.phase, slot.launch))
            {
                count_step(&mut steps);
                match (phase, launch) {
                    (Phase::Admitted | Phase::HasResult, _) => self.poll_slot(index),
                    // Alternate the two orders an establishment task can
                    // report in: started first, as tasks normally are, or
                    // straight from submission, as a task dropped unpolled.
                    (Phase::LaunchSubmitted, Launch::Submitted) if index % 2 == 0 => {
                        assert!(self.queue.start_establishment(self.slot_waiter(index)));
                        let current = self.slots[index].as_mut().expect("slot present");
                        current.phase = Phase::LaunchStarted;
                        current.launch = Launch::Started;
                        self.report.start_true += 1;
                    }
                    (Phase::LaunchSubmitted | Phase::LaunchStarted, _) => {
                        self.commit_establishment_slot(index)
                    }
                    (phase, launch) => {
                        panic!("quiesce left slot {index} in {phase:?} with {launch:?}")
                    }
                }
                self.audit();
            }
        }

        assert!(
            self.queue.records.is_empty(),
            "quiesced queue retained records"
        );
        assert!(
            matches!(self.queue.waiting, WaitingQueueState::Empty),
            "quiesced queue retained a FIFO"
        );
        assert!(self.queue.h1_compatible_waiters.is_empty());
        assert!(self.queue.h2_compatible_waiters.is_empty());
        assert_eq!(0, self.queue.pending_count());
        assert_eq!(0, self.permits_in_queue, "permits remained after quiesce");
        assert!(
            self.tokens.state_owned().is_empty(),
            "outcomes remained after quiesce"
        );
        assert!(self.demand.active.is_none());
        self.report.quiesce_steps += steps;
    }

    /// Compares the harness model with the queue after every operation.
    fn audit(&self) {
        self.queue.assert_consistent();
        assert!(
            !self.tokens.any_injected(),
            "operation left a token with unknown residence"
        );

        // Every record is a live slot and every live slot is a record.
        let live: BTreeMap<WaiterId, (usize, &Slot)> = self
            .live_slots()
            .filter(|(_, slot)| slot.has_record())
            .map(|(index, slot)| (slot.waiter, (index, slot)))
            .collect();
        assert_eq!(
            live.len(),
            self.queue.records.len(),
            "record count: model {:?} vs queue {:?}",
            live.keys().collect::<Vec<_>>(),
            self.queue.records.keys().collect::<BTreeSet<_>>()
        );
        for (waiter, (index, slot)) in &live {
            let record = self
                .queue
                .records
                .get(waiter)
                .unwrap_or_else(|| panic!("slot {index} has no record"));
            assert_eq!(
                slot.requirement, record.requirement,
                "slot {index} requirement"
            );
            let agrees = match (slot.phase, &record.state) {
                (Phase::Waiting, WaiterState::Waiting { .. }) => true,
                (
                    Phase::Reserved,
                    WaiterState::DeliveryPending {
                        pending_result: None,
                        ..
                    },
                ) => true,
                (
                    Phase::ReservedWithPending,
                    WaiterState::DeliveryPending {
                        pending_result: Some(outcome),
                        ..
                    },
                ) => slot.pending_token == Some(token_of(outcome)),
                (
                    Phase::CancelledReserved,
                    WaiterState::DeliveryCancelled { pending_result, .. },
                ) => slot.pending_token == pending_result.as_ref().map(token_of),
                (Phase::Admitted, WaiterState::ReadyToEstablish { .. }) => true,
                (
                    Phase::LaunchSubmitted,
                    WaiterState::Launching {
                        phase: EstablishmentPhase::Submitted,
                        ..
                    },
                ) => true,
                (
                    Phase::LaunchStarted,
                    WaiterState::Launching {
                        phase: EstablishmentPhase::Started,
                        ..
                    },
                ) => true,
                (Phase::HasResult, WaiterState::Ready(_)) => true,
                _ => false,
            };
            assert!(
                agrees,
                "slot {index} model {:?} (pending {:?}) disagrees with record {:?}",
                slot.phase, slot.pending_token, record.state
            );
        }

        // FIFO order is arrival order over the waiting slots.
        let expected_order: Vec<WaiterId> = {
            let mut order: Vec<_> = self
                .live_slots()
                .filter(|(_, slot)| slot.phase == Phase::Waiting)
                .map(|(_, slot)| slot.waiter)
                .collect();
            order.sort();
            order
        };
        let actual_order: Vec<WaiterId> = {
            let mut order = Vec::new();
            let mut current = match &self.queue.waiting {
                WaitingQueueState::Empty => None,
                WaitingQueueState::Active { head, .. } => Some(*head),
            };
            while let Some(waiter) = current {
                order.push(waiter);
                current = match &self.queue.records[&waiter].state {
                    WaiterState::Waiting { next, .. } => *next,
                    other => panic!("linked waiter {waiter:?} in {other:?}"),
                };
                assert!(order.len() <= self.queue.records.len(), "FIFO cycle");
            }
            order
        };
        assert_eq!(expected_order, actual_order, "FIFO order");

        // The active demand describes the model's head.
        match (&self.queue.waiting, &self.demand.active) {
            (WaitingQueueState::Empty, None) => {}
            (WaitingQueueState::Active { head, demand, .. }, Some(active)) => {
                let head_slot = self.slots[active.head]
                    .as_ref()
                    .expect("model head slot is empty");
                assert_eq!(head_slot.waiter, *head, "FIFO head");
                assert_eq!(active.id, demand.id, "demand id");
                assert_eq!(active.version, demand.version, "demand version");
                assert_eq!(active.requirement, demand.requirement, "demand requirement");
            }
            (waiting, active) => panic!("FIFO {waiting:?} vs model demand {active:?}"),
        }
        assert_eq!(
            self.demand.current_snapshot(&self.group),
            self.queue.current_demand_snapshot(&self.group),
            "current demand snapshot"
        );

        // Every outcome the queue holds is one the ledger says it holds.
        let held: BTreeSet<u64> = self
            .queue
            .records
            .values()
            .filter_map(|record| match &record.state {
                WaiterState::Ready(outcome) => Some(token_of(outcome)),
                WaiterState::DeliveryPending { pending_result, .. }
                | WaiterState::DeliveryCancelled { pending_result, .. } => {
                    pending_result.as_ref().map(token_of)
                }
                WaiterState::Waiting { .. }
                | WaiterState::ReadyToEstablish { .. }
                | WaiterState::Launching { .. } => None,
            })
            .collect();
        assert_eq!(self.tokens.state_owned(), held, "state-owned outcomes");

        // Permits are conserved by count.
        let ready_to_establish = self
            .queue
            .records
            .values()
            .filter(|record| matches!(record.state, WaiterState::ReadyToEstablish { .. }))
            .count();
        assert_eq!(self.permits_in_queue, ready_to_establish, "permits held");

        // Read-only queries agree with the model.
        let h1 = self.expected_pick(ProtocolRequirement::accepts_h1);
        let h2 = self.expected_pick(ProtocolRequirement::accepts_h2);
        assert_eq!(
            h1.is_some(),
            self.queue.has_h1_compatible_waiter(),
            "has H1 waiter"
        );
        assert_eq!(
            h1.map(|(_, waiter)| waiter),
            self.queue.oldest_h1_compatible_waiter(),
            "oldest H1 waiter"
        );
        assert_eq!(
            self.live_slots()
                .any(|(_, slot)| slot.is_indexed() && slot.requirement.accepts_h1()),
            self.queue.has_prior_h1_waiter(),
            "prior H1 waiter"
        );
        assert_eq!(
            h2.map(|(_, waiter)| waiter),
            self.queue.oldest_h2_compatible_waiter(),
            "oldest H2 waiter"
        );
        assert_eq!(
            h2.is_some(),
            self.queue.has_h2_compatible_waiter(),
            "has H2 waiter"
        );
        assert_eq!(
            self.live_slots()
                .filter(|(_, slot)| slot.is_pending())
                .count(),
            self.queue.pending_count(),
            "pending count"
        );
        assert_eq!(
            self.expected_route_cutoff(),
            self.queue.route_cutoff(),
            "route cutoff"
        );
    }
}

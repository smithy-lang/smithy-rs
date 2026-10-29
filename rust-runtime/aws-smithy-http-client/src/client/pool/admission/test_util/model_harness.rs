/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Applies operation sequences to production admission state.
//!
//! The harness owns one [`AdmissionState`] and stands in for every cell.
//! Prepared crossings that production hands to a detached action are held in
//! harness slots until a later operation settles them the way the action
//! would. After each operation the harness checks the relationships that no
//! single sub-state can see: permit conservation across admission and held
//! slots, assignment ownership, and match ownership.
//!
//! [`Harness::quiesce`] drains admission using the production action order
//! and then asserts that "nothing to do" is true of the raw state. That is the
//! liveness check: a readiness index that drifted from demand and supply shows
//! up as an assertion instead of a request that never completes.

use super::super::demand::DemandState;
use super::super::h1::{
    H1MatchAudit, H1MatchId, H1MatchKind, H1MatchPhase, H1SupplyOutcome, H1SupplyStatus,
    PreparedH1Match,
};
use super::super::h2::{H2SupplyStatus, PreparedH2Reclaim, PreparedH2Route};
use super::super::{
    AdmissionState, CapacityPermit, DemandAssignment, DemandAssignmentOutcome, DemandId,
    DemandSnapshot, ProtocolRequirement, SnapshotVersion, SupplyRevision,
};
use super::operation_sequence::{
    Operation, Outcome, ProbeReply, Profile, Requirement, ReservationReply,
};
use crate::client::pool::cell::h2::H2GenerationId;
use crate::client::pool::partition::PartitionId;

const PERMIT_SLOTS: usize = 4;
const ASSIGNMENT_SLOTS: usize = 4;
const MATCH_SLOTS: usize = 4;
/// Upper bound on quiesce iterations before the harness reports a livelock.
const QUIESCE_STEP_LIMIT: usize = 4_096;

/// Resource a held assignment will deliver.
#[derive(Debug)]
enum AssignmentSource {
    /// Permit removed from admission for a new establishment.
    Capacity(CapacityPermit),
    /// Sender borrowed through a retained HTTP/1 match.
    BorrowedH1 {
        match_id: H1MatchId,
        supplier: PartitionId,
    },
    /// Route to a peer HTTP/2 generation.
    H2Route(PreparedH2Route),
}

/// One assignment crossing to a requesting cell.
#[derive(Debug)]
struct HeldAssignment {
    assignment: DemandAssignment,
    source: AssignmentSource,
}

/// Per-partition monotonic identities the cell would allocate.
#[derive(Debug, Default)]
struct PartitionCounters {
    next_demand: u64,
    next_h1_revision: u64,
    next_h2_revision: u64,
    next_generation: u64,
    current_generation: Option<H2GenerationId>,
}

/// Counts of what one sequence exercised, for coverage reporting.
#[derive(Debug, Default, Eq, PartialEq)]
pub(in crate::client::pool::admission) struct SequenceReport {
    pub(in crate::client::pool::admission) operations: usize,
    pub(in crate::client::pool::admission) capacity_deliveries: usize,
    pub(in crate::client::pool::admission) h1_borrows: usize,
    pub(in crate::client::pool::admission) h1_reclaims: usize,
    pub(in crate::client::pool::admission) h2_routes: usize,
    pub(in crate::client::pool::admission) h2_reclaims: usize,
    pub(in crate::client::pool::admission) quiesce_steps: usize,
}

/// Production admission state plus every handle a cell would be holding.
pub(in crate::client::pool::admission) struct Harness {
    profile: Profile,
    state: AdmissionState,
    permits: [Option<CapacityPermit>; PERMIT_SLOTS],
    assignments: [Option<HeldAssignment>; ASSIGNMENT_SLOTS],
    matches: [Option<H1MatchId>; MATCH_SLOTS],
    h2_reclaim: Option<PreparedH2Reclaim>,
    counters: Vec<PartitionCounters>,
    report: SequenceReport,
}

/// Runs one sequence and returns what it exercised.
pub(in crate::client::pool::admission) fn run_sequence(
    profile: Profile,
    operations: &[Operation],
) -> SequenceReport {
    let mut harness = Harness::new(profile);
    for operation in operations {
        harness.apply(operation);
    }
    harness.quiesce();
    harness.finish()
}

impl Harness {
    pub(in crate::client::pool::admission) fn new(profile: Profile) -> Self {
        assert!(
            profile.limit().get() <= PERMIT_SLOTS,
            "permit slots must hold every permit the profile can issue"
        );
        Self {
            profile,
            state: AdmissionState::new(profile.limit()),
            permits: Default::default(),
            assignments: Default::default(),
            matches: Default::default(),
            h2_reclaim: None,
            counters: (0..profile.partitions())
                .map(|_| PartitionCounters::default())
                .collect(),
            report: SequenceReport::default(),
        }
    }

    /// Applies one operation and audits the result.
    pub(in crate::client::pool::admission) fn apply(&mut self, operation: &Operation) {
        self.report.operations += 1;
        match operation.clone() {
            Operation::PublishDemand {
                partition,
                requirement,
            } => self.publish_demand(partition, requirement),
            Operation::BumpDemandVersion { partition } => self.bump_demand_version(partition),
            Operation::CancelDemand { partition } => self.cancel_demand(partition),
            Operation::TakePermit => self.take_permit(),
            Operation::ReturnPermit { slot } => self.return_permit(slot),
            Operation::PrepareCapacityDelivery => self.prepare_capacity_delivery(),
            Operation::SettleAssignment {
                slot,
                outcome,
                successor,
                stale_route,
            } => self.settle_assignment(slot, outcome, successor, stale_route),
            Operation::PublishH1Supply {
                partition,
                returnable,
                blocked,
            } => self.publish_h1_supply(partition, returnable, blocked),
            Operation::PrepareH1Match => self.prepare_h1_match(),
            Operation::H1ProbeReply { slot, reply } => self.h1_probe_reply(slot, reply),
            Operation::H1ReservationReply { slot, reply } => self.h1_reservation_reply(slot, reply),
            Operation::H1SenderReturned { slot, accepted } => {
                self.h1_sender_returned(slot, accepted)
            }
            Operation::H1CancelStep => self.h1_cancel_step(),
            Operation::PublishH2Supply {
                partition,
                fresh_generation,
                idle,
            } => self.publish_h2_supply(partition, fresh_generation, idle),
            Operation::PublishH2Unavailable { partition } => self.publish_h2_unavailable(partition),
            Operation::PrepareH2Route => self.prepare_h2_route(),
            Operation::PrepareH2Reclaim => self.prepare_h2_reclaim(),
            Operation::SettleH2Reclaim { closed } => self.settle_h2_reclaim(closed),
            Operation::Quiesce => self.quiesce(),
        }
        self.audit();
    }

    /// Returns the coverage report after the final audit.
    pub(in crate::client::pool::admission) fn finish(self) -> SequenceReport {
        self.audit();
        self.report
    }

    // Identity helpers.

    fn partition(&self, selector: u8) -> PartitionId {
        self.profile.partition(selector)
    }

    fn counters(&mut self, partition: PartitionId) -> &mut PartitionCounters {
        let index = self
            .profile
            .all_partitions()
            .iter()
            .position(|candidate| *candidate == partition)
            .expect("partition belongs to the profile");
        &mut self.counters[index]
    }

    fn next_demand_id(&mut self, partition: PartitionId) -> DemandId {
        let counters = self.counters(partition);
        counters.next_demand += 1;
        DemandId::from_u64(counters.next_demand)
    }

    fn h1_live(&mut self, supplier: PartitionId, returnable: bool) -> H1SupplyOutcome {
        let counters = self.counters(supplier);
        counters.next_h1_revision += 1;
        H1SupplyOutcome::supplier_live(
            supplier,
            SupplyRevision::new(
                counters.next_h1_revision,
                H1SupplyStatus {
                    has_returnable_connection: returnable,
                    peer_use_blocked: false,
                },
            ),
        )
    }

    fn h2_revision(
        &mut self,
        supplier: PartitionId,
        status: H2SupplyStatus,
    ) -> SupplyRevision<H2SupplyStatus> {
        let counters = self.counters(supplier);
        counters.next_h2_revision += 1;
        SupplyRevision::new(counters.next_h2_revision, status)
    }

    fn free_slot<T>(slots: &[Option<T>]) -> Option<usize> {
        slots.iter().position(Option::is_none)
    }

    /// Simulates a closed connection returning its lease.
    fn return_one_held_permit(&mut self) -> bool {
        let Some(slot) = self.permits.iter().position(Option::is_some) else {
            return false;
        };
        let permit = self.permits[slot].take().expect("slot was occupied");
        self.state.return_permit(permit);
        true
    }

    /// Builds the successor the requesting cell would publish on settlement.
    fn successor_snapshot(&mut self, requester: PartitionId) -> DemandSnapshot {
        let requirement = self
            .state
            .demand
            .latest_for_test(&requester)
            .and_then(|latest| match &latest.state {
                DemandState::Active { requirement, .. } => Some(*requirement),
                DemandState::Inactive => None,
            })
            .unwrap_or(ProtocolRequirement::H1Compatible);
        let id = self.next_demand_id(requester);
        DemandSnapshot::active(
            id,
            SnapshotVersion::INITIAL,
            requirement,
            self.profile.group_of(requester),
        )
    }

    fn clear_match_slot(&mut self, match_id: H1MatchId) {
        for slot in &mut self.matches {
            if *slot == Some(match_id) {
                *slot = None;
            }
        }
    }

    // Demand.

    fn publish_demand(&mut self, partition: u8, requirement: Requirement) {
        let partition = self.partition(partition);
        let id = self.next_demand_id(partition);
        let snapshot = DemandSnapshot::active(
            id,
            SnapshotVersion::INITIAL,
            requirement.into(),
            self.profile.group_of(partition),
        );
        self.state.apply_demand_snapshot(partition, snapshot);
    }

    fn bump_demand_version(&mut self, partition: u8) {
        let partition = self.partition(partition);
        let Some(latest) = self.state.demand.latest_for_test(&partition).cloned() else {
            return;
        };
        let DemandState::Active {
            requirement,
            eligibility_group,
        } = latest.state
        else {
            return;
        };
        self.state.apply_demand_snapshot(
            partition,
            DemandSnapshot::active(
                latest.id,
                latest.version.next(),
                requirement,
                eligibility_group,
            ),
        );
    }

    fn cancel_demand(&mut self, partition: u8) {
        let partition = self.partition(partition);
        let Some(latest) = self.state.demand.latest_for_test(&partition).cloned() else {
            return;
        };
        if !latest.is_active() {
            return;
        }
        self.state.apply_demand_snapshot(
            partition,
            DemandSnapshot::inactive(latest.id, latest.version.next()),
        );
    }

    // Permits.

    fn take_permit(&mut self) {
        let Some(slot) = Self::free_slot(&self.permits) else {
            return;
        };
        if let Some(permit) = self.state.take_permit() {
            self.permits[slot] = Some(permit);
        }
    }

    fn return_permit(&mut self, slot: u8) {
        if let Some(permit) = self.permits[slot as usize % PERMIT_SLOTS].take() {
            self.state.return_permit(permit);
        }
    }

    // Capacity delivery.

    fn prepare_capacity_delivery(&mut self) {
        let Some(slot) = Self::free_slot(&self.assignments) else {
            return;
        };
        if let Some(prepared) = self.state.prepare_capacity_delivery() {
            self.report.capacity_deliveries += 1;
            self.assignments[slot] = Some(HeldAssignment {
                assignment: prepared.assignment,
                source: AssignmentSource::Capacity(prepared.permit),
            });
        }
    }

    fn settle_assignment(
        &mut self,
        slot: u8,
        outcome: Outcome,
        successor: bool,
        stale_route: bool,
    ) {
        let Some(held) = self.assignments[slot as usize % ASSIGNMENT_SLOTS].take() else {
            return;
        };
        let requester = held.assignment.requester;
        let successor = successor.then(|| self.successor_snapshot(requester));
        let outcome = match outcome {
            Outcome::Accepted => DemandAssignmentOutcome::Accepted { successor },
            Outcome::Refused => DemandAssignmentOutcome::Refused { successor },
            Outcome::Retry => DemandAssignmentOutcome::RetrySamePosition,
        };
        match held.source {
            AssignmentSource::Capacity(permit) => {
                // An accepted permit becomes the establishment's lease. Any
                // other outcome returns it before the assignment settles, as
                // `DeliveryGuard` does.
                match &outcome {
                    DemandAssignmentOutcome::Accepted { .. } => {
                        let slot = Self::free_slot(&self.permits)
                            .expect("permit slots hold every issued permit");
                        self.permits[slot] = Some(permit);
                    }
                    _ => self.state.return_permit(permit),
                }
                self.state.settle_assignment(&held.assignment, outcome);
            }
            AssignmentSource::BorrowedH1 { match_id, supplier } => {
                // `settle_borrow_delivery`: the assignment settles, then the
                // supplier cell reports its status and the match is removed.
                self.state.settle_assignment(&held.assignment, outcome);
                let live = self.h1_live(supplier, true);
                self.state.h1_supply.settle_match_for_test(match_id, live);
            }
            AssignmentSource::H2Route(prepared) => {
                if stale_route {
                    let AdmissionState {
                        h2_supply, demand, ..
                    } = &mut self.state;
                    h2_supply.remove_exact_generation_for_test(
                        &prepared.supplier,
                        prepared.generation,
                        demand,
                    );
                }
                self.state.settle_assignment(&held.assignment, outcome);
            }
        }
    }

    // HTTP/1 supply and matches.

    fn publish_h1_supply(&mut self, partition: u8, returnable: bool, blocked: bool) {
        let partition = self.partition(partition);
        let group = self.profile.group_of(partition);
        let counters = self.counters(partition);
        counters.next_h1_revision += 1;
        let revision = SupplyRevision::new(
            counters.next_h1_revision,
            H1SupplyStatus {
                has_returnable_connection: returnable,
                peer_use_blocked: blocked,
            },
        );
        self.state
            .h1_supply
            .apply_revision_for_test(partition, group, revision);
    }

    fn prepare_h1_match(&mut self) {
        let Some(slot) = Self::free_slot(&self.matches) else {
            return;
        };
        if let Some(prepared) = self.state.h1_supply.prepare_match(&self.state.demand) {
            self.matches[slot] = Some(prepared.match_id());
        }
    }

    fn held_match(&self, slot: u8) -> Option<(usize, H1MatchId, H1MatchAudit)> {
        let slot = slot as usize % MATCH_SLOTS;
        let match_id = self.matches[slot]?;
        let audit = self.state.h1_supply.match_audit(match_id)?;
        Some((slot, match_id, audit))
    }

    fn h1_probe_reply(&mut self, slot: u8, reply: ProbeReply) {
        let Some((slot, match_id, audit)) = self.held_match(slot) else {
            return;
        };
        if audit.phase != H1MatchPhase::ProbingIdle {
            return;
        }
        match reply {
            ProbeReply::IdleFound => {
                if !self
                    .state
                    .h1_supply
                    .probe_found_candidate_for_test(match_id)
                {
                    self.reject_candidate(match_id, audit.supplier);
                    self.matches[slot] = None;
                    return;
                }
                self.resolve_match(match_id, audit.supplier);
                self.matches[slot] = None;
            }
            ProbeReply::Busy | ProbeReply::Expired => {
                let outcome = if reply == ProbeReply::Busy {
                    self.h1_live(audit.supplier, true)
                } else {
                    H1SupplyOutcome::supplier_expired(audit.supplier)
                };
                let AdmissionState {
                    h1_supply, demand, ..
                } = &mut self.state;
                if h1_supply
                    .probe_missed_for_test(match_id, demand, outcome)
                    .is_none()
                {
                    self.matches[slot] = None;
                }
            }
        }
    }

    fn h1_reservation_reply(&mut self, slot: u8, reply: ReservationReply) {
        let Some((slot, match_id, audit)) = self.held_match(slot) else {
            return;
        };
        if audit.phase != H1MatchPhase::Reserving {
            return;
        }
        match reply {
            ReservationReply::Candidate => {
                if !self
                    .state
                    .h1_supply
                    .reservation_settled_for_test(match_id, true)
                {
                    self.reject_candidate(match_id, audit.supplier);
                    self.matches[slot] = None;
                    return;
                }
                self.resolve_match(match_id, audit.supplier);
                self.matches[slot] = None;
            }
            ReservationReply::Installed => {
                self.state
                    .h1_supply
                    .reservation_settled_for_test(match_id, false);
            }
            ReservationReply::Rejected => {
                let live = self.h1_live(audit.supplier, true);
                self.state.h1_supply.settle_match_for_test(match_id, live);
                self.matches[slot] = None;
            }
            ReservationReply::Expired => {
                self.state.h1_supply.settle_match_for_test(
                    match_id,
                    H1SupplyOutcome::supplier_expired(audit.supplier),
                );
                self.matches[slot] = None;
            }
        }
    }

    fn h1_sender_returned(&mut self, slot: u8, accepted: bool) {
        let Some((slot, match_id, audit)) = self.held_match(slot) else {
            return;
        };
        if !matches!(
            audit.phase,
            H1MatchPhase::WaitingForSender | H1MatchPhase::Cancelling
        ) {
            return;
        }
        if accepted {
            self.resolve_match(match_id, audit.supplier);
        } else {
            self.reject_candidate(match_id, audit.supplier);
        }
        self.matches[slot] = None;
    }

    fn h1_cancel_step(&mut self) {
        let Some(prepared) = self.state.h1_supply.prepare_cancellation() else {
            return;
        };
        let match_id = prepared.match_id();
        let live = self.h1_live(prepared.supplier(), true);
        self.state.h1_supply.settle_match_for_test(match_id, live);
        self.clear_match_slot(match_id);
    }

    /// Mirrors `h1::reject_candidate`: the sender goes back to its cell.
    fn reject_candidate(&mut self, match_id: H1MatchId, supplier: PartitionId) {
        let live = self.h1_live(supplier, true);
        self.state.h1_supply.settle_match_for_test(match_id, live);
    }

    /// Mirrors `h1::resolve_match` for a match whose sender is available.
    fn resolve_match(&mut self, match_id: H1MatchId, supplier: PartitionId) {
        let Some(retained) = self.state.h1_supply.begin_resolution_for_test(match_id) else {
            self.reject_candidate(match_id, supplier);
            return;
        };
        if retained.cancelled
            || !self
                .state
                .demand
                .is_current_queued(&retained.requester, retained.demand)
        {
            self.reject_candidate(match_id, retained.supplier);
            return;
        }
        match retained.kind {
            H1MatchKind::BorrowSender => {
                let Some(slot) = Self::free_slot(&self.assignments) else {
                    self.reject_candidate(match_id, retained.supplier);
                    return;
                };
                let assignment_id = self.state.take_assignment_id();
                let old_group = self.state.demand.group_for(&retained.requester);
                let Some(assignment) = self.state.demand.prepare_h1_assignment(
                    &retained.requester,
                    retained.demand,
                    assignment_id,
                ) else {
                    self.reject_candidate(match_id, retained.supplier);
                    return;
                };
                self.state
                    .reconcile_demand_indexes(&assignment.requester, old_group);
                self.report.h1_borrows += 1;
                self.assignments[slot] = Some(HeldAssignment {
                    assignment,
                    source: AssignmentSource::BorrowedH1 {
                        match_id,
                        supplier: retained.supplier,
                    },
                });
            }
            H1MatchKind::ReclaimCapacity => {
                // `H1CapacityReclaim`: the connection closes, its lease returns
                // the permit, then the match settles with the supplier's
                // post-close status.
                let closed = self.return_one_held_permit();
                self.report.h1_reclaims += 1;
                let outcome = self.h1_live(retained.supplier, !closed);
                self.state
                    .h1_supply
                    .settle_match_for_test(match_id, outcome);
            }
        }
    }

    // HTTP/2 supply, routes, and reclaim.

    fn publish_h2_supply(&mut self, partition: u8, fresh_generation: bool, idle: bool) {
        let partition = self.partition(partition);
        let group = self.profile.group_of(partition);
        let counters = self.counters(partition);
        let generation = match counters.current_generation {
            Some(current) if !fresh_generation => current,
            _ => {
                counters.next_generation += 1;
                H2GenerationId::for_test(counters.next_generation)
            }
        };
        counters.current_generation = Some(generation);
        let revision = self.h2_revision(partition, H2SupplyStatus::Accepting { generation, idle });
        let AdmissionState {
            h2_supply, demand, ..
        } = &mut self.state;
        h2_supply.apply_revision_for_test(partition, group, revision, demand);
    }

    fn publish_h2_unavailable(&mut self, partition: u8) {
        let partition = self.partition(partition);
        let group = self.profile.group_of(partition);
        self.counters(partition).current_generation = None;
        let revision = self.h2_revision(partition, H2SupplyStatus::Unavailable);
        let AdmissionState {
            h2_supply, demand, ..
        } = &mut self.state;
        h2_supply.apply_revision_for_test(partition, group, revision, demand);
    }

    fn prepare_h2_route(&mut self) {
        let Some(slot) = Self::free_slot(&self.assignments) else {
            return;
        };
        if let Some(route) = self.state.prepare_h2_route() {
            self.report.h2_routes += 1;
            self.assignments[slot] = Some(HeldAssignment {
                assignment: route.assignment.clone(),
                source: AssignmentSource::H2Route(route),
            });
        }
    }

    fn prepare_h2_reclaim(&mut self) {
        if self.h2_reclaim.is_some() {
            return;
        }
        // Production also gates this on the transport guaranteeing HTTP/1 for
        // an H1-required attempt; the harness always permits reclaim.
        if let Some(prepared) = self.state.h2_supply.prepare_reclaim(&self.state.demand) {
            self.h2_reclaim = Some(prepared);
        }
    }

    fn settle_h2_reclaim(&mut self, closed: bool) {
        let Some(prepared) = self.h2_reclaim.take() else {
            return;
        };
        self.report.h2_reclaims += 1;
        let status = if closed {
            // `reclaim_idle_h2` logically closed the generation; its lease
            // returned the permit before admission settled the reclaim.
            self.return_one_held_permit();
            self.counters(prepared.supplier).current_generation = None;
            H2SupplyStatus::Unavailable
        } else {
            match self.counters(prepared.supplier).current_generation {
                Some(generation) => H2SupplyStatus::Accepting {
                    generation,
                    idle: false,
                },
                None => H2SupplyStatus::Unavailable,
            }
        };
        let revision = self.h2_revision(prepared.supplier, status);
        let AdmissionState {
            h2_supply, demand, ..
        } = &mut self.state;
        h2_supply.settle_reclaim_for_test(&prepared, Some(revision), demand);
    }

    // Liveness.

    /// Drains admission in production action order, then checks for stalls.
    ///
    /// Each prepared crossing is settled the way its detached action most
    /// commonly would: permits and routes are accepted, HTTP/1 matches find an
    /// idle sender, and reclaims close their connection. Every step removes a
    /// demand, a match, a generation, or a permit from play, so the loop
    /// terminates.
    pub(in crate::client::pool::admission) fn quiesce(&mut self) {
        let mut steps = 0;
        loop {
            steps += 1;
            self.report.quiesce_steps += 1;
            assert!(
                steps <= QUIESCE_STEP_LIMIT,
                "quiesce did not converge after {QUIESCE_STEP_LIMIT} steps"
            );
            if let Some(prepared) = self.state.h1_supply.prepare_cancellation() {
                let match_id = prepared.match_id();
                let live = self.h1_live(prepared.supplier(), true);
                self.state.h1_supply.settle_match_for_test(match_id, live);
                self.clear_match_slot(match_id);
                continue;
            }
            if let Some(prepared) = self.state.prepare_capacity_delivery() {
                self.report.capacity_deliveries += 1;
                let slot =
                    Self::free_slot(&self.permits).expect("permit slots hold every issued permit");
                self.permits[slot] = Some(prepared.permit);
                self.state.settle_assignment(
                    &prepared.assignment,
                    DemandAssignmentOutcome::Accepted { successor: None },
                );
                continue;
            }
            if let Some(route) = self.state.prepare_h2_route() {
                self.report.h2_routes += 1;
                self.state.settle_assignment(
                    &route.assignment,
                    DemandAssignmentOutcome::Accepted { successor: None },
                );
                continue;
            }
            if let Some(prepared) = self.state.h1_supply.prepare_match(&self.state.demand) {
                let match_id = prepared.match_id();
                let supplier = prepared.supplier();
                let resolving = match prepared {
                    PreparedH1Match::ProbeIdle(_) => self
                        .state
                        .h1_supply
                        .probe_found_candidate_for_test(match_id),
                    PreparedH1Match::Reserve(_) => self
                        .state
                        .h1_supply
                        .reservation_settled_for_test(match_id, true),
                };
                assert!(resolving, "freshly prepared match refused its candidate");
                self.resolve_match_for_quiesce(match_id, supplier);
                continue;
            }
            if let Some(prepared) = self.state.h2_supply.prepare_reclaim(&self.state.demand) {
                self.h2_reclaim = Some(prepared);
                self.settle_h2_reclaim(true);
                continue;
            }
            break;
        }
        self.assert_quiescent();
    }

    /// Resolves a match during quiesce and settles any borrow it produces.
    fn resolve_match_for_quiesce(&mut self, match_id: H1MatchId, supplier: PartitionId) {
        self.resolve_match(match_id, supplier);
        let Some(slot) = self.assignments.iter().position(|held| {
            matches!(
                held,
                Some(HeldAssignment {
                    source: AssignmentSource::BorrowedH1 { match_id: held_id, .. },
                    ..
                }) if *held_id == match_id
            )
        }) else {
            return;
        };
        let held = self.assignments[slot].take().expect("slot was occupied");
        let AssignmentSource::BorrowedH1 { supplier, .. } = held.source else {
            unreachable!("slot held a borrowed assignment");
        };
        self.state.settle_assignment(
            &held.assignment,
            DemandAssignmentOutcome::Accepted { successor: None },
        );
        let live = self.h1_live(supplier, true);
        self.state.h1_supply.settle_match_for_test(match_id, live);
    }

    /// Checks that no work remains once admission reports none.
    ///
    /// These predicates state the current scheduler's contract. Permits and
    /// HTTP/1 supply are selected for the origin head only; HTTP/2 routes are
    /// selected per eligibility group. A scheduler that offers HTTP/1 supply
    /// per group strengthens the HTTP/1 predicate to match the HTTP/2 one.
    fn assert_quiescent(&self) {
        if self.state.capacity.available > 0 {
            assert!(
                !self.state.demand.head_is_queued(),
                "a permit is available while the origin head is queued"
            );
        }
        if let Some(head) = self.state.demand.queued_head() {
            let retained = self.state.h1_supply.match_for_requester(&head.requester);
            if retained.is_none() {
                if head.requirement.accepts_h1() {
                    assert!(
                        self.state
                            .h1_supply
                            .selectable_peer_supplier(&head.eligibility_group, head.requester)
                            .is_none(),
                        "origin head {:?} could borrow an idle peer sender",
                        head.requester
                    );
                    assert!(
                        !self
                            .state
                            .h1_supply
                            .has_peer_reclaim_supplier(head.requester),
                        "origin head {:?} could reclaim a peer HTTP/1 connection",
                        head.requester
                    );
                } else {
                    assert!(
                        !self.state.h1_supply.has_any_reclaim_supplier(),
                        "HTTP/2-required head {:?} could reclaim an HTTP/1 connection",
                        head.requester
                    );
                }
            }
            if !head.requirement.accepts_h2() {
                assert!(
                    !self.state.h2_supply.has_reclaim_generation(),
                    "HTTP/1-required head {:?} could reclaim an idle HTTP/2 generation",
                    head.requester
                );
            }
        }
        for group in self.profile.groups() {
            let Some(head) = self.state.demand.queued_group_head(&group) else {
                continue;
            };
            if head.requirement.accepts_h2() {
                assert!(
                    self.state
                        .h2_supply
                        .routeable_peer_generation(&group, head.requester)
                        .is_none(),
                    "group head {:?} could route to a peer HTTP/2 generation",
                    head.requester
                );
            }
        }
    }

    // Cross-structure audit.

    /// Checks relationships between admission state and held handles.
    ///
    /// Each sub-state already checks its own consistency after every
    /// mutation. This audit adds the facts that span them.
    fn audit(&self) {
        let limit = self.state.capacity.limit;
        let available = self.state.capacity.available;
        let held = self.permits.iter().flatten().count();
        let crossing = self
            .assignments
            .iter()
            .flatten()
            .filter(|held| matches!(held.source, AssignmentSource::Capacity(_)))
            .count();
        assert_eq!(
            available + held + crossing,
            limit,
            "permits were created or lost: available {available}, held {held}, crossing {crossing}"
        );

        let demand = self.state.demand.audit();
        let held_assignments: Vec<_> = self.assignments.iter().flatten().collect();
        assert_eq!(
            self.state.demand.pending_assignment_count(),
            held_assignments.len(),
            "pending assignments do not match held assignments"
        );
        for held in &held_assignments {
            let partition = demand
                .partition(&held.assignment.requester)
                .expect("assigned requester has a demand record");
            assert_eq!(
                partition.assignment(),
                Some(&held.assignment),
                "held assignment is not the requester's pending assignment"
            );
        }

        let h1 = self.state.h1_supply.audit();
        let mut owned_matches: Vec<H1MatchId> = self.matches.iter().flatten().copied().collect();
        owned_matches.extend(
            held_assignments
                .iter()
                .filter_map(|held| match held.source {
                    AssignmentSource::BorrowedH1 { match_id, .. } => Some(match_id),
                    _ => None,
                }),
        );
        owned_matches.sort();
        let retained: Vec<H1MatchId> = h1.matches.iter().map(|(match_id, _)| *match_id).collect();
        assert_eq!(
            owned_matches, retained,
            "retained matches do not match harness-held matches"
        );
        for (match_id, retained) in &h1.matches {
            if retained.cancelled {
                continue;
            }
            let latest = self
                .state
                .demand
                .latest_for_test(&retained.requester)
                .expect("match requester has a demand record");
            assert!(
                latest.id == retained.demand && latest.is_active(),
                "match {match_id:?} is not cancelled but its demand is no longer current"
            );
        }

        let h2 = self.state.h2_supply.audit();
        assert_eq!(
            h2.reclaiming, self.h2_reclaim,
            "admission's reclaim reservation does not match the held one"
        );
    }
}

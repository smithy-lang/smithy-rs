/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Generated operation sequences against the production acquisition queue.

use super::super::test_util::operation_sequence::{
    CutoffSelector, DemandSelector, Lane, Operation, Requirement, CROSSINGS, SLOTS,
};
use super::super::test_util::queue_harness::run_sequence;
use proptest::prelude::*;

const MAX_OPERATIONS: usize = 64;

fn requirement() -> impl Strategy<Value = Requirement> {
    prop_oneof![
        2 => Just(Requirement::H1Required),
        4 => Just(Requirement::H1Compatible),
        2 => Just(Requirement::H2Required),
    ]
}

fn demand_selector() -> impl Strategy<Value = DemandSelector> {
    prop_oneof![
        5 => Just(DemandSelector::Current),
        1 => Just(DemandSelector::Stale),
    ]
}

fn cutoff() -> impl Strategy<Value = CutoffSelector> {
    prop_oneof![
        3 => Just(CutoffSelector::None),
        2 => Just(CutoffSelector::RouteCutoff),
        2 => (0u8..SLOTS).prop_map(CutoffSelector::Slot),
    ]
}

/// Generates operations weighted toward the ones that create work.
///
/// Registration is weighted highest so sequences populate several slots.
/// Cancellation is weighted low so most attempts reach a delivery. The
/// bounded lane adds the admission-side operations; the unbounded lane
/// never sees them because production never calls them for such a cell.
pub(super) fn operation(lane: Lane) -> BoxedStrategy<Operation> {
    let slot = 0u8..SLOTS;
    let crossing = 0u8..CROSSINGS as u8;
    match lane {
        Lane::Unbounded => prop_oneof![
            8 => (slot.clone(), requirement())
                .prop_map(|(slot, requirement)| Operation::Register { slot, requirement }),
            3 => slot.clone().prop_map(|slot| Operation::Cancel { slot }),
            6 => slot.clone().prop_map(|slot| Operation::Poll { slot }),
            4 => slot.clone().prop_map(|slot| Operation::StartEstablishment { slot }),
            4 => slot.prop_map(|slot| Operation::CommitEstablishment { slot }),
            4 => Just(Operation::OfferReturnedH1),
            4 => cutoff().prop_map(|cutoff| Operation::OfferH2Activation { cutoff }),
            1 => Just(Operation::Quiesce),
        ]
        .boxed(),
        Lane::Bounded => prop_oneof![
            8 => (slot.clone(), requirement())
                .prop_map(|(slot, requirement)| Operation::Register { slot, requirement }),
            3 => slot.clone().prop_map(|slot| Operation::Cancel { slot }),
            8 => slot.clone().prop_map(|slot| Operation::Poll { slot }),
            5 => slot.clone().prop_map(|slot| Operation::StartEstablishment { slot }),
            4 => slot.prop_map(|slot| Operation::CommitEstablishment { slot }),
            8 => demand_selector().prop_map(|demand| Operation::ReserveDelivery { demand }),
            6 => crossing.clone().prop_map(|crossing| Operation::CommitCapacity { crossing }),
            3 => crossing.prop_map(|crossing| Operation::CommitBorrowedH1 { crossing }),
            3 => Just(Operation::OfferReturnedH1),
            3 => cutoff().prop_map(|cutoff| Operation::OfferH2Activation { cutoff }),
            2 => demand_selector().prop_map(|demand| Operation::SupersedeDemand { demand }),
            1 => Just(Operation::Quiesce),
        ]
        .boxed(),
    }
}

pub(super) fn sequence(lane: Lane) -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation(lane), 1..=MAX_OPERATIONS)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        ..ProptestConfig::default()
    })]

    #[test]
    fn bounded_sequences_preserve_invariants(operations in sequence(Lane::Bounded)) {
        run_sequence(Lane::Bounded, &operations);
    }

    #[test]
    fn unbounded_sequences_preserve_invariants(operations in sequence(Lane::Unbounded)) {
        run_sequence(Lane::Unbounded, &operations);
    }
}

// Longer soak for local runs; not part of the default suite.
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 10_000,
        ..ProptestConfig::default()
    })]
    #[test]
    #[ignore = "long soak; run explicitly"]
    fn soak_bounded_sequences(operations in sequence(Lane::Bounded)) {
        run_sequence(Lane::Bounded, &operations);
    }
}

// Deterministic sequences that pin the harness's own behavior so a broken
// harness cannot pass the generated tests vacuously.

#[test]
fn empty_sequence_is_quiescent_in_every_lane() {
    for lane in Lane::ALL {
        let report = run_sequence(lane, &[]);
        assert_eq!(0, report.operations);
        assert_eq!(0, report.registrations);
        assert_eq!(1, report.quiesce_runs);
        assert_eq!(0, report.quiesce_steps);
    }
}

#[test]
fn bounded_delivery_reaches_establishment() {
    let report = run_sequence(
        Lane::Bounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::Poll { slot: 0 },
            Operation::ReserveDelivery {
                demand: DemandSelector::Current,
            },
            Operation::CommitCapacity { crossing: 0 },
            Operation::Poll { slot: 0 },
            Operation::StartEstablishment { slot: 0 },
            Operation::CommitEstablishment { slot: 0 },
            Operation::Poll { slot: 0 },
        ],
    );
    assert_eq!(1, report.reservations);
    assert_eq!(1, report.capacity_committed);
    assert_eq!(1, report.polls_pending);
    assert_eq!(1, report.polls_start);
    assert_eq!(1, report.start_true);
    assert_eq!(1, report.establish_committed_started);
    assert_eq!(1, report.polls_resolved);
    // The pending poll was woken once, by the capacity commit.
    assert_eq!(1, report.wakes);
    assert_eq!(0, report.quiesce_steps);
}

#[test]
fn local_h1_return_beats_crossing_capacity() {
    let report = run_sequence(
        Lane::Bounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::Poll { slot: 0 },
            Operation::ReserveDelivery {
                demand: DemandSelector::Current,
            },
            Operation::OfferReturnedH1,
            Operation::CommitCapacity { crossing: 0 },
            Operation::Poll { slot: 0 },
        ],
    );
    assert_eq!(1, report.h1_to_reserved);
    assert_eq!(1, report.capacity_refused_pending);
    assert_eq!(1, report.wakes);
    assert_eq!(1, report.polls_resolved);
}

#[test]
fn cancel_during_crossing_returns_both_payloads() {
    let report = run_sequence(
        Lane::Bounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::Poll { slot: 0 },
            Operation::ReserveDelivery {
                demand: DemandSelector::Current,
            },
            Operation::OfferReturnedH1,
            Operation::Cancel { slot: 0 },
            Operation::Cancel { slot: 0 },
            Operation::CommitBorrowedH1 { crossing: 0 },
        ],
    );
    assert_eq!(1, report.cancel_reserved);
    assert_eq!(1, report.cancel_repeated);
    assert_eq!(1, report.borrowed_refused_cancelled);
    // The cancelled attempt's waker still comes back with the payloads.
    assert_eq!(1, report.wakes);
}

#[test]
fn returned_h1_prefers_older_launching_attempt_over_waiting_head() {
    let report = run_sequence(
        Lane::Bounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::ReserveDelivery {
                demand: DemandSelector::Current,
            },
            Operation::CommitCapacity { crossing: 0 },
            Operation::Poll { slot: 0 },
            Operation::Register {
                slot: 1,
                requirement: Requirement::H1Compatible,
            },
            Operation::OfferReturnedH1,
        ],
    );
    assert_eq!(1, report.h1_to_launching);
    assert_eq!(0, report.h1_to_waiting);
}

#[test]
fn h2_activation_through_a_cutoff_serves_only_prioritized_attempts() {
    let report = run_sequence(
        Lane::Unbounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H2Required,
            },
            Operation::Register {
                slot: 1,
                requirement: Requirement::H2Required,
            },
            // Slot 0 is at the cutoff and is served.
            Operation::OfferH2Activation {
                cutoff: CutoffSelector::Slot(0),
            },
            // Only slot 1 remains and it is past the cutoff: production
            // would not offer through this cutoff, so the harness skips.
            Operation::OfferH2Activation {
                cutoff: CutoffSelector::Slot(0),
            },
            // The route cutoff recorded now covers slot 1.
            Operation::OfferH2Activation {
                cutoff: CutoffSelector::RouteCutoff,
            },
        ],
    );
    assert_eq!(2, report.h2_to_admitted);
    assert_eq!(1, report.skipped);
    assert_eq!(0, report.h2_none);
}

#[test]
fn retired_demand_is_rejected() {
    let report = run_sequence(
        Lane::Bounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H2Required,
            },
            Operation::Register {
                slot: 1,
                requirement: Requirement::H1Compatible,
            },
            Operation::Cancel { slot: 0 },
            Operation::ReserveDelivery {
                demand: DemandSelector::Stale,
            },
            Operation::SupersedeDemand {
                demand: DemandSelector::Stale,
            },
            Operation::SupersedeDemand {
                demand: DemandSelector::Current,
            },
        ],
    );
    assert_eq!(1, report.cancel_waiting_head);
    assert_eq!(1, report.reservations_rejected);
    assert_eq!(1, report.supersede_false);
    assert_eq!(1, report.supersede_true);
}

#[test]
fn establishment_task_reports_after_cancellation() {
    let report = run_sequence(
        Lane::Unbounded,
        &[
            Operation::Register {
                slot: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::Poll { slot: 0 },
            Operation::Cancel { slot: 0 },
            Operation::StartEstablishment { slot: 0 },
            Operation::CommitEstablishment { slot: 0 },
        ],
    );
    assert_eq!(1, report.polls_start);
    assert_eq!(1, report.cancel_launching);
    assert_eq!(1, report.start_false);
    assert_eq!(1, report.establish_refused);
}

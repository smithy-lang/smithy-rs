/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Generated operation sequences against production admission state.

use super::super::test_util::model_harness::{run_sequence, SequenceReport};
use super::super::test_util::operation_sequence::{
    Operation, Outcome, ProbeReply, Profile, Requirement, ReservationReply,
};
use proptest::prelude::*;

const MAX_OPERATIONS: usize = 64;

fn requirement() -> impl Strategy<Value = Requirement> {
    prop_oneof![
        2 => Just(Requirement::H1Required),
        4 => Just(Requirement::H1Compatible),
        2 => Just(Requirement::H2Required),
    ]
}

fn outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        5 => Just(Outcome::Accepted),
        2 => Just(Outcome::Refused),
        2 => Just(Outcome::Retry),
    ]
}

fn probe_reply() -> impl Strategy<Value = ProbeReply> {
    prop_oneof![
        4 => Just(ProbeReply::IdleFound),
        3 => Just(ProbeReply::Busy),
        1 => Just(ProbeReply::Expired),
    ]
}

fn reservation_reply() -> impl Strategy<Value = ReservationReply> {
    prop_oneof![
        3 => Just(ReservationReply::Candidate),
        3 => Just(ReservationReply::Installed),
        2 => Just(ReservationReply::Rejected),
        1 => Just(ReservationReply::Expired),
    ]
}

/// Generates operations weighted toward the ones that create work.
///
/// Prepare and settle operations are no-ops until demand and supply exist,
/// so publication is weighted highest. Cancellation and version bumps are
/// weighted low so most sequences reach saturation.
fn operation(profile: Profile) -> impl Strategy<Value = Operation> {
    let partition = 0u8..profile.partitions();
    let slot = 0u8..4;
    prop_oneof![
        8 => (partition.clone(), requirement())
            .prop_map(|(partition, requirement)| Operation::PublishDemand { partition, requirement }),
        1 => partition.clone().prop_map(|partition| Operation::BumpDemandVersion { partition }),
        2 => partition.clone().prop_map(|partition| Operation::CancelDemand { partition }),
        3 => Just(Operation::TakePermit),
        3 => slot.clone().prop_map(|slot| Operation::ReturnPermit { slot }),
        6 => Just(Operation::PrepareCapacityDelivery),
        6 => (slot.clone(), outcome(), any::<bool>(), any::<bool>()).prop_map(
            |(slot, outcome, successor, stale_route)| Operation::SettleAssignment {
                slot,
                outcome,
                successor,
                stale_route,
            },
        ),
        6 => (partition.clone(), any::<bool>(), any::<bool>()).prop_map(
            |(partition, returnable, blocked)| Operation::PublishH1Supply {
                partition,
                returnable,
                blocked,
            },
        ),
        5 => Just(Operation::PrepareH1Match),
        4 => (slot.clone(), probe_reply())
            .prop_map(|(slot, reply)| Operation::H1ProbeReply { slot, reply }),
        4 => (slot.clone(), reservation_reply())
            .prop_map(|(slot, reply)| Operation::H1ReservationReply { slot, reply }),
        3 => (slot.clone(), any::<bool>())
            .prop_map(|(slot, accepted)| Operation::H1SenderReturned { slot, accepted }),
        2 => Just(Operation::H1CancelStep),
        6 => (partition.clone(), any::<bool>(), any::<bool>()).prop_map(
            |(partition, fresh_generation, idle)| Operation::PublishH2Supply {
                partition,
                fresh_generation,
                idle,
            },
        ),
        2 => partition.prop_map(|partition| Operation::PublishH2Unavailable { partition }),
        5 => Just(Operation::PrepareH2Route),
        3 => Just(Operation::PrepareH2Reclaim),
        3 => any::<bool>().prop_map(|closed| Operation::SettleH2Reclaim { closed }),
        1 => Just(Operation::Quiesce),
    ]
}

pub(super) fn sequence(profile: Profile) -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation(profile), 1..=MAX_OPERATIONS)
}

macro_rules! profile_property {
    ($name:ident, $profile:expr) => {
        proptest! {
            #![proptest_config(ProptestConfig {
                cases: 256,
                ..ProptestConfig::default()
            })]
            #[test]
            fn $name(operations in sequence($profile)) {
                run_sequence($profile, &operations);
            }
        }
    };
}

profile_property!(
    single_partition_sequences_preserve_invariants,
    Profile::Single
);
profile_property!(
    shared_group_sequences_preserve_invariants,
    Profile::SharedGroup
);
profile_property!(two_group_sequences_preserve_invariants, Profile::TwoGroups);
profile_property!(
    partition_local_sequences_preserve_invariants,
    Profile::PartitionLocal
);

// Longer soak for local runs; not part of the default suite.
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 10_000,
        ..ProptestConfig::default()
    })]
    #[test]
    #[ignore = "long soak; run explicitly"]
    fn soak_shared_group_sequences(operations in sequence(Profile::SharedGroup)) {
        run_sequence(Profile::SharedGroup, &operations);
    }
}

// Deterministic sequences that pin the harness's own behavior so a broken
// harness cannot pass the generated tests vacuously.

#[test]
fn empty_sequence_is_quiescent_in_every_profile() {
    for profile in Profile::ALL {
        let report = run_sequence(profile, &[]);
        assert_eq!(0, report.operations);
        assert_eq!(0, report.capacity_deliveries);
    }
}

#[test]
fn demand_with_available_capacity_is_delivered_at_quiesce() {
    let report = run_sequence(
        Profile::SharedGroup,
        &[
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::PublishDemand {
                partition: 1,
                requirement: Requirement::H1Compatible,
            },
            Operation::PublishDemand {
                partition: 2,
                requirement: Requirement::H1Compatible,
            },
        ],
    );
    // Two permits serve two of the three demands; the third stays queued
    // with no permit or supply, which is a valid quiescent state.
    assert_eq!(2, report.capacity_deliveries);
}

#[test]
fn idle_peer_sender_is_borrowed_at_saturation() {
    let report = run_sequence(
        Profile::SharedGroup,
        &[
            Operation::TakePermit,
            Operation::TakePermit,
            Operation::PublishH1Supply {
                partition: 1,
                returnable: true,
                blocked: false,
            },
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Compatible,
            },
        ],
    );
    assert_eq!(0, report.capacity_deliveries);
    assert_eq!(1, report.h1_borrows);
}

#[test]
fn peer_h2_generation_is_routed_at_saturation() {
    let report = run_sequence(
        Profile::SharedGroup,
        &[
            Operation::TakePermit,
            Operation::TakePermit,
            Operation::PublishH2Supply {
                partition: 1,
                fresh_generation: true,
                idle: false,
            },
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Compatible,
            },
        ],
    );
    assert_eq!(0, report.capacity_deliveries);
    assert_eq!(1, report.h2_routes);
    assert_eq!(0, report.h1_borrows);
}

#[test]
fn incompatible_head_reclaims_idle_h2_at_saturation() {
    let report = run_sequence(
        Profile::SharedGroup,
        &[
            Operation::TakePermit,
            Operation::TakePermit,
            Operation::PublishH2Supply {
                partition: 1,
                fresh_generation: true,
                idle: true,
            },
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Required,
            },
        ],
    );
    // Reclaim closes the idle generation, its permit returns, and the
    // returned permit is delivered to the H1-required head.
    assert_eq!(1, report.h2_reclaims);
    assert_eq!(1, report.capacity_deliveries);
}

#[test]
fn partition_local_scope_never_borrows_or_routes() {
    let report = run_sequence(
        Profile::PartitionLocal,
        &[
            Operation::TakePermit,
            Operation::TakePermit,
            Operation::PublishH1Supply {
                partition: 1,
                returnable: true,
                blocked: false,
            },
            Operation::PublishH2Supply {
                partition: 2,
                fresh_generation: true,
                idle: false,
            },
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Compatible,
            },
        ],
    );
    assert_eq!(0, report.h1_borrows);
    assert_eq!(0, report.h2_routes);
    // The only way forward is reclaiming a peer's HTTP/1 connection.
    assert_eq!(1, report.h1_reclaims);
}

#[test]
fn report_counts_are_exact_for_a_mixed_sequence() {
    let report = run_sequence(
        Profile::TwoGroups,
        &[
            Operation::PublishDemand {
                partition: 0,
                requirement: Requirement::H1Compatible,
            },
            Operation::PrepareCapacityDelivery,
            Operation::SettleAssignment {
                slot: 0,
                outcome: Outcome::Accepted,
                successor: false,
                stale_route: false,
            },
        ],
    );
    assert_eq!(
        SequenceReport {
            operations: 3,
            capacity_deliveries: 1,
            quiesce_steps: 1,
            ..SequenceReport::default()
        },
        report
    );
}

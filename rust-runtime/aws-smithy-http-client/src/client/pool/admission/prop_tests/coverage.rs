/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checks that the generator reaches every resource path in each profile.
//!
//! A property test that never prepares a borrow or a reclaim would pass
//! vacuously. This test aggregates coverage over a fixed number of generated
//! sequences and requires each path the profile permits to occur.

use super::super::test_util::model_harness::{run_sequence, SequenceReport};
use super::super::test_util::operation_sequence::Profile;
use super::prop::sequence;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};

const SAMPLES: usize = 512;

fn aggregate(profile: Profile) -> SequenceReport {
    let mut runner = TestRunner::new(Config {
        cases: SAMPLES as u32,
        ..Config::default()
    });
    let strategy = sequence(profile);
    let mut total = SequenceReport::default();
    for _ in 0..SAMPLES {
        let operations = strategy
            .new_tree(&mut runner)
            .expect("sequence strategy generates")
            .current();
        let report = run_sequence(profile, &operations);
        total.operations += report.operations;
        total.capacity_deliveries += report.capacity_deliveries;
        total.h1_borrows += report.h1_borrows;
        total.h1_reclaims += report.h1_reclaims;
        total.h2_routes += report.h2_routes;
        total.h2_reclaims += report.h2_reclaims;
        total.quiesce_steps += report.quiesce_steps;
    }
    total
}

#[test]
fn single_profile_reaches_permit_and_reclaim_paths_only() {
    let total = aggregate(Profile::Single);
    assert!(total.capacity_deliveries > 0, "{total:?}");
    // With no peers, protocol-incompatible demand can proceed only by
    // reclaiming the partition's own idle connection of the other protocol.
    assert!(total.h1_reclaims > 0, "{total:?}");
    assert!(total.h2_reclaims > 0, "{total:?}");
    // One partition has no peers: borrow and route must never occur.
    assert_eq!(0, total.h1_borrows, "{total:?}");
    assert_eq!(0, total.h2_routes, "{total:?}");
}

#[test]
fn shared_group_profile_reaches_every_path() {
    let total = aggregate(Profile::SharedGroup);
    assert!(total.capacity_deliveries > 0, "{total:?}");
    assert!(total.h1_borrows > 0, "{total:?}");
    assert!(total.h1_reclaims > 0, "{total:?}");
    assert!(total.h2_routes > 0, "{total:?}");
    assert!(total.h2_reclaims > 0, "{total:?}");
}

#[test]
fn two_group_profile_reaches_every_path() {
    let total = aggregate(Profile::TwoGroups);
    assert!(total.capacity_deliveries > 0, "{total:?}");
    assert!(total.h1_borrows > 0, "{total:?}");
    assert!(total.h1_reclaims > 0, "{total:?}");
    assert!(total.h2_routes > 0, "{total:?}");
    assert!(total.h2_reclaims > 0, "{total:?}");
}

#[test]
fn partition_local_profile_reclaims_but_never_reuses() {
    let total = aggregate(Profile::PartitionLocal);
    assert!(total.capacity_deliveries > 0, "{total:?}");
    assert!(total.h1_reclaims > 0, "{total:?}");
    assert!(total.h2_reclaims > 0, "{total:?}");
    assert_eq!(0, total.h1_borrows, "{total:?}");
    assert_eq!(0, total.h2_routes, "{total:?}");
}

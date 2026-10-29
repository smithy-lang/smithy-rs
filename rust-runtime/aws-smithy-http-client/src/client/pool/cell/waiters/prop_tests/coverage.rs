/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checks that the generator reaches every queue transition in each lane.
//!
//! A property test whose sequences never cancel during a crossing or never
//! let a local result beat a delivery would pass vacuously. This aggregates
//! coverage over a fixed number of generated sequences and requires each
//! transition the lane permits to occur, and each it forbids not to.

use super::super::test_util::operation_sequence::Lane;
use super::super::test_util::queue_harness::{run_sequence, SequenceReport};
use super::prop::sequence;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};

const SAMPLES: usize = 1_024;

fn aggregate(lane: Lane) -> SequenceReport {
    let mut runner = TestRunner::new(Config {
        cases: SAMPLES as u32,
        ..Config::default()
    });
    let strategy = sequence(lane);
    let mut total = SequenceReport::default();
    for _ in 0..SAMPLES {
        let operations = strategy
            .new_tree(&mut runner)
            .expect("sequence strategy generates")
            .current();
        total.absorb(&run_sequence(lane, &operations));
    }
    total
}

#[test]
fn bounded_lane_reaches_every_transition() {
    let total = aggregate(Lane::Bounded);

    // Admission crossings, including both ways a commit can lose.
    assert!(total.reservations > 0, "{total:?}");
    assert!(total.reservations_rejected > 0, "{total:?}");
    assert!(total.capacity_committed > 0, "{total:?}");
    assert!(total.capacity_refused_pending > 0, "{total:?}");
    assert!(total.capacity_refused_cancelled > 0, "{total:?}");
    assert!(total.borrowed_committed > 0, "{total:?}");
    assert!(total.borrowed_refused_pending > 0, "{total:?}");
    assert!(total.borrowed_refused_cancelled > 0, "{total:?}");
    assert!(total.supersede_true > 0, "{total:?}");
    assert!(total.supersede_false > 0, "{total:?}");

    // Protocol offers landing in every residence that accepts them.
    assert!(total.h1_to_waiting > 0, "{total:?}");
    assert!(total.h1_to_reserved > 0, "{total:?}");
    assert!(total.h1_to_admitted > 0, "{total:?}");
    assert!(total.h1_to_launching > 0, "{total:?}");
    assert!(total.h1_refused > 0, "{total:?}");
    assert!(total.h2_to_waiting > 0, "{total:?}");
    assert!(total.h2_to_reserved > 0, "{total:?}");
    assert!(total.h2_to_admitted > 0, "{total:?}");
    assert!(total.h2_to_launching > 0, "{total:?}");
    assert!(total.h2_none > 0, "{total:?}");

    // Establishment racing a result, and reporting after removal.
    assert!(total.start_true > 0, "{total:?}");
    assert!(total.start_false > 0, "{total:?}");
    assert!(total.establish_committed_submitted > 0, "{total:?}");
    assert!(total.establish_committed_started > 0, "{total:?}");
    assert!(total.establish_refused > 0, "{total:?}");

    // Cancellation from every residence.
    assert!(total.cancel_waiting_head > 0, "{total:?}");
    assert!(total.cancel_waiting_other > 0, "{total:?}");
    assert!(total.cancel_reserved > 0, "{total:?}");
    assert!(total.cancel_admitted > 0, "{total:?}");
    assert!(total.cancel_ready > 0, "{total:?}");
    assert!(total.cancel_launching > 0, "{total:?}");
    assert!(total.cancel_repeated > 0, "{total:?}");

    // Wakers came back for outstanding polls.
    assert!(total.wakes > 0, "{total:?}");
    assert!(total.polls_pending > 0, "{total:?}");
    assert!(total.polls_resolved > 0, "{total:?}");
}

#[test]
fn unbounded_lane_never_touches_admission() {
    let total = aggregate(Lane::Unbounded);

    // Reached.
    assert!(total.registrations > 0, "{total:?}");
    assert!(total.polls_start > 0, "{total:?}");
    assert!(total.start_true > 0, "{total:?}");
    assert!(total.start_false > 0, "{total:?}");
    assert!(total.establish_committed_submitted > 0, "{total:?}");
    assert!(total.establish_committed_started > 0, "{total:?}");
    assert!(total.establish_refused > 0, "{total:?}");
    assert!(total.h1_to_admitted > 0, "{total:?}");
    assert!(total.h1_to_launching > 0, "{total:?}");
    assert!(total.h1_refused > 0, "{total:?}");
    assert!(total.h2_to_admitted > 0, "{total:?}");
    assert!(total.h2_to_launching > 0, "{total:?}");
    assert!(total.cancel_admitted > 0, "{total:?}");
    assert!(total.cancel_ready > 0, "{total:?}");
    assert!(total.cancel_launching > 0, "{total:?}");
    assert!(total.cancel_repeated > 0, "{total:?}");
    assert!(total.wakes > 0, "{total:?}");

    // Forbidden: no FIFO, no demand, no crossings.
    assert_eq!(0, total.reservations, "{total:?}");
    assert_eq!(0, total.reservations_rejected, "{total:?}");
    assert_eq!(0, total.capacity_committed, "{total:?}");
    assert_eq!(0, total.borrowed_committed, "{total:?}");
    assert_eq!(0, total.supersede_true, "{total:?}");
    assert_eq!(0, total.supersede_false, "{total:?}");
    assert_eq!(0, total.h1_to_waiting, "{total:?}");
    assert_eq!(0, total.h1_to_reserved, "{total:?}");
    assert_eq!(0, total.h2_to_waiting, "{total:?}");
    assert_eq!(0, total.h2_to_reserved, "{total:?}");
    assert_eq!(0, total.cancel_waiting_head, "{total:?}");
    assert_eq!(0, total.cancel_waiting_other, "{total:?}");
    assert_eq!(0, total.cancel_reserved, "{total:?}");
}

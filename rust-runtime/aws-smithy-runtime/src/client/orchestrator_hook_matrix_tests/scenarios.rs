/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! The hook list, the expectation vocabulary, and the scenario table itself.

use super::fixtures::BoundedRetryStrategy;
use super::recorder::Observed;
use aws_smithy_types::timeout::TimeoutConfig;
use std::time::Duration;

/// `known_hooks!()` directly so that a *reordering* is caught too, not only an addition.
pub(super) const HOOKS: &[&str] = &[
    "read_before_execution",
    "modify_before_serialization",
    "read_before_serialization",
    "read_after_serialization",
    "modify_before_retry_loop",
    "read_before_attempt",
    "modify_before_signing",
    "read_before_signing",
    "read_after_signing",
    "modify_before_transmit",
    "read_before_transmit",
    "read_after_transmit",
    "modify_before_deserialization",
    "read_before_deserialization",
    "read_after_deserialization",
    "modify_before_attempt_completion",
    "read_after_attempt",
    "modify_before_completion",
    "read_after_execution",
];

/// Expected invocation count for a hook in a given scenario.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum Expect {
    /// Never invoked.
    Never,
    /// Exactly once per execution.
    Once,
    /// Once per attempt actually started, measured by the `read_before_attempt` count.
    ///
    /// A literal would not do: a consumer detects a missing invocation with an equality against the
    /// attempt count, so a fix with the wrong cardinality would silently corrupt its metrics.
    PerAttempt,
    /// Once per attempt that received a response, else zero.
    PerResponse,
}
use Expect::*;

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[derive(Copy, Clone)]
pub(super) enum Connector {
    Ok,
    Fail,
    FailThenOk,
    /// Never responds, so a timeout is the only way the attempt ends.
    Never,
}

pub(super) struct Scenario {
    pub(super) name: &'static str,
    pub(super) connector: Connector,
    pub(super) timeout: Option<TimeoutConfig>,
    pub(super) retry: Option<BoundedRetryStrategy>,
    /// Abandon the future instead of letting the operation finish, to pin caller-initiated
    /// cancellation behaviour.
    pub(super) caller_abandons: bool,
    /// Attempts expected to start, i.e. the `read_before_attempt` count.
    pub(super) attempts: usize,
    /// Attempts expected to receive a response.
    pub(super) responses: usize,
    /// Expected count for each hook in [`HOOKS`] order.
    pub(super) expected: [Expect; 19],
    /// Expected `Observed` for every finalizer invocation, in invocation order.
    pub(super) expected_observations: Vec<(&'static str, Observed)>,
}

/// Builds an expectation row from the per-stage groups, so the scenarios read as a table.
pub(super) fn row(
    prologue: Expect,
    attempt: Expect,
    deserialization: Expect,
    attempt_finalizers: Expect,
    execution_finalizers: Expect,
) -> [Expect; 19] {
    [
        // prologue: read_before_execution .. modify_before_retry_loop
        prologue,
        prologue,
        prologue,
        prologue,
        prologue,
        // attempt: read_before_attempt .. read_before_transmit
        attempt,
        attempt,
        attempt,
        attempt,
        attempt,
        attempt,
        // deserialization: read_after_transmit .. read_after_deserialization
        deserialization,
        deserialization,
        deserialization,
        deserialization,
        // finalizers
        attempt_finalizers,
        attempt_finalizers,
        execution_finalizers,
        execution_finalizers,
    ]
}

pub(super) fn attempt_timeout(millis: u64) -> Option<TimeoutConfig> {
    Some(
        TimeoutConfig::builder()
            .operation_attempt_timeout(Duration::from_millis(millis))
            .build(),
    )
}

pub(super) fn operation_timeout(millis: u64) -> Option<TimeoutConfig> {
    Some(
        TimeoutConfig::builder()
            .operation_timeout(Duration::from_millis(millis))
            .build(),
    )
}

/// Every finalizer in one attempt plus the execution, all observing the same thing.
pub(super) fn observations(
    attempt: Observed,
    execution: Observed,
) -> Vec<(&'static str, Observed)> {
    vec![
        ("modify_before_attempt_completion", attempt),
        ("read_after_attempt", attempt),
        ("modify_before_completion", execution),
        ("read_after_execution", execution),
    ]
}

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "success",
            connector: Connector::Ok,
            timeout: None,
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 1,
            expected: row(Once, PerAttempt, PerResponse, PerAttempt, Once),
            expected_observations: observations(Observed::Success, Observed::Success),
        },
        Scenario {
            name: "retry then success",
            connector: Connector::FailThenOk,
            timeout: None,
            retry: Some(BoundedRetryStrategy {
                max_attempts: 2,
                delay: None,
                initial_delay: None,
            }),
            caller_abandons: false,
            attempts: 2,
            responses: 1,
            expected: row(Once, PerAttempt, PerResponse, PerAttempt, Once),
            // The first attempt genuinely failed; the second succeeded.
            expected_observations: vec![
                (
                    "modify_before_attempt_completion",
                    Observed::ConnectorFailure,
                ),
                ("read_after_attempt", Observed::ConnectorFailure),
                ("modify_before_attempt_completion", Observed::Success),
                ("read_after_attempt", Observed::Success),
                ("modify_before_completion", Observed::Success),
                ("read_after_execution", Observed::Success),
            ],
        },
        Scenario {
            name: "transport error",
            connector: Connector::Fail,
            timeout: None,
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(
                Observed::ConnectorFailure,
                Observed::ConnectorFailure,
            ),
        },
        Scenario {
            name: "attempt timeout",
            connector: Connector::Never,
            timeout: attempt_timeout(200),
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(Observed::Timeout, Observed::Timeout),
        },
        Scenario {
            name: "operation timeout mid-attempt",
            connector: Connector::Never,
            timeout: operation_timeout(200),
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(Observed::Timeout, Observed::Timeout),
        },
        Scenario {
            name: "operation timeout during retry backoff",
            connector: Connector::Fail,
            timeout: operation_timeout(300),
            retry: Some(BoundedRetryStrategy {
                max_attempts: 10,
                delay: Some(Duration::from_secs(2)),
                initial_delay: None,
            }),
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            // The attempt itself failed on the connector before the operation budget expired, so
            // the attempt pair sees the real error and only the execution pair sees the timeout.
            expected_observations: observations(Observed::ConnectorFailure, Observed::Timeout),
        },
        Scenario {
            // The negative control for the in-flight marker: the operation budget expires while
            // the initial request is still stalled, so no attempt ever starts. The attempt-level
            // finalizers must NOT fire -- `read_after_attempt` is owed only once
            // `read_before_attempt` has run -- while the execution-level pair still must. A fix
            // that settled the orphaned attempt unconditionally instead of consulting the marker
            // would fire them here and be caught.
            name: "operation timeout before any attempt starts",
            connector: Connector::Never,
            timeout: operation_timeout(200),
            retry: Some(BoundedRetryStrategy {
                max_attempts: 1,
                delay: None,
                initial_delay: Some(Duration::from_secs(2)),
            }),
            caller_abandons: false,
            attempts: 0,
            responses: 0,
            expected: row(Once, Never, Never, Never, Once),
            expected_observations: vec![
                ("modify_before_completion", Observed::Timeout),
                ("read_after_execution", Observed::Timeout),
            ],
        },
        Scenario {
            // A dropped future cannot run anything: `finally_attempt` and `finally_op` live inside
            // the `async move` block the caller just dropped, and Rust has no async drop. These
            // zeros are structural rather than a gap left to fix later -- the only way to change
            // them is to run the finalizers from `Drop`, which this design rejected. If this row
            // fails, that architectural choice changed.
            name: "caller abandons the future",
            connector: Connector::Never,
            timeout: None,
            retry: None,
            caller_abandons: true,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, Never, Never),
            expected_observations: vec![],
        },
    ]
}

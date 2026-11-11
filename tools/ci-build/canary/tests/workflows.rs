/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Invariants of the canary's three GitHub workflows.
//!
//! These are properties that YAML cannot express and review does not reliably catch, in files where
//! a mistake is quiet: a workflow is not compiled, and most of what is asserted here fails by doing
//! nothing rather than by erroring.
//!
//! It is a test in this crate rather than a standalone script because `check-tools` already runs
//! the crate's tests, so it needs no new CI wiring and cannot be forgotten.
//!
//! Two of these were written after the thing they check had already gone wrong:
//!
//! - The orchestrator's release-tag trigger initially had no arming gate, so merging it would have
//!   started a 24-hour, 60-leg watch on the next release as a side effect.
//! - The self-test's ticket-ARN template was validated for *leftover* placeholders rather than
//!   *missing* ones, so a secret carrying a hardcoded severity would have cut a severity-2 ticket
//!   from a self-test — which is what the pinned severity exists to prevent.
//!
//! The syntax is already checked elsewhere: the repository's pre-commit configuration runs
//! `check-yaml` and `pretty-format-yaml`. Nothing here duplicates that.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_yaml::Value;

const WORKFLOWS: &[&str] = &["build-canary", "post-publish-watch", "paging-selftest"];

fn workflow_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .join(".github/workflows")
}

fn load(name: &str) -> Value {
    let path = workflow_dir().join(format!("{name}.yml"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

fn raw(name: &str) -> String {
    std::fs::read_to_string(workflow_dir().join(format!("{name}.yml"))).expect("readable")
}

/// The jobs of a workflow, by name.
fn jobs(wf: &Value) -> BTreeMap<String, &Value> {
    wf.get("jobs")
        .and_then(Value::as_mapping)
        .expect("a workflow has jobs")
        .iter()
        .map(|(k, v)| (k.as_str().expect("a job name").to_string(), v))
        .collect()
}

fn steps(job: &Value) -> Vec<&Value> {
    job.get("steps")
        .and_then(Value::as_sequence)
        .map(|s| s.iter().collect())
        .unwrap_or_default()
}

fn step_name(step: &Value) -> String {
    step.get("name")
        .or_else(|| step.get("uses"))
        .and_then(Value::as_str)
        .unwrap_or("(unnamed)")
        .to_string()
}

/// The `on:` mapping. YAML 1.1 reads a bare `on` as the boolean `true`, which is why this is not
/// simply a string lookup.
fn triggers(wf: &Value) -> Vec<String> {
    let on = wf
        .get(Value::Bool(true))
        .or_else(|| wf.get("on"))
        .expect("a workflow has triggers");
    on.as_mapping()
        .expect("triggers are a mapping")
        .keys()
        .map(|k| k.as_str().expect("a trigger name").to_string())
        .collect()
}

/// A workflow input reaching a `run:` block through `${{ }}` is substituted into the command *text*
/// before any shell sees it, so a value carrying a quote and `$(...)` executes on the runner. Every
/// input must arrive through `env` instead.
///
/// This matters more now than when it was first written: two of these workflows assume an AWS role,
/// so an injection here reaches AWS rather than just the runner.
#[test]
fn no_workflow_interpolates_into_a_run_block() {
    for name in WORKFLOWS {
        let wf = load(name);
        for (job_name, job) in jobs(&wf) {
            for step in steps(job) {
                if let Some(run) = step.get("run").and_then(Value::as_str) {
                    assert!(
                        !run.contains("${{"),
                        "{name}.yml: {job_name}/{} interpolates into its run block",
                        step_name(step)
                    );
                }
            }
        }
    }
}

/// A job without a timeout inherits GitHub's six-hour default. For a leg that would mean the
/// 15-minute dispatch tick could be outlived by its own legs; for the floor tier it would mean a
/// stuck sleep holding a runner for six hours.
#[test]
fn every_job_bounds_its_own_runtime() {
    for name in WORKFLOWS {
        let wf = load(name);
        for (job_name, job) in jobs(&wf) {
            let timeout = job.get("timeout-minutes").and_then(Value::as_u64);
            assert!(
                timeout.is_some_and(|t| t > 0 && t <= 360),
                "{name}.yml: {job_name} has no usable timeout-minutes ({timeout:?}); \
                 the platform cap is 360"
            );
        }
    }
}

/// `id-token: write` is what lets a job mint an OIDC token and assume a role. A job holding it
/// without assuming anything is unnecessary authority; a job assuming a role without it fails at
/// runtime with a message that does not mention the permission.
#[test]
fn oidc_permission_and_role_assumption_go_together() {
    for name in WORKFLOWS {
        let wf = load(name);
        for (job_name, job) in jobs(&wf) {
            let has_permission = job
                .get("permissions")
                .and_then(|p| p.get("id-token"))
                .and_then(Value::as_str)
                == Some("write");
            let assumes = steps(job).iter().any(|s| {
                s.get("uses")
                    .and_then(Value::as_str)
                    .is_some_and(|u| u.contains("configure-aws-credentials"))
            });
            assert_eq!(
                has_permission, assumes,
                "{name}.yml: {job_name} has id-token:write={has_permission} but \
                 assumes a role={assumes}; these must agree"
            );
        }
    }
}

/// Internal ticket routing — the integration name, the category, the resolver group — must never be
/// written into a file in a public repository. It would tell any reader which internal queue to aim
/// at, and there is no precedent for any of it here.
///
/// The alarm action arrives as a template from a secret instead.
#[test]
fn no_workflow_contains_internal_ticket_routing() {
    // Split so this file does not itself contain the literals it bans.
    let banned = [
        concat!("cwa", "-internal"),
        concat!("arn:aws:", "cloudwatch::"),
        concat!("SDKs", "+and+Tools"),
    ];
    for name in WORKFLOWS {
        let text = raw(name);
        for needle in banned {
            assert!(
                !text.contains(needle),
                "{name}.yml contains internal ticket routing ({needle:?}); \
                 it belongs in a secret"
            );
        }
    }
}

/// Merging must not arm the canary. The orchestrator triggers on a release tag, so without a gate
/// landing the file would start a 24-hour, 60-leg watch on the next release — before the
/// report-only window that exists to measure its false-positive rate has ever run.
#[test]
fn the_orchestrator_is_armed_by_a_variable_and_not_by_merging() {
    let wf = load("post-publish-watch");
    assert!(
        triggers(&wf).contains(&"push".to_string()),
        "the orchestrator is expected to trigger on a release tag"
    );

    let jobs = jobs(&wf);
    let entry = jobs
        .get("dispatch-tier")
        .expect("the orchestrator starts with the dispatch tier");
    let condition = entry
        .get("if")
        .and_then(Value::as_str)
        .expect("the entry job must be conditional");

    assert!(
        condition.contains("vars.BUILD_CANARY_ENABLED"),
        "the entry job is not gated on the arming variable: {condition:?}"
    );

    // Every later job must key on the entry job having actually run. `always()` alone would also
    // fire when the entry job was skipped for being unarmed, which would start the day of
    // dispatching the gate exists to prevent.
    for later in ["floor-tier", "record"] {
        let condition = jobs
            .get(later)
            .unwrap_or_else(|| panic!("expected a {later} job"))
            .get("if")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{later} must be conditional"));
        assert!(
            condition.contains("needs.dispatch-tier.result"),
            "{later} does not key on whether the dispatch tier ran: {condition:?}"
        );
    }
}

/// The floor tier is 24 hourly cycles. As a matrix, "one at a time" is the only thing making it
/// hourly rather than 24 simultaneous dispatches, and a gap in the hours would silently shorten the
/// window.
#[test]
fn the_floor_tier_is_twenty_four_cycles_run_one_at_a_time() {
    let wf = load("post-publish-watch");
    let floor = jobs(&wf)
        .get("floor-tier")
        .copied()
        .expect("a floor tier job");
    let strategy = floor.get("strategy").expect("a matrix strategy");

    let hours: Vec<u64> = strategy
        .get("matrix")
        .and_then(|m| m.get("hour"))
        .and_then(Value::as_sequence)
        .expect("an hour matrix")
        .iter()
        .map(|h| h.as_u64().expect("an hour number"))
        .collect();
    assert_eq!(
        hours,
        (1..=24).collect::<Vec<u64>>(),
        "the floor tier must cover hours 1..24 with no gaps"
    );

    assert_eq!(
        strategy.get("max-parallel").and_then(Value::as_u64),
        Some(1),
        "without max-parallel 1 the hours run at once and the tier is not hourly at all"
    );
    // One hour failing for its own reasons says nothing about the next, and cancelling the
    // remaining 23 would turn a single transient into a lost day of checking.
    assert_eq!(
        strategy.get("fail-fast").and_then(Value::as_bool),
        Some(false),
        "fail-fast would let one bad hour cancel the rest of the window"
    );

    let timeout = floor
        .get("timeout-minutes")
        .and_then(Value::as_u64)
        .expect("a timeout");
    assert!(
        timeout > 60,
        "an hour-long sleep needs a timeout above 60 minutes, got {timeout}"
    );
}

/// The self-test exists to cut tickets nobody asked for. Cutting high-severity tickets
/// unnecessarily gets an application's ticketing access revoked, so the severity is pinned in
/// reviewed code and must not be selectable at dispatch time.
#[test]
fn the_self_test_pins_its_severity_and_runs_only_by_hand() {
    let text = raw("paging-selftest");
    assert!(
        text.contains("SEVERITY=5"),
        "the self-test must pin severity 5 in the script"
    );
    assert!(
        !text.contains("inputs.severity"),
        "severity must not be a dispatch input"
    );

    let wf = load("paging-selftest");
    assert_eq!(
        triggers(&wf),
        ["workflow_dispatch"],
        "the self-test must never be triggered automatically"
    );
}

/// The template's placeholders must be required to be *present* before substitution. Checking only
/// for leftovers afterwards passes trivially for a template that hardcoded a value: the
/// substitution is a no-op and nothing remains. A hardcoded severity would then defeat the pin
/// above, and a hardcoded dedupe key would collapse every session into one ticket.
#[test]
fn the_self_test_requires_both_arn_placeholders_before_substituting() {
    let text = raw("paging-selftest");
    for placeholder in ["{severity}", "{dedupe}"] {
        assert!(
            text.contains(placeholder),
            "the ARN template substitution must handle {placeholder}"
        );
    }
    assert!(
        text.contains("must contain ${PLACEHOLDER}"),
        "the self-test must refuse a template that is missing a placeholder, \
         not merely one with leftovers"
    );
}

/// The metrics and alarm roles are disjoint on purpose: one writes datapoints, the other manages
/// the alarm evaluating them. A single run holding both would undo that split.
#[test]
fn the_self_test_never_holds_both_roles_at_once() {
    let wf = load("paging-selftest");
    let job = jobs(&wf).get("selftest").copied().expect("a selftest job");

    let condition_for = |name: &str| -> String {
        steps(job)
            .iter()
            .find(|s| step_name(s) == name)
            .unwrap_or_else(|| panic!("expected a {name:?} step"))
            .get("if")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name:?} must be conditional"))
            .to_string()
    };

    let metrics = condition_for("Assume the metrics role");
    let alarms = condition_for("Assume the alarm role");
    assert!(
        metrics.contains("== 'fire'"),
        "only `fire` writes a datapoint, so only it should take the metrics role: {metrics:?}"
    );
    assert!(
        alarms.contains("!= 'fire'"),
        "every action but `fire` manages the alarm: {alarms:?}"
    );
}

/// Every AWS step must be skipped when its secret is absent, so the whole thing is inert before the
/// stack is deployed rather than failing on an empty `role-to-assume`.
#[test]
fn aws_steps_are_skipped_when_unconfigured() {
    let wf = load("build-canary");
    let leg = jobs(&wf).get("leg").copied().expect("a leg job");
    for name in ["Assume the metrics role", "Record the verdict"] {
        let condition = steps(leg)
            .iter()
            .find(|s| step_name(s) == name)
            .unwrap_or_else(|| panic!("expected a {name:?} step"))
            .get("if")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name:?} must be conditional"));
        assert!(
            condition.contains("steps.metrics.outputs.configured == 'true'"),
            "{name:?} is not gated on the metrics role being configured: {condition:?}"
        );
    }
}

/// A leg has to report before GitHub kills its job, or it emits nothing at all — and an absent
/// datapoint cannot raise an alarm, which is worse than a red one. The leg tracks its own budget,
/// so the job's timeout only has to leave room for it.
#[test]
fn a_leg_job_outlives_the_legs_own_budget() {
    let wf = load("build-canary");
    let leg = jobs(&wf).get("leg").copied().expect("a leg job");
    let job_timeout = leg
        .get("timeout-minutes")
        .and_then(Value::as_u64)
        .expect("a timeout");
    let own_budget = canary::leg::BUDGET.as_secs() / 60;

    assert!(
        job_timeout >= own_budget,
        "the job's {job_timeout} minutes is under the leg's own {own_budget}-minute budget, \
         so GitHub would kill it before it could report"
    );
    // And the whole thing has to fit inside the watcher's tick, or a leg could outlive the tick
    // that dispatched it and be re-dispatched alongside itself.
    let tick = canary::watch::INTERVAL.as_secs() / 60;
    assert!(
        job_timeout <= tick,
        "the job's {job_timeout} minutes exceeds the {tick}-minute dispatch tick"
    );
}

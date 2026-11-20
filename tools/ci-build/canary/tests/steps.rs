/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The canary's workflow steps, run.
//!
//! See `common/mod.rs` for why this exists alongside the invariants in `workflows.rs`: two defects
//! got past a careful reading of the YAML and were obvious the first time a step executed.
//!
//! Every test here is a property of a *step*, not of the file it lives in. Where the property is
//! "this value never reaches a public log", the assertion is one that cannot pass vacuously — an
//! empty log is not evidence of absence.

mod common;

use common::{bash_available, pinned, Outcome, StepRunner};

/// The account id in the stand-in ticket template. Nothing real, and the point of most of these
/// tests is that it does not appear anywhere a person could read it.
const ACCOUNT: &str = "111122223333";

const TEMPLATE: &str =
    "arn:aws:cloudwatch::111122223333:action/x/y/z/Sev{severity}+ticket+for+{dedupe}/abc";

const TAG: &str = "release-2026-11-16";

/// A stand-in `aws` for the arming steps.
///
/// `MODE` decides what the action readback returns, which is the question those steps turn on. The
/// template is filled per alarm name, as CloudWatch would: the orchestrator arms two alarms whose
/// dedupe keys differ, and a stub returning one value for both makes the induced alarm's comparison
/// fail and look like a defect in the step.
fn arming_stub(severity: &str) -> String {
    format!(
        r#"
printf '%s\n' "$*" >> "${{CALLS:?}}"
name=""
prev=""
for a in "$@"; do
  [[ "$prev" == "--alarm-names" ]] && name="$a"
  prev="$a"
done
filled="arn:aws:cloudwatch::{ACCOUNT}:action/x/y/z/Sev{severity}+ticket+for+${{name}}/abc"
case "$*" in
  *"describe-alarms --alarm-name-prefix"*)
    # What the previous-release listing returns. Empty is the first release; `None` is what
    # `--output text` prints for a null; tab-separated names are the ordinary case. All three go
    # through the same loop, and two of them used to end the step before it armed anything.
    printf '%s\n' "${{EXISTING:-}}"
    ;;
  *"length(MetricAlarms)"*) echo "1" ;;
  *"AlarmActions[0]"*)
    case "${{MODE}}" in
      exact)   printf '%s\n' "$filled" ;;
      spaces)  printf '%s\n' "${{filled//+/ }}" ;;
      pct)     printf '%s\n' "${{filled//+/%20}}" ;;
      wrong)   printf '%s\n' "arn:aws:cloudwatch::999988887777:action/other/Sev5+ticket/zzz" ;;
      none)    printf 'None\n' ;;
      sevtypo) printf '%s\n' "${{filled/Sev{severity}/Sev5}}" ;;
      *) echo "unknown MODE ${{MODE}}" >&2; exit 65 ;;
    esac
    ;;
  *"describe-alarms"*)
    # Whatever was selected, answered as the CLI would: if the query asked for `AlarmActions`, the
    # action is in the response. Without that a step teeing this output leaks nothing here and the
    # tests below cannot tell a scrubbed readback from an unscrubbed one.
    case "$*" in
      *AlarmActions*) echo "{{\"State\":\"OK\",\"Actions\":[\"$filled\"],\"MissingData\":\"notBreaching\",\"Dimensions\":[]}}" ;;
      *) echo '{{"State":"OK","MissingData":"notBreaching","Dimensions":[]}}' ;;
    esac
    ;;
  *"put-metric-alarm"*) ;;
  *"delete-alarms"*) ;;
  *) echo "unexpected aws call: $*" >&2; exit 64 ;;
esac
"#
    )
}

/// The orchestrator's arming step, with the action coming back as `mode` describes.
fn arm_watch(mode: &str) -> Outcome {
    arm_watch_with(mode, "")
}

/// The same, with `existing` standing in for what the previous-release alarm listing returns.
fn arm_watch_with(mode: &str, existing: &str) -> Outcome {
    let runner = StepRunner::new("post-publish-watch", "alarms", "Arm");
    let severity = pinned(runner.script(), "SEVERITY");
    runner
        .stub_aws(&arming_stub(&severity))
        .env("EXISTING", existing)
        .env("MODE", mode)
        .env("TAG", TAG)
        .env("TICKET_ARN_TEMPLATE", TEMPLATE)
        .run()
}

/// The paging self-test's arming step, which drives the same chain by hand.
fn arm_selftest(mode: &str) -> Outcome {
    let runner = StepRunner::new("paging-selftest", "selftest", "Arm");
    let severity = pinned(runner.script(), "SEVERITY");
    runner
        .stub_aws(&arming_stub(&severity))
        .env("MODE", mode)
        .env("ALARM", "paging-selftest-abc123")
        .env("TICKET_ARN_TEMPLATE", TEMPLATE)
        .run()
}

/// The defect that prompted all of this.
///
/// The previous-alarm delete filtered blank lines with `grep -v`, which exits 1 when it matches
/// nothing. Under `set -o pipefail` that failed the pipeline and `set -e` ended the step — before
/// `put-metric-alarm`. Nothing matches on the first release, so no alarm was ever created, and the
/// next release had nothing to delete either: the canary could not arm, ever.
#[test]
fn arming_works_when_there_is_no_previous_alarm_to_delete() {
    if !bash_available() {
        return;
    }
    let run = arm_watch("exact");
    assert!(
        run.succeeded(),
        "an empty delete list must not end the step: {}",
        run.log
    );
    assert!(run.output.contains("armed=true"));
    assert!(
        run.calls.contains("put-metric-alarm"),
        "it has to have actually armed something, not merely exited 0: {}",
        run.calls
    );
}

/// `--output text` prints `None` for a null, and `delete-alarms --alarm-names None` would fail --
/// taking the arming step with it, the same way the blank line used to.
#[test]
fn arming_survives_a_previous_alarm_listing_of_none() {
    if !bash_available() {
        return;
    }
    let run = arm_watch_with("exact", "None");
    assert!(
        run.succeeded(),
        "a `None` listing must not end the step: {}",
        run.log
    );
    assert!(run.output.contains("armed=true"));
    assert!(
        !run.calls.contains("delete-alarms --alarm-names None"),
        "it must not try to delete an alarm called None: {}",
        run.calls
    );
}

/// And the ordinary case still deletes. A previous release's alarm keeps evaluating a series
/// nothing writes to, and occupies a dedupe key this release may want.
#[test]
fn arming_deletes_the_previous_releases_alarms() {
    if !bash_available() {
        return;
    }
    let run = arm_watch_with(
        "exact",
        "build-canary-release-2026-11-09\tbuild-canary-induced-release-2026-11-09",
    );
    assert!(run.succeeded(), "{}", run.log);
    for old in [
        "build-canary-release-2026-11-09",
        "build-canary-induced-release-2026-11-09",
    ] {
        assert!(
            run.calls
                .contains(&format!("delete-alarms --alarm-names {old}")),
            "{old} was not deleted: {}",
            run.calls
        );
    }
}

#[test]
fn arming_reports_the_action_matching_without_publishing_it() {
    if !bash_available() {
        return;
    }
    let run = arm_watch("exact");
    assert!(run.succeeded());
    assert!(run.summary.contains("matches the filled template exactly"));
    // Both alarms, because the release alarm and the induced one are one mechanism.
    assert!(run
        .summary
        .contains("build-canary-induced-release-2026-11-16"));
    // The whole point: GitHub masks the secret, not a value derived from it.
    run.summary_lacks(ACCOUNT);
    run.log_lacks(ACCOUNT);
}

/// Whether CloudWatch returns a space as `+` or `%20` is undocumented, so both are normalised
/// before comparing. A cosmetic re-encoding must not stop the canary arming.
#[test]
fn arming_accepts_either_space_encoding() {
    if !bash_available() {
        return;
    }
    for mode in ["spaces", "pct"] {
        let run = arm_watch(mode);
        assert!(run.succeeded(), "{mode} should still arm: {}", run.log);
        assert!(run.output.contains("armed=true"));
        assert!(run.summary.contains("once space encoding is normalised"));
        run.log_lacks(ACCOUNT);
    }
}

/// An alarm carrying the wrong action is armed in name only, and the readback is what notices.
#[test]
fn arming_refuses_an_action_it_did_not_ask_for() {
    if !bash_available() {
        return;
    }
    for (mode, note) in [
        ("wrong", "a different account entirely"),
        ("none", "no action at all"),
        ("sevtypo", "a severity the step did not choose"),
    ] {
        let run = arm_watch(mode);
        assert_eq!(run.status, Some(1), "{note} must fail the step");
        assert!(run.log.contains("::error title=Wrong action::"), "{note}");
        // Neither the one that came back nor the one that was expected.
        run.log_lacks("999988887777");
        run.log_lacks(ACCOUNT);
        assert!(
            run.output.is_empty(),
            "{note} must not mark the release armed: {}",
            run.output
        );
    }
}

#[test]
fn the_self_test_arms_on_the_same_terms() {
    if !bash_available() {
        return;
    }
    let run = arm_selftest("exact");
    assert!(run.succeeded(), "{}", run.log);
    assert!(run.calls.contains("put-metric-alarm"));
    assert!(run.summary.contains("matches the filled template exactly"));
    run.summary_lacks(ACCOUNT);
    run.log_lacks(ACCOUNT);

    let wrong = arm_selftest("wrong");
    assert_eq!(wrong.status, Some(1));
    assert!(wrong.log.contains("::error title=Wrong action::"));
    wrong.log_lacks("999988887777");
}

/// A stand-in `aws` whose `put-metric-alarm` rejects the action, spelling it as `SHAPE` says.
///
/// The API rejects a well-formed ARN that points nowhere, and the CLI's error echoes the parameter
/// it rejected — into the public job log. The shapes matter because the CLI is free to re-encode
/// the value, which defeats a literal scrub and is what the fallback to the error code is for.
fn rejecting_stub() -> String {
    r#"
printf '%s\n' "$*" >> "${CALLS:?}"
case "$*" in
  *"put-metric-alarm"*)
    action=""
    prev=""
    for a in "$@"; do
      [[ "$prev" == "--alarm-actions" ]] && action="$a"
      prev="$a"
    done
    prefix='An error occurred (ValidationError) when calling the PutMetricAlarm operation: Invalid action: '
    case "${SHAPE}" in
      verbatim)    printf '%s%s\n' "$prefix" "$action" >&2 ;;
      spaces)      printf '%s%s\n' "$prefix" "${action//+/ }" >&2 ;;
      reencoded)   printf '%s%s\n' "$prefix" "${action//+/%20}" >&2 ;;
      nocode)      printf 'Invalid action: %s\n' "$action" >&2 ;;
      accountonly) printf 'Not authorized to publish to the topic owned by 111122223333\n' >&2 ;;
      clean)       printf 'An error occurred (ValidationError) when calling the PutMetricAlarm operation: Parameter Threshold must not be null\n' >&2 ;;
      *) echo "unknown SHAPE ${SHAPE}" >&2; exit 65 ;;
    esac
    exit 254
    ;;
  *"describe-alarms --alarm-name-prefix"*) printf '\n' ;;
  *"length(MetricAlarms)"*) echo "1" ;;
  *"AlarmActions[0]"*) printf 'None\n' ;;
  *"describe-alarms"*)
    case "$*" in
      *AlarmActions*) echo "{\"State\":\"OK\",\"Actions\":[\"$action\"]}" ;;
      *) echo '{"State":"OK"}' ;;
    esac
    ;;
  *"delete-alarms"*) ;;
  *) echo "unexpected aws call: $*" >&2; exit 64 ;;
esac
"#
    .to_string()
}

fn rejected(workflow: &str, job: &str, shape: &str) -> Outcome {
    StepRunner::new(workflow, job, "Arm")
        .stub_aws(&rejecting_stub())
        .env("SHAPE", shape)
        .env("TAG", TAG)
        .env("ALARM", "paging-selftest-abc123")
        .env("TICKET_ARN_TEMPLATE", TEMPLATE)
        .run()
}

/// A rejected action must not take the ticket routing into the log with it.
///
/// The shape checks on the filled template catch an unfilled placeholder and a string that is not
/// an ARN. They cannot catch an ARN that is well formed and points nowhere: that reaches the API,
/// and the error is not ours to shape.
#[test]
fn a_rejected_action_is_scrubbed_from_the_error() {
    if !bash_available() {
        return;
    }
    for (workflow, job) in [
        ("post-publish-watch", "alarms"),
        ("paging-selftest", "selftest"),
    ] {
        let run = rejected(workflow, job, "verbatim");
        assert_eq!(run.status, Some(1), "{workflow} must fail");
        assert!(run.calls.contains("put-metric-alarm"), "{workflow}");
        assert!(
            run.log.contains("::error title=Arming rejected::"),
            "{workflow} must say why: {}",
            run.log
        );
        run.log_lacks(ACCOUNT);
        run.log_lacks("ticket+for");
        assert!(
            run.log.contains("<redacted>"),
            "{workflow} should mark what it removed: {}",
            run.log
        );
    }
}

/// The CLI may spell the action differently from the way it was sent, which a literal scrub misses.
/// The space form is caught by normalising; the percent form falls back to the error code.
#[test]
fn a_reencoded_action_does_not_slip_past_the_scrub() {
    if !bash_available() {
        return;
    }
    for (workflow, job) in [
        ("post-publish-watch", "alarms"),
        ("paging-selftest", "selftest"),
    ] {
        let spaced = rejected(workflow, job, "spaces");
        assert_eq!(spaced.status, Some(1));
        spaced.log_lacks(ACCOUNT);
        spaced.log_lacks("arn:aws");
        assert!(
            spaced.log.contains("Invalid action"),
            "{workflow}: a form the scrub handles should keep the message, not degrade to the code"
        );

        for shape in ["reencoded", "nocode"] {
            let run = rejected(workflow, job, shape);
            assert_eq!(run.status, Some(1), "{workflow}/{shape}");
            run.log_lacks(ACCOUNT);
            run.log_lacks("arn:aws");
        }

        // An error naming only the account, with no ARN for the literal scrub to match.
        let account_only = rejected(workflow, job, "accountonly");
        assert_eq!(account_only.status, Some(1));
        account_only.log_lacks(ACCOUNT);
    }
}

/// And an error that never mentioned the action keeps its diagnosis, or the scrub would cost every
/// unrelated failure its explanation.
#[test]
fn an_unrelated_rejection_keeps_its_message() {
    if !bash_available() {
        return;
    }
    for (workflow, job) in [
        ("post-publish-watch", "alarms"),
        ("paging-selftest", "selftest"),
    ] {
        let run = rejected(workflow, job, "clean");
        assert_eq!(run.status, Some(1));
        assert!(
            run.log.contains("Threshold must not be null"),
            "{workflow} lost an unrelated diagnosis: {}",
            run.log
        );
    }
}

const ORCHESTRATOR_ROLE: &str = "arn:aws:iam::111122223333:role/orchestrator";
const METRICS_ROLE: &str = "arn:aws:iam::111122223333:role/metrics";

/// The arming job's configuration check, with whichever secrets `present` names.
fn configured(enabled: &str, orchestrator: &str, template: &str, metrics: &str) -> Outcome {
    StepRunner::new("post-publish-watch", "alarms", "Is the alarm configured")
        .env("ENABLED", enabled)
        .env("ORCHESTRATOR_ROLE", orchestrator)
        .env("TICKET_ARN_TEMPLATE", template)
        .env("METRICS_ROLE", metrics)
        .run()
}

#[test]
fn a_complete_paging_chain_is_configured_and_says_nothing() {
    if !bash_available() {
        return;
    }
    let run = configured("true", ORCHESTRATOR_ROLE, TEMPLATE, METRICS_ROLE);
    assert!(run.succeeded(), "{}", run.log);
    assert!(run.output.contains("configured=true"));
    assert!(
        run.log.is_empty(),
        "a working configuration should be silent: {}",
        run.log
    );
}

/// Enabled has to mean a verdict can reach a person.
///
/// The variable decides whether the orchestrator runs, not whether paging works, and the three
/// places that notice a missing secret all treated it as a notice. So the canary could be switched
/// on, run every leg, stay green and page nobody — the failure it exists to remove.
#[test]
fn enabling_the_canary_without_the_whole_chain_fails() {
    if !bash_available() {
        return;
    }
    for (name, orchestrator, template, metrics) in [
        (
            "BUILD_CANARY_ORCHESTRATOR_ROLE_ARN",
            "",
            TEMPLATE,
            METRICS_ROLE,
        ),
        (
            "BUILD_CANARY_TICKET_ARN_TEMPLATE",
            ORCHESTRATOR_ROLE,
            "",
            METRICS_ROLE,
        ),
        (
            "BUILD_CANARY_METRICS_ROLE_ARN",
            ORCHESTRATOR_ROLE,
            TEMPLATE,
            "",
        ),
    ] {
        let run = configured("true", orchestrator, template, metrics);
        assert_eq!(run.status, Some(1), "missing {name} must fail when enabled");
        assert!(
            run.log.contains("::error title=Paging chain incomplete"),
            "missing {name}: {}",
            run.log
        );
        assert!(run.log.contains(name), "it has to name {name}: {}", run.log);
        assert!(
            run.output.is_empty(),
            "missing {name} must not claim to be configured"
        );
    }
}

/// And report-only stays available while the variable is not yet true, which is a deliberate
/// rollout stage rather than a misconfiguration.
#[test]
fn an_unenabled_canary_falls_back_to_report_only() {
    if !bash_available() {
        return;
    }
    for value in ["", "false"] {
        let run = configured(value, "", "", "");
        assert!(run.succeeded(), "ENABLED={value:?}: {}", run.log);
        assert!(run.output.contains("configured=false"));
        assert!(run.log.contains("::notice title=No alarm"));
        assert!(
            !run.log.contains("::error"),
            "report-only is not an error: {}",
            run.log
        );
    }
}

#[test]
fn an_enabled_canary_with_nothing_set_names_every_missing_secret() {
    if !bash_available() {
        return;
    }
    let run = configured("true", "", "", "");
    assert_eq!(run.status, Some(1));
    for name in [
        "BUILD_CANARY_ORCHESTRATOR_ROLE_ARN",
        "BUILD_CANARY_TICKET_ARN_TEMPLATE",
        "BUILD_CANARY_METRICS_ROLE_ARN",
    ] {
        assert!(run.log.contains(name), "{name} unnamed: {}", run.log);
    }
}

/// A durable archive is deferred, and if it comes back it must not join the paging chain.
///
/// It answers a historical question and no page depends on it. Asserted against `$GITHUB_OUTPUT`
/// rather than the log, which a configured step leaves empty: nothing the step decides may name an
/// archive secret, so this fails the day one joins the chain.
#[test]
fn the_archive_is_not_part_of_the_enabled_state() {
    if !bash_available() {
        return;
    }
    let run = configured("true", ORCHESTRATOR_ROLE, TEMPLATE, METRICS_ROLE);
    assert!(run.succeeded());
    assert!(!run.log.contains("BUILD_CANARY_RECORD"));
    assert!(!run.output.contains("BUILD_CANARY_RECORD"));
}

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
//! The syntax is already checked elsewhere: the repository's pre-commit configuration runs
//! `check-yaml` and `pretty-format-yaml`. Nothing here duplicates that.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_yaml::Value;

const WORKFLOWS: &[&str] = &["build-canary", "post-publish-watch", "paging-selftest"];

/// Ticket severities anything here is allowed to cut.
///
/// A deliberately short list, because the failure it guards is a typo. `SEVERITY=25` or a severity
/// left empty by a bad edit would still substitute into the alarm action and cut tickets at
/// something nobody chose — and the first notice of that would be either a silent ticket or a
/// paged oncall, depending on which way the typo fell.
const SANCTIONED_SEVERITIES: &[&str] = &["5", "3", "2.5", "2"];

/// The severity a workflow pins in its script, as written.
///
/// Extracted rather than asserted against a literal, because the value legitimately changes as the
/// rollout proceeds. What must not change is that it is pinned at all.
fn pinned_severity(text: &str) -> String {
    let found: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("SEVERITY="))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one pinned SEVERITY= line, found {found:?}"
    );
    found[0].to_string()
}

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
/// landing the file would start a 24-hour, full-matrix watch on the next release — before the
/// report-only window that exists to measure its false-positive rate has ever run.
#[test]
fn the_orchestrator_is_armed_by_a_variable_and_not_by_merging() {
    let wf = load("post-publish-watch");
    assert!(
        triggers(&wf).contains(&"push".to_string()),
        "the orchestrator is expected to trigger on a release tag"
    );

    let jobs = jobs(&wf);

    // Found rather than named, so restructuring the orchestrator cannot quietly move the gate off
    // the entry point. Any job with no `needs` starts when the trigger fires.
    let entry: Vec<&String> = jobs
        .iter()
        .filter(|(_, j)| j.get("needs").is_none())
        .map(|(name, _)| name)
        .collect();
    assert!(!entry.is_empty(), "something must start the workflow");

    for name in &entry {
        let condition = jobs[*name]
            .get("if")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("entry job {name} must be conditional"));
        assert!(
            condition.contains("vars.BUILD_CANARY_ENABLED"),
            "entry job {name} is not gated on the arming variable: {condition:?}"
        );
    }

    // And every other job must key on something upstream, so that an entry job skipped for being
    // unarmed cannot leave a later tier running on its own.
    for (name, job) in &jobs {
        if entry.contains(&name) {
            continue;
        }
        let condition = job
            .get("if")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name} must be conditional"));
        assert!(
            condition.contains("needs."),
            "{name} does not key on an upstream job: {condition:?}"
        );
    }
}

/// The alarm has to exist before any leg can report. A verdict that lands first is a datapoint on a
/// series nothing is evaluating, which is silence where a page should be — so the arming job must
/// be upstream of the dispatch tier, not beside it.
#[test]
fn the_alarm_is_armed_before_any_leg_runs() {
    let wf = load("post-publish-watch");
    let jobs = jobs(&wf);
    let alarms = jobs.get("alarms").expect("an alarms job");
    let dispatch = jobs.get("dispatch-tier").expect("a dispatch tier");

    let needs = serde_yaml::to_string(
        dispatch
            .get("needs")
            .expect("dispatch-tier needs something"),
    )
    .expect("serialisable");
    assert!(
        needs.contains("alarms"),
        "the dispatch tier must wait for the alarm: needs = {needs}"
    );

    // And the arming job must read the alarm back rather than trusting the API call, because it is
    // the one job whose own failure no alarm can report.
    let text = raw("post-publish-watch");
    assert!(
        text.contains("describe-alarms --alarm-names"),
        "the arming job must read the alarm back after creating it"
    );

    // Severity in reviewed code, not inside the secret, so that raising it is a reviewed edit
    // rather than a secret rotation nobody sees. The value legitimately changes as the rollout
    // proceeds — 2.5 while the chain is being proven, 2 once a false-positive rate has been
    // measured — so what is asserted is that it is pinned and sane, not what it equals.
    let severity = pinned_severity(&text);
    assert!(
        SANCTIONED_SEVERITIES.contains(&severity.as_str()),
        "arming severity {severity:?} is not one of {SANCTIONED_SEVERITIES:?}; a typo here \
         would cut tickets at a severity nobody chose"
    );
    assert!(
        alarms.get("environment").and_then(Value::as_str) == Some("canary-orchestrator"),
        "arming needs the orchestrator environment, since the alarm role is gated on it"
    );
}

/// A break we rigged ourselves has to page on its own alarm, apart from a real one.
///
/// A ticket's dedupe key is the name of the alarm that cut it, and there is one alarm per series.
/// So if a rigged leg and a real break shared an alarm, a genuine break landing during a test
/// release would collapse into the test's ticket and become invisible — the exact failure this
/// canary exists to prevent, reintroduced by the act of testing it.
///
/// This is the half that lives in YAML. The other half — that a rigged leg writes to a different
/// series at all — is asserted in `metric`, and the two have to agree or the alarm watches a series
/// nothing writes to, which looks identical to a release with nothing wrong.
#[test]
fn a_rigged_break_is_armed_apart_from_a_real_one() {
    let text = raw("post-publish-watch");

    let assigned = |var: &str| -> String {
        let prefix = format!("{var}=");
        let found: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix(&prefix))
            .collect();
        assert_eq!(found.len(), 1, "expected one {var}= line, found {found:?}");
        found[0].trim_matches('"').to_string()
    };
    let real = assigned("ALARM");
    let rigged = assigned("INDUCED_ALARM");

    assert_ne!(
        real, rigged,
        "one alarm name means one dedupe key, so a real break would dedupe into the test's ticket"
    );
    for name in [&real, &rigged] {
        // Per release, so a straggler from a superseded release lands on a series whose alarm has
        // been deleted rather than paging about a release nobody is shipping.
        assert!(
            name.contains("${TAG}"),
            "alarm name {name:?} is not per-release"
        );
    }

    // The alarm role is scoped by name prefix, so an alarm outside it is denied at
    // `put-metric-alarm` — which fails the arming job rather than failing quietly, but only after
    // the prefix has been got wrong in the first place.
    let delete_prefix = text
        .split_whitespace()
        .skip_while(|w| *w != "--alarm-name-prefix")
        .nth(1)
        .expect("the arming job deletes the previous release's alarms by prefix")
        .to_string();
    for name in [&real, &rigged] {
        assert!(
            name.starts_with(&delete_prefix),
            "alarm {name:?} is outside {delete_prefix:?}, so it is neither permitted by the role \
             nor cleaned up by the next release"
        );
    }

    // Exactly two alarms are created, and they are those two.
    let created: Vec<String> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("--alarm-name "))
        .map(|rest| rest.trim().trim_end_matches('\\').trim().to_string())
        .collect();
    assert_eq!(
        created,
        ["\"$ALARM\"", "\"$INDUCED_ALARM\""],
        "both alarms must be created, and nothing else"
    );

    // One of them, and only one, watches the series a rigged leg writes to. The dimension name is
    // the crate's, so the series the legs emit to and the series the alarm evaluates cannot drift
    // apart silently.
    let marker = format!("Name={},Value=true", canary::metric::INDUCED);
    assert_eq!(
        text.lines().filter(|l| l.contains(&marker)).count(),
        1,
        "exactly one alarm should carry {marker}"
    );

    // And its description has to say it is a test, because on a Sev2.5 that description is all the
    // responder sees. A deliberate ticket that reads like a real finding spends the responder's
    // trust, and the next real one gets read as another drill.
    let descriptions: Vec<&str> = text
        .lines()
        .filter(|l| l.trim().starts_with("--alarm-description "))
        .collect();
    assert_eq!(descriptions.len(), 2, "one description per alarm");
    assert_eq!(
        descriptions
            .iter()
            .filter(|d| d.contains("THIS IS A TEST"))
            .count(),
        1,
        "the rigged alarm's description must announce itself, and the real one must not"
    );
}

/// One `aws cloudwatch put-metric-alarm` invocation, as the flags that decide what it watches.
struct AlarmCommand {
    namespace: String,
    metric_name: String,
    /// The `Name=…,Value=…` arguments, in order, with shell quoting removed.
    dimensions: Vec<String>,
}

/// Split a command into arguments, keeping double-quoted arguments whole.
///
/// Enough of a shell for the arming step and no more: the only quoting it uses is `"…"` around
/// dimension arguments, which must survive as single arguments because that is what makes each one
/// a dimension rather than two.
fn arguments(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// Every `put-metric-alarm` invocation in a workflow, read out of the shipped YAML.
///
/// Reads the file rather than a copy of it, because the whole point is that these flags are a third
/// copy of something the crate also declares.
fn alarm_commands(text: &str) -> Vec<AlarmCommand> {
    let mut commands = Vec::new();
    let mut lines = text.lines();
    while let Some(first) = lines.next() {
        // The invocation itself, not a comment mentioning it: the arming step's prose refers to
        // `put-metric-alarm` too, and parsing that found no flags and failed confusingly.
        if !first
            .trim_start()
            .starts_with("aws cloudwatch put-metric-alarm")
        {
            continue;
        }
        // Backslash-continued: take lines until one does not end in a backslash.
        let mut whole = String::new();
        let mut line = first.to_string();
        loop {
            let trimmed = line.trim_end();
            let continues = trimmed.ends_with('\\');
            whole.push(' ');
            whole.push_str(trimmed.trim_end_matches('\\').trim());
            if !continues {
                break;
            }
            match lines.next() {
                Some(next) => line = next.to_string(),
                None => break,
            }
        }

        let args = arguments(&whole);
        let value_of = |flag: &str| -> String {
            let at = args
                .iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag} is missing from {whole:?}"));
            args.get(at + 1)
                .unwrap_or_else(|| panic!("{flag} has no value in {whole:?}"))
                .clone()
        };
        let dimensions = {
            let at = args
                .iter()
                .position(|a| a == "--dimensions")
                .unwrap_or_else(|| panic!("--dimensions is missing from {whole:?}"));
            args[at + 1..]
                .iter()
                .take_while(|a| !a.starts_with("--"))
                .cloned()
                .collect()
        };
        commands.push(AlarmCommand {
            namespace: value_of("--namespace"),
            metric_name: value_of("--metric-name"),
            dimensions,
        });
    }
    commands
}

/// Each alarm must watch the series a red leg actually writes.
///
/// That series' identity — namespace, metric name, dimension set — exists in three places: this
/// crate, the CDK stack, and the arming step's `put-metric-alarm` flags. The namespace is checked
/// against the CDK by a test in `metric.rs`, and the `Induced` dimension name is already taken from
/// the crate's constant. The rest of the arming step's copy was unchecked, and a mismatch there is
/// the quietest failure available to this canary: the legs emit, the alarm sits at `OK` for the
/// whole release, every job is green, and nothing pages. CloudWatch matches dimensions exactly, so
/// one dimension too many on the alarmed datapoint is enough to cause it.
///
/// The expectation is *generated* by the code the legs use, with the release tag passed in as the
/// same `${TAG}` the workflow substitutes — so what is compared is the real datapoint shape against
/// the real arming flags, not two literals written next to each other.
#[test]
fn the_alarms_watch_the_series_a_red_leg_writes() {
    use canary::metric;

    let kept = canary::toolchain::resolve(
        canary::toolchain::Version::new(1, 94, 1),
        canary::toolchain::Version::new(1, 86, 0),
    )
    .expect("the toolchain span should resolve")
    .kept;
    let legs = canary::scenarios::release_legs(&kept);
    let leg = legs.first().expect("the release matrix has legs").clone();
    let mut rigged = leg.clone();
    rigged.induced = Some("a requirement moved on purpose".to_string());

    // The alarmed datapoint is the one carrying the fewest dimensions; the other is the diagnosis
    // copy, which deliberately has more and which no alarm watches.
    let alarmed = |l: &canary::scenarios::Leg| -> metric::Datapoint {
        let written = metric::datapoints("${TAG}", l, Some(canary::classify::Verdict::CompileFail));
        assert_eq!(
            written.len(),
            2,
            "a red leg writes an alarmed and a diagnosis datapoint"
        );
        let fewest = written
            .iter()
            .min_by_key(|d| d.dimensions.len())
            .expect("datapoints")
            .clone();
        let most = written
            .iter()
            .max_by_key(|d| d.dimensions.len())
            .expect("datapoints");
        assert!(
            most.dimensions.len() > fewest.dimensions.len(),
            "the diagnosis datapoint must be distinguishable from the alarmed one"
        );
        fewest
    };

    let expected = [alarmed(&leg), alarmed(&rigged)];
    let armed = alarm_commands(&raw("post-publish-watch"));
    assert_eq!(
        armed.len(),
        2,
        "one alarm for a real break and one for a rigged one"
    );

    for (command, want) in armed.iter().zip(expected.iter()) {
        assert_eq!(
            command.namespace,
            metric::NAMESPACE,
            "the alarm watches a namespace the legs do not write to, so it can never fire"
        );
        assert_eq!(
            command.metric_name,
            metric::METRIC_NAME,
            "the alarm watches a metric the legs do not write, so it can never fire"
        );
        let want: Vec<String> = want
            .dimensions
            .iter()
            .map(|d| format!("Name={},Value={}", d.name, d.value))
            .collect();
        assert_eq!(
            command.dimensions, want,
            "the alarm's dimensions differ from the ones a red leg writes; CloudWatch matches the \
             set exactly, so this alarm would never see the datapoint"
        );
    }
}

/// `all` must mean the release matrix, not every leg the matrix can express.
///
/// The test-registry legs resolve a graph committed to this repository, so they can say nothing
/// about a release. Including them in `all` would cost twenty-five runs a release — one per floor
/// hour plus the publish window — re-proving a graph that only moves when somebody commits a change
/// to it. Worse, it would put their verdicts on the release's own alarmed series, so a deliberately
/// broken test family would page as though a published crate had broken.
///
/// Asserted here because the filter lives in YAML, where the Rust-side marker cannot reach it. The
/// marker and the filter have to agree, and nothing but this test says so.
#[test]
fn the_all_scenario_excludes_the_test_registry_legs() {
    let raw = raw("build-canary");
    let all_branch = raw
        .lines()
        .skip_while(|l| !l.contains("if want == 'all'"))
        .take(10)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !all_branch.is_empty(),
        "the matrix job must have an `all` branch"
    );
    assert!(
        all_branch.contains("test_registry"),
        "`all` must filter out the test-registry legs, or a release runs them: {all_branch}"
    );
    assert!(
        all_branch.contains("not l.get('test_registry')"),
        "and it must exclude them rather than select them: {all_branch}"
    );
}

/// Overlapping dispatches must queue, never cancel.
///
/// Two ticks that dispatch different manifest sets land in different concurrency groups and run
/// side by side. The case this pins down is a repeated dispatch of the *same* set, where
/// `cancel-in-progress` decides between two outcomes that are not symmetric. Queued, the second run
/// starts late and still reports every leg. Cancelled, the first run's legs die mid-flight: they
/// emit no datapoint and leave no report, so they arrive in the record as `unreported` — which is
/// the signal reserved for a leg that died silently, and is how a deliberate cancellation would get
/// filed as a defect in the canary.
///
/// Asserted because the setting is one word, and the difference between its two values is invisible
/// until ticks actually overlap.
#[test]
fn a_repeated_dispatch_queues_rather_than_cancelling_its_predecessor() {
    let wf = load("build-canary");
    let concurrency = wf
        .get("concurrency")
        .expect("the leg workflow must bound its own concurrency");

    assert_eq!(
        concurrency
            .get("cancel-in-progress")
            .and_then(Value::as_bool),
        Some(false),
        "cancelling in flight turns a leg that was deliberately superseded into a leg that looks \
         like it died silently"
    );

    // And the group has to carry the scenario, or two ticks dispatching *different* sets would
    // serialise behind each other for no reason.
    let group = concurrency
        .get("group")
        .and_then(Value::as_str)
        .expect("a concurrency group");
    assert!(
        group.contains("inputs.scenario"),
        "the group must be per scenario set, or unrelated dispatches block each other: {group:?}"
    );
    assert!(
        group.contains("inputs.tag"),
        "and per release, or one release's dispatches would queue behind another's: {group:?}"
    );
}

/// The floor hour's gate has to be able to see the previous release, and to run at all.
///
/// Two ways this degrades silently rather than breaking loudly, both of which have to be ruled out
/// in YAML because neither is visible from the Rust side.
///
/// Scoping the run listing to this release's tag — which the gate it replaced did, with
/// `--branch "$TAG"` — makes the baseline permanently unfindable. The gate then treats every red
/// leg as news and behaves exactly as the old rule did, on every release, with nothing to show that
/// anything is wrong.
///
/// And the gate is a command in this crate rather than a shell one-liner, because deciding from the
/// verdicts in two runs is not something to trust to `jq`. That needs a checkout and a toolchain in
/// a job whose only other steps are a sleep and an API call, so their absence would fail every floor
/// hour on the first real release and nowhere earlier.
#[test]
fn the_floor_gate_can_see_the_previous_release_and_can_run() {
    let wf = load("post-publish-watch");
    let job = jobs(&wf).get("floor-tier").copied().expect("a floor tier");
    let all_steps = steps(job);

    assert!(
        all_steps
            .iter()
            .any(|s| s.get("uses").and_then(Value::as_str) == Some("actions/checkout@v4")),
        "the gate runs a command from this repository, so the job has to check it out"
    );
    assert!(
        all_steps.iter().any(|s| s
            .get("uses")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("rust-toolchain"))),
        "and it needs a toolchain to build it"
    );

    let gate = all_steps
        .iter()
        .filter_map(|s| s.get("run").and_then(Value::as_str))
        .find(|r| r.contains("floor-gate"))
        .expect("the floor hour must decide with `floor-gate`");

    // The listing has to span releases, so it must not be narrowed to this one.
    let listing_line = gate
        .lines()
        .find(|l| l.contains("gh run list"))
        .expect("the gate lists runs");
    let listing_block: String = gate
        .lines()
        .skip_while(|l| !l.contains("gh run list"))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !listing_block.contains("--branch"),
        "scoping the listing to this tag makes the previous release unfindable, which silently \
         restores the rule this replaced: {listing_line:?}"
    );

    // And the decision must come from the verdicts, never from a run's conclusion — the whole
    // reason an infrastructure failure can no longer stop the window.
    assert!(
        !gate.contains("conclusion'] == 'failure'") && !gate.contains("conclusion\"] == \"failure"),
        "the gate must not read a run's conclusion: {gate}"
    );
}

/// A release that nothing checked has to be reported, and from the right credentials.
///
/// This is the one condition no verdict exists for, so the record job is the only thing positioned
/// to notice it. Noticing needs the metrics role; archiving needs the record role; and
/// `configure-aws-credentials` overwrites whatever came before it. So the order is load-bearing in
/// a way YAML cannot express: assume metrics, emit, then assume record. Reversed, the emit step
/// runs with credentials that cannot write a datapoint, and the one thing standing between an
/// unchecked release and silence fails with an access error.
#[test]
fn an_unchecked_release_is_reported_before_the_record_role_is_taken() {
    let wf = load("post-publish-watch");
    let job = jobs(&wf).get("record").copied().expect("a record job");
    let names: Vec<String> = steps(job)
        .iter()
        .map(|s| step_name(s).to_string())
        .collect();

    let at = |needle: &str| -> usize {
        names
            .iter()
            .position(|n| n.contains(needle))
            .unwrap_or_else(|| panic!("no step containing {needle:?} in {names:?}"))
    };

    let emit = at("never checked");
    let metrics = at("Assume the metrics role");
    let record = at("Assume the record role");

    assert!(
        metrics < emit,
        "the metrics role must be held when the datapoint is written"
    );
    assert!(
        emit < record,
        "the record role overwrites the metrics credentials, so it must come after the emit: \
         {names:?}"
    );

    // And the emission has to be able to run when the rest of the job has already gone wrong,
    // because "nothing ran" is itself a symptom of things having gone wrong.
    let emit_step = &steps(job)[emit];
    let condition = emit_step
        .get("if")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        condition.contains("always()"),
        "the unchecked-release report must survive an earlier failure: {condition:?}"
    );

    // The release must come from the folded record, never from a workflow input: a datapoint
    // recorded against a release the canary was not watching would land on an armed series and page
    // about the wrong thing.
    let run = emit_step.get("run").and_then(Value::as_str).unwrap_or("");
    assert!(
        run.contains("--decision"),
        "the emission must read the decision rather than take a release argument: {run:?}"
    );
    assert!(
        !run.contains("--release") && !run.contains("TAG"),
        "the release must not be passed in: {run:?}"
    );
}

/// An arming failure must not stop the release being checked. Checking without being able to page
/// still beats not checking, and the arming failure is loud in its own job. What the dispatch tier
/// does need is to know *which* release.
#[test]
fn an_arming_failure_does_not_stop_the_release_being_checked() {
    let wf = load("post-publish-watch");
    let condition = jobs(&wf)
        .get("dispatch-tier")
        .expect("a dispatch tier")
        .get("if")
        .and_then(Value::as_str)
        .expect("conditional");

    assert!(
        condition.contains("always()"),
        "the dispatch tier must run even when arming failed: {condition:?}"
    );
    assert!(
        condition.contains("needs.alarms.outputs.tag != ''"),
        "but only when the release is identified: {condition:?}"
    );
    // Specifically *not* keyed on the arming job's success.
    assert!(
        !condition.contains("needs.alarms.result == 'success'"),
        "keying on arming success would trade a day of checking for a paging gap: {condition:?}"
    );
}

/// The tag is resolved once and read everywhere else. Two independent `case` statements would be
/// two things to keep in step, and the one that drifted would watch the wrong release.
#[test]
fn the_release_tag_has_a_single_source() {
    let text = raw("post-publish-watch");
    assert_eq!(
        text.matches("does not look like release-*").count(),
        1,
        "the tag should be validated in exactly one place"
    );
    // A job cannot reference its own `needs`, so the producing job must use its own step output.
    assert!(
        text.contains("TAG: ${{ steps.resolve.outputs.tag }}"),
        "the arming job must read its own resolved tag"
    );
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

    // Pinned, not chosen at dispatch time. Cutting high-severity tickets unnecessarily gets an
    // application's ticketing access revoked, so this should be a reviewed edit rather than a
    // dropdown choice made at 2am.
    assert!(
        !text.contains("inputs.severity"),
        "severity must not be a dispatch input"
    );
    let severity = pinned_severity(&text);

    // The value may change as the rollout proceeds, but never to 2 here. A full Sev2 pages
    // immediately at any hour, and this workflow exists to cut tickets nobody asked for --
    // repeatedly, by hand. 2 is correct for the canary once it has earned it; it is never correct
    // for the self-test.
    assert_ne!(
        severity, "2",
        "the self-test must never cut a full Sev2: it pages immediately and these tickets are fake"
    );
    assert!(
        SANCTIONED_SEVERITIES.contains(&severity.as_str()),
        "severity {severity:?} is not one of {SANCTIONED_SEVERITIES:?}"
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

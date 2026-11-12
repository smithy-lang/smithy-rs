/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Turning a verdict into the datapoint an alarm can see.
//!
//! This is the join between the canary and the paging chain. Everything before it writes to a job
//! log; this is where a verdict becomes something that can wake someone.
//!
//! # One metric, two series
//!
//! ```text
//! Red  {Tag}                                            <- the alarm watches this
//! Red  {Tag, Manifest, Verdict, Leg, Toolchain, Target}  <- a responder reads this
//! ```
//!
//! Each distinct dimension set is a distinct series under one metric name, so the two never
//! interfere. The alarm needs a series it can threshold without caring *which* leg broke; a
//! responder needs to know exactly that. Trying to serve both from one series would mean either an
//! alarm per leg — one alarm and one ticket per leg, for a single break — or an undimensioned one that
//! says a release is broken and nothing about how.
//!
//! # Why `Tag` is a dimension and not a label
//!
//! It is what makes attribution structural rather than a property of timing. On one
//! release-agnostic series, "which release does this datapoint belong to?" would be answered by
//! whichever alarm happened to be armed when it landed. That breaks on cancellation: superseding a
//! release cancels the watcher, but the legs run in their own dispatched runs which the cancellation
//! cannot reach, so they keep emitting for a few minutes — by which time the next release's alarm is
//! armed, and an old leg's verdict would page about a release nobody is shipping. With `{Tag}`, a
//! straggler lands on a series whose alarm has been deleted, so it lands nowhere. That is the
//! intended outcome: the newer release provides fresh coverage over a newer graph.
//!
//! # Only a verdict emits
//!
//! A green leg writes nothing, and neither does a superseded one — it reached no verdict at all, so
//! it has no opinion to record. The alarm's `notBreaching` treatment is what turns that silence back
//! into `OK`, which is what lets an oncall close the ticket rather than waiting for the next release.
//!
//! Writing a `0` for a green leg would look more complete and be worse: it would make the absence of
//! a datapoint — the signature of a leg that died without reporting — indistinguishable from a leg
//! that ran and passed.
//!
//! # Every verdict, including `INFRA_FAIL`
//!
//! One severity for all five, which means a runner failure can wake someone. The retry policy
//! absorbs the transient cases — an `INFRA_FAIL` costs three attempts over six minutes before it is
//! ever reported — and the verdict is on the second series, so a responder sees which kind it was
//! before doing anything.

use std::fmt;

use serde::Serialize;

use crate::classify::Verdict;
use crate::scenarios::Leg;

/// The namespace every datapoint is written to.
///
/// Must equal `METRIC_NAMESPACE` in the CDK stack, which is the sole bound on the metrics role:
/// `PutMetricData` takes no resource-level permissions, so a condition on `cloudwatch:namespace` is
/// the only lever there is. [`tests::the_namespace_matches_the_cdk_stack`] checks the two agree.
///
/// That check matters more than it looks. A disagreement is denied by the namespace condition, so
/// the call fails and the verdict is never recorded — and a verdict that was never recorded cannot
/// raise an alarm, which is silence where a page should have been. The CDK's own comment says
/// nothing checks this; now something does.
pub const NAMESPACE: &str = "smithy-rs-build-canary";

/// The metric name. One name, two dimension sets.
pub const METRIC_NAME: &str = "Red";

/// CloudWatch rejects an empty dimension value, so a leg with no cross-compilation target has to say
/// something. `native` is that something.
pub const NATIVE: &str = "native";

/// One dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Dimension {
    pub name: String,
    pub value: String,
}

/// One datapoint, in the shape `aws cloudwatch put-metric-data --metric-data` wants.
// No `Eq`: the value is an `f64`, because that is what CloudWatch takes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Datapoint {
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    pub dimensions: Vec<Dimension>,
}

impl fmt::Display for Datapoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dims: Vec<String> = self
            .dimensions
            .iter()
            .map(|d| format!("{}={}", d.name, d.value))
            .collect();
        write!(f, "{}=1 {{{}}}", self.metric_name, dims.join(", "))
    }
}

fn dimension(name: &str, value: &str) -> Dimension {
    Dimension {
        name: name.to_string(),
        value: value.to_string(),
    }
}

fn datapoint(dimensions: Vec<Dimension>) -> Datapoint {
    Datapoint {
        metric_name: METRIC_NAME.to_string(),
        value: 1.0,
        unit: "Count".to_string(),
        dimensions,
    }
}

/// Marks a datapoint as coming from a manifest we rigged. See [`crate::scenarios::Scenario::induced`].
pub const INDUCED: &str = "Induced";

/// The two datapoints a red leg writes.
///
/// Returns empty for no verdict, which is how a green or superseded leg writes nothing.
///
/// # A rigged leg pages on its own series
///
/// A rigged leg's alarmed series is `Red {Tag, Induced=true}`, not `Red {Tag}`. The dedupe key a
/// ticket is cut under comes from the alarm, and there is one alarm per series, so sharing a series
/// would mean a real break landing during a test collapses into the test's ticket and becomes
/// invisible — which is the exact failure this whole canary exists to prevent, reintroduced by the
/// act of testing it.
///
/// The consequence is that the release alarm will not fire for a rigged leg, so the orchestrator
/// arms a second alarm against this series. Without that second alarm a rigged break pages nobody
/// and proves nothing, which is why the two are written and checked together.
pub fn datapoints(tag: &str, leg: &Leg, verdict: Option<Verdict>) -> Vec<Datapoint> {
    let Some(verdict) = verdict else {
        return vec![];
    };
    let mut tag_only = vec![dimension("Tag", tag)];
    let mut full = vec![
        dimension("Tag", tag),
        dimension("Manifest", &leg.manifest),
        dimension("Verdict", &verdict.to_string()),
        dimension("Leg", &leg.id),
        dimension("Toolchain", &leg.toolchain_version),
        dimension("Target", leg.target.as_deref().unwrap_or(NATIVE)),
    ];
    if leg.induced.is_some() {
        // `true` rather than the reason. The reason is prose, it is edited between releases, and a
        // dimension value is part of a series identity — so carrying it here would start a new
        // series every time the wording changed. The reason belongs on the report and in the
        // record, where there is room for it.
        for dims in [&mut tag_only, &mut full] {
            dims.push(dimension(INDUCED, "true"));
        }
    }
    vec![datapoint(tag_only), datapoint(full)]
}

/// The watcher, which asks for the legs.
pub const WATCHER: &str = "watcher";

/// The record job, which is the last thing to look at a release and so the only thing positioned to
/// notice that nothing looked at it.
pub const RECORD: &str = "record";

/// The datapoints a part of the canary other than a leg writes when it fails in a way that leaves a
/// release unchecked.
///
/// A leg reporting a verdict is the only thing that says anything about a release. So the failures
/// worth a datapoint here are the ones where **no verdict exists and none is coming**: the watcher
/// could not ask for the legs, or the legs were asked for and never ran. Either way the release
/// proceeds with no post-publish verification at all, which is the condition this whole canary
/// exists to remove, and which would otherwise be visible only as a red job nobody is watching.
///
/// **The alarmed series is byte-identical to a leg's**, which is the point: one alarm covers all of
/// it, so a crate being broken and a release never being checked reach the same oncall by the same
/// path. A test asserts that equality, because an alarm watching `{Tag}` would silently ignore a
/// series that differed by one dimension.
///
/// The diagnosis series is deliberately *not* the leg's shape. These failures have no manifest, no
/// toolchain and no target, and filling those with placeholders to force a common shape would put
/// six dimensions of nothing on a ticket. A different kind of emitter gets a different diagnosis:
/// which component noticed, and what it noticed.
pub fn component_datapoints(tag: &str, component: &str, reason: &str) -> Vec<Datapoint> {
    vec![
        datapoint(vec![dimension("Tag", tag)]),
        datapoint(vec![
            dimension("Tag", tag),
            dimension("Component", component),
            dimension("Reason", reason),
        ]),
    ]
}

/// Record a failure from somewhere other than a leg. Same contract as [`record`]: validated, then
/// emitted, and an emission failure is an error rather than a shrug.
pub fn record_component_failure(
    tag: &str,
    component: &str,
    reason: &str,
    emitter: &mut dyn Emitter,
) -> anyhow::Result<Vec<Datapoint>> {
    let datapoints = component_datapoints(tag, component, reason);
    validate(&datapoints)?;
    emitter.emit(NAMESPACE, &serde_json::to_string(&datapoints)?)?;
    Ok(datapoints)
}

/// Reject anything CloudWatch would reject, before the call rather than after.
///
/// A rejected `PutMetricData` is a verdict that never reached an alarm. Catching the avoidable cases
/// here means a bad dimension value fails with a message naming it, instead of an API error naming
/// the whole request.
pub fn validate(datapoints: &[Datapoint]) -> anyhow::Result<()> {
    // CloudWatch's documented maxima.
    const MAX_DIMENSIONS: usize = 30;
    const MAX_NAME: usize = 255;
    const MAX_VALUE: usize = 1024;

    for d in datapoints {
        anyhow::ensure!(
            d.dimensions.len() <= MAX_DIMENSIONS,
            "{} dimensions exceeds the limit of {MAX_DIMENSIONS}",
            d.dimensions.len()
        );
        for dim in &d.dimensions {
            anyhow::ensure!(
                !dim.name.is_empty() && dim.name.len() <= MAX_NAME,
                "dimension name {:?} is empty or too long",
                dim.name
            );
            // The one that would actually happen: a leg with no target, or an empty tag from an
            // unset variable.
            anyhow::ensure!(
                !dim.value.is_empty(),
                "dimension {} has an empty value, which CloudWatch rejects",
                dim.name
            );
            anyhow::ensure!(
                dim.value.len() <= MAX_VALUE,
                "dimension {} has a value longer than {MAX_VALUE}",
                dim.name
            );
        }
    }
    Ok(())
}

/// Writes datapoints somewhere. Injected so the shape can be tested without a credential.
pub trait Emitter {
    /// `metric_data` is the JSON array of datapoints.
    fn emit(&mut self, namespace: &str, metric_data: &str) -> anyhow::Result<()>;
}

/// Writes through the AWS CLI, which is present on every GitHub runner and picks up the job's OIDC
/// credentials from the environment with no code here.
///
/// The payload goes via a file rather than an argument. It is JSON containing a release tag and a
/// leg id, and passing it on the command line would put values this crate did not author into a
/// shell word — the same class of mistake as interpolating a workflow input into a `run:` block.
pub struct AwsCli {
    /// Where to write the payload. The leg's `RUNNER_TEMP` in CI.
    pub payload_dir: std::path::PathBuf,
    /// Print the command and payload instead of calling AWS.
    pub dry_run: bool,
}

impl Emitter for AwsCli {
    fn emit(&mut self, namespace: &str, metric_data: &str) -> anyhow::Result<()> {
        use anyhow::Context;

        std::fs::create_dir_all(&self.payload_dir)
            .with_context(|| format!("creating {}", self.payload_dir.display()))?;
        let path = self.payload_dir.join("metric-data.json");
        std::fs::write(&path, metric_data)
            .with_context(|| format!("writing {}", path.display()))?;

        let argv = vec![
            "cloudwatch".to_string(),
            "put-metric-data".to_string(),
            "--namespace".to_string(),
            namespace.to_string(),
            "--metric-data".to_string(),
            format!("file://{}", path.display()),
        ];

        if self.dry_run {
            eprintln!("would run: aws {}", argv.join(" "));
            eprintln!("with payload:\n{metric_data}");
            return Ok(());
        }

        let out = std::process::Command::new("aws")
            .args(&argv)
            .output()
            .context("running aws cloudwatch put-metric-data")?;
        anyhow::ensure!(
            out.status.success(),
            "put-metric-data failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    }
}

/// Record a leg's verdict, if it reached one.
///
/// Returns the datapoints written, so the caller can log them. Empty means the leg had no opinion.
pub fn record(
    tag: &str,
    leg: &Leg,
    verdict: Option<Verdict>,
    emitter: &mut dyn Emitter,
) -> anyhow::Result<Vec<Datapoint>> {
    let datapoints = datapoints(tag, leg, verdict);
    if datapoints.is_empty() {
        return Ok(datapoints);
    }
    validate(&datapoints)?;
    emitter.emit(NAMESPACE, &serde_json::to_string(&datapoints)?)?;
    Ok(datapoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios;
    use crate::toolchain::{self, Version};

    fn matrix() -> Vec<Leg> {
        let clamped = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .expect("a real span");
        scenarios::legs(&clamped.kept)
    }

    /// A leg from the real matrix, with the rigged marker explicitly cleared — see
    /// [`crate::leg`]'s fixture for why it is cleared rather than taken as it comes. In short: one
    /// manifest really is rigged during the stage that tests the paging chain, and a fixture that
    /// changed meaning then would fail the commit that rigs it.
    fn leg(id: &str) -> Leg {
        let leg = matrix()
            .into_iter()
            .find(|l| l.id == id)
            .unwrap_or_else(|| panic!("no leg {id}"));
        Leg {
            induced: None,
            ..leg
        }
    }

    #[derive(Default)]
    struct FakeEmitter {
        calls: Vec<(String, String)>,
        fail: bool,
    }

    impl Emitter for FakeEmitter {
        fn emit(&mut self, namespace: &str, metric_data: &str) -> anyhow::Result<()> {
            self.calls
                .push((namespace.to_string(), metric_data.to_string()));
            anyhow::ensure!(!self.fail, "AccessDenied");
            Ok(())
        }
    }

    /// The rule that keeps an absent datapoint meaningful. A green leg writing `0` would make
    /// "nothing arrived" indistinguishable from "it passed", and "nothing arrived" is the signature
    /// of a leg that died without reporting.
    #[test]
    fn a_green_leg_writes_nothing() {
        let mut e = FakeEmitter::default();
        let written = record("release-x", &leg("new-project--stable"), None, &mut e).unwrap();
        assert!(written.is_empty());
        assert!(e.calls.is_empty(), "no call at all, not an empty call");
    }

    /// Every verdict emits, including `INFRA_FAIL`: one severity for all five is the deliberate
    /// trade, and the verdict is on the second series so a responder can tell them apart.
    #[test]
    fn every_verdict_writes_both_series() {
        for verdict in [
            Verdict::ResolveFail,
            Verdict::CompileFail,
            Verdict::HarnessFail,
            Verdict::RuntimeFail,
            Verdict::InfraFail,
        ] {
            let mut e = FakeEmitter::default();
            let written = record(
                "release-2026-11-09",
                &leg("aws-esdk--stable"),
                Some(verdict),
                &mut e,
            )
            .unwrap();

            assert_eq!(written.len(), 2, "{verdict} should write two series");
            assert_eq!(e.calls.len(), 1, "both series in one call");
            assert_eq!(e.calls[0].0, NAMESPACE);

            // The alarmed series carries the tag and nothing else: an alarm must not have to know
            // which leg broke.
            assert_eq!(
                written[0].dimensions,
                [dimension("Tag", "release-2026-11-09")]
            );
            // And the diagnosis series carries the verdict.
            assert!(
                written[1]
                    .dimensions
                    .contains(&dimension("Verdict", &verdict.to_string())),
                "{verdict} missing from the diagnosis series"
            );
        }
    }

    #[test]
    fn the_diagnosis_series_says_what_broke() {
        let mut e = FakeEmitter::default();
        let written = record(
            "release-2026-11-09",
            &leg("new-project--stable-2"),
            Some(Verdict::CompileFail),
            &mut e,
        )
        .unwrap();

        assert_eq!(
            written[1].to_string(),
            "Red=1 {Tag=release-2026-11-09, Manifest=new-project, \
             Verdict=COMPILE_FAIL, Leg=new-project--stable-2, \
             Toolchain=1.97.0, Target=native}"
        );
    }

    /// CloudWatch rejects an empty dimension value, and a native leg has no target. Every leg in the
    /// matrix has to produce a valid datapoint, not just the ones that cross-compile.
    #[test]
    fn every_leg_in_the_matrix_produces_a_valid_datapoint() {
        for leg in matrix() {
            let d = datapoints("release-2026-11-09", &leg, Some(Verdict::CompileFail));
            assert_eq!(d.len(), 2, "{}", leg.id);
            validate(&d).unwrap_or_else(|e| panic!("{}: {e}", leg.id));
            for dim in &d[1].dimensions {
                assert!(!dim.value.is_empty(), "{}: {} is empty", leg.id, dim.name);
            }
        }
    }

    /// A cross-compiled leg names its target; a native one says `native` rather than nothing.
    #[test]
    fn a_cross_compiled_leg_names_its_target() {
        let musl = datapoints("release-x", &leg("musl"), Some(Verdict::CompileFail));
        let target = musl[1]
            .dimensions
            .iter()
            .find(|d| d.name == "Target")
            .unwrap();
        assert_eq!(target.value, "aarch64-unknown-linux-musl");

        let native = datapoints(
            "release-x",
            &leg("new-project--stable"),
            Some(Verdict::CompileFail),
        );
        assert_eq!(
            native[1]
                .dimensions
                .iter()
                .find(|d| d.name == "Target")
                .unwrap()
                .value,
            NATIVE
        );
    }

    /// An unset tag would otherwise be sent and rejected by the API, with the error naming the
    /// request rather than the cause.
    #[test]
    fn an_empty_tag_is_refused_before_the_call() {
        let mut e = FakeEmitter::default();
        let err = record(
            "",
            &leg("new-project--stable"),
            Some(Verdict::CompileFail),
            &mut e,
        )
        .expect_err("an empty tag must not be sent");
        assert!(err.to_string().contains("empty value"), "{err}");
        assert!(e.calls.is_empty(), "and nothing should have been sent");
    }

    /// The payload has to be exactly what the CLI's `--metric-data` accepts, so its shape is pinned
    /// rather than assumed.
    #[test]
    fn the_payload_is_the_shape_the_cli_wants() {
        let mut e = FakeEmitter::default();
        record(
            "release-2026-11-09",
            &leg("aws-esdk--stable"),
            Some(Verdict::CompileFail),
            &mut e,
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&e.calls[0].1).unwrap();
        let arr = parsed.as_array().expect("an array of datapoints");
        assert_eq!(arr.len(), 2);
        for d in arr {
            assert_eq!(d["MetricName"], "Red");
            assert_eq!(d["Value"], 1.0);
            assert_eq!(d["Unit"], "Count");
            let dims = d["Dimensions"].as_array().unwrap();
            assert!(!dims.is_empty());
            for dim in dims {
                assert!(dim["Name"].is_string());
                assert!(dim["Value"].is_string());
            }
        }
    }

    /// A failed emission has to be an error. A verdict that was not recorded cannot raise an alarm,
    /// so swallowing this would turn a break into silence.
    #[test]
    fn a_failed_emission_is_an_error() {
        let mut e = FakeEmitter {
            fail: true,
            ..Default::default()
        };
        assert!(record(
            "release-x",
            &leg("new-project--stable"),
            Some(Verdict::CompileFail),
            &mut e
        )
        .is_err());
    }

    /// A leg whose manifest was rigged, which the real table never has — see
    /// [`crate::scenarios::Scenario::induced`] for why it is withdrawn before the next release.
    fn rigged(id: &str) -> Leg {
        Leg {
            induced: Some("rigged to exercise the paging chain".to_string()),
            ..leg(id)
        }
    }

    /// A ticket's dedupe key comes from its alarm, and there is one alarm per series, so if a
    /// rigged leg wrote to `Red {Tag}` then a real break landing in the same release would
    /// collapse into the test's ticket and disappear — the exact failure the canary exists to
    /// prevent, reintroduced by the act of testing it.
    #[test]
    fn a_rigged_leg_pages_on_its_own_series() {
        let real = datapoints(
            "release-x",
            &leg("new-project--stable"),
            Some(Verdict::CompileFail),
        );
        let test = datapoints(
            "release-x",
            &rigged("new-project--stable"),
            Some(Verdict::CompileFail),
        );

        assert_ne!(
            real[0].dimensions, test[0].dimensions,
            "sharing the alarmed series means sharing the ticket"
        );
        assert_eq!(
            test[0].dimensions,
            [dimension("Tag", "release-x"), dimension(INDUCED, "true")]
        );
        // And the diagnosis copy says so too, so a responder can see it on the ticket rather than
        // having to find the commit that rigged it.
        assert!(test[1].dimensions.contains(&dimension(INDUCED, "true")));
        validate(&test).expect("a rigged leg still has to produce a valid datapoint");
    }

    /// The release alarm must *not* see a rigged leg. That is the whole mechanism, and it is also
    /// what makes the second alarm load-bearing rather than decorative: without it a rigged break
    /// pages nobody and the test proves nothing.
    #[test]
    fn the_release_alarm_does_not_see_a_rigged_leg() {
        let armed = vec![dimension("Tag", "release-x")];
        let test = datapoints(
            "release-x",
            &rigged("aws-esdk--stable"),
            Some(Verdict::CompileFail),
        );
        assert!(
            test.iter().all(|d| d.dimensions != armed),
            "no datapoint from a rigged leg may land on the series the release alarm watches"
        );
    }

    /// The marker says `true`, never the reason. A dimension value is part of a series identity, so
    /// carrying prose that gets reworded between releases would start a new series each time — and
    /// an alarm armed against the old one would then watch nothing.
    #[test]
    fn the_marker_does_not_carry_the_reason() {
        let a = datapoints(
            "release-x",
            &Leg {
                induced: Some("one wording".into()),
                ..leg("new-project--stable")
            },
            Some(Verdict::RuntimeFail),
        );
        let b = datapoints(
            "release-x",
            &Leg {
                induced: Some("a completely different wording".into()),
                ..leg("new-project--stable")
            },
            Some(Verdict::RuntimeFail),
        );
        assert_eq!(a, b, "the wording must not change the series");
    }

    /// The property the single alarm depends on. An alarm watches `Red {Tag}` and nothing else, so
    /// if the watcher's alarmed series differed from a leg's by even one dimension it would land on
    /// a series nothing evaluates — and a failed dispatch, which leaves a release entirely
    /// unchecked, would page no one.
    #[test]
    fn the_watcher_and_a_leg_share_the_alarmed_series_exactly() {
        let leg = datapoints(
            "release-2026-11-09",
            &leg("new-project--stable"),
            Some(Verdict::CompileFail),
        );
        let watcher = component_datapoints("release-2026-11-09", WATCHER, "dispatch-failed");
        assert_eq!(
            leg[0], watcher[0],
            "the alarmed series must be identical, or one alarm cannot cover both"
        );
        assert_eq!(leg[0].dimensions.len(), 1, "and it is the tag alone");
    }

    /// The diagnosis series says who failed and why, without pretending to be a leg.
    #[test]
    fn a_watcher_failure_names_itself_and_its_reason() {
        let mut e = FakeEmitter::default();
        let written =
            record_component_failure("release-2026-11-09", WATCHER, "dispatch-failed", &mut e)
                .unwrap();

        assert_eq!(written.len(), 2);
        assert_eq!(
            written[1].to_string(),
            "Red=1 {Tag=release-2026-11-09, Component=watcher, Reason=dispatch-failed}"
        );
        assert_eq!(e.calls.len(), 1);
        assert_eq!(e.calls[0].0, NAMESPACE);
    }

    /// Same refusal as a leg's: an unset tag would be rejected by the API with an error naming the
    /// request rather than the cause.
    #[test]
    fn a_watcher_failure_with_no_tag_is_refused_before_the_call() {
        let mut e = FakeEmitter::default();
        assert!(record_component_failure("", WATCHER, "dispatch-failed", &mut e).is_err());
        assert!(e.calls.is_empty());
    }

    /// The namespace is duplicated into TypeScript, where it is the *only* bound on the metrics
    /// role: `PutMetricData` has no resource-level permissions, so a condition on the namespace is
    /// all there is. A disagreement means every call is denied and every verdict is lost.
    #[test]
    fn the_namespace_matches_the_cdk_stack() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root");
        let ts =
            std::fs::read_to_string(root.join("tools/ci-cdk/lib/smithy-rs/build-canary-stack.ts"))
                .expect("the CDK stack should be readable from the repo");

        let declared = ts
            .lines()
            .find_map(|l| {
                let rest = l.trim().strip_prefix("export const METRIC_NAMESPACE = ")?;
                Some(rest.trim_end_matches(';').trim_matches('"').to_string())
            })
            .expect("METRIC_NAMESPACE not found in the CDK stack");

        assert_eq!(
            declared, NAMESPACE,
            "the namespace the legs emit to and the one the role permits have diverged"
        );
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One durable record per release that broke.
//!
//! # Why this exists at all, given the job log says the same thing
//!
//! Because the job log stops existing. Workflow artifacts and logs are kept 90 days by default, and
//! since October 2026 that same retention covers the workflow *runs* themselves — so three months
//! after a release broke there is no log, no artifact, and no run. The red-leg evidence this
//! repository uploads is kept 14 days, shorter still.
//!
//! This canary exists because a break once went unnoticed for 21 hours. The question that gets
//! asked afterwards is not "what does this run say" — someone is already looking at the run — it is
//! "how often does this happen, and to what". That question is asked months later, by which time
//! every trace has been deleted. So a release that broke gets one small object written somewhere
//! durable, and the ability to answer it survives.
//!
//! This is not about keeping the diagnosis private. The job log is public and carries it already,
//! for as long as the log exists.
//!
//! # Only when something broke
//!
//! A green release writes nothing. How often releases are green is a question for the metric, which
//! has a datapoint per leg per run and is built for counting; the archive is for the rare case
//! worth reading in full. `INFRA_FAIL` alone is not worth recording either — it says the runner
//! broke, not the release — so the trigger is "some leg reached a hard verdict", which is exactly
//! [`RunSummary::had_a_hard_verdict`].
//!
//! # A leg with no report is not a green leg
//!
//! The one thing this must never do is infer success from silence. A leg that died without
//! reporting is a defect in the canary, and recording it as green in the permanent record would
//! bury the only evidence of it. So [`summarize`] puts legs into four buckets — red, green,
//! superseded, unreported — and a leg reaches `green` only by having submitted a report that says
//! so. `unreported` is the bucket that must be empty, and it is in the record so that it is visible
//! when it is not.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::classify::{Diagnosis, Step, Verdict};
use crate::leg::Report;
use crate::scenarios::Leg;

/// Which run this is, so the record identifies itself without reference to anything that expires.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// The `release-*` tag the legs built against.
    pub release: String,
    /// GitHub's run id, and the attempt, because a re-run keeps the id and increments the attempt.
    /// Both are in the key, so re-running cannot overwrite the first attempt's record.
    pub run_id: String,
    pub run_attempt: u32,
    /// A link, which will outlive the run it points at. Kept anyway: while it resolves it is the
    /// fastest way in, and once it 404s it still says which run this was.
    pub run_url: String,
    /// `dispatch`, `floor` or `manual`.
    pub tier: String,
}

/// The key prefix the records live under.
///
/// Must equal `RECORD_PREFIX` in the CDK stack, which is what grants the orchestrator permission to
/// write exactly these keys. [`tests::the_prefix_matches_the_cdk_stack`] checks it, because a
/// disagreement would deny every write and leave the archive silently empty.
pub const PREFIX: &str = "runs";

/// The key a run's record is written under, relative to the bucket.
///
/// Per release *and* per attempt, so nothing ever overwrites anything: the archive only grows,
/// which is also why the role holding it has no delete permission.
pub fn key_prefix(run: &Run) -> anyhow::Result<String> {
    let safe = |label: &str, value: &str| -> anyhow::Result<()> {
        let ok = !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            && !value.contains("..");
        anyhow::ensure!(ok, "{label} {value:?} is not usable in an object key");
        Ok(())
    };
    safe("release", &run.release)?;
    safe("run id", &run.run_id)?;
    Ok(format!(
        "{PREFIX}/{}/{}-{}",
        run.release, run.run_id, run.run_attempt
    ))
}

/// A leg that reached a hard verdict, with everything needed to understand it later.
#[derive(Debug, Clone, Serialize)]
pub struct RedLeg {
    pub leg_id: String,
    pub manifest: String,
    pub toolchain: String,
    pub toolchain_version: String,
    pub target: Option<String>,
    pub verdict: Verdict,
    pub failed_step: Option<Step>,
    /// Carries the error codes, the message, and the versions — which is what makes the record
    /// readable a year later without the lockfile beside it.
    pub diagnosis: Option<Diagnosis>,
    pub elapsed_secs: u64,
    /// Present, and saying why, when this manifest was deliberately rigged to break.
    ///
    /// The one thing a reader of an old record cannot otherwise reconstruct. A rigged release looks
    /// exactly like a broken one — same verdict, same diagnosis, same ticket — and the commit that
    /// rigged it was reverted before the next release, so nothing else surviving says which it was.
    /// Omitted when absent, so it stands out rather than appearing as `false` on every leg of every
    /// real break.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub induced: Option<String>,
}

/// What one run of the canary found.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    #[serde(flatten)]
    pub run: Run,
    /// How many legs the matrix had, which is the denominator for everything below.
    pub legs_total: usize,
    /// Leg counts by outcome: each verdict by name, plus `GREEN`, `SUPERSEDED` and `UNREPORTED`.
    /// One map rather than several fields, so a reader can see at a glance that the four buckets
    /// add up to `legs_total`.
    pub outcomes: BTreeMap<String, usize>,
    pub red: Vec<RedLeg>,
    pub green: Vec<String>,
    /// Dispatched for a release that a newer tag superseded. Reached no verdict, so not green.
    pub superseded: Vec<String>,
    /// In the matrix, but no report came back. Should be empty; in the record so that it is visible
    /// when it is not.
    pub unreported: Vec<String>,
}

impl RunSummary {
    /// Whether this run is worth recording: some leg blamed the published graph.
    ///
    /// `INFRA_FAIL` does not count. It says the runner broke, and a permanent archive of runner
    /// flakes would make the archive useless for its one question.
    pub fn had_a_hard_verdict(&self) -> bool {
        self.red.iter().any(|l| l.verdict.is_hard())
    }

    /// Whether this release was checked **at all**.
    ///
    /// Every other signal in this system starts from a leg reaching a verdict. This is the one
    /// condition where no leg reached anything, so there is no verdict to be red, green or even
    /// `INFRA_FAIL` — and every mechanism downstream is therefore silent. A release can arrive here
    /// because the dispatch was accepted and dropped, because the `scenario` input named nothing,
    /// because the matrix job failed to expand, or because the runs were cancelled. The causes
    /// differ; the consequence is identical and is exactly what this canary exists to prevent.
    ///
    /// Deliberately *not* a threshold. A release where 39 of 40 legs are unreported is alarming,
    /// but it still produced a verdict, so the existing machinery saw it and the per-leg warning
    /// names it. The all-or-nothing case is different in kind rather than in degree: nothing
    /// anywhere observed the release.
    ///
    /// A superseded release does **not** count, because its legs report `superseded` rather than
    /// going silent — they deliberately say "I did not build", which is what that report exists for.
    pub fn nothing_ran(&self) -> bool {
        self.legs_total > 0 && self.unreported.len() == self.legs_total
    }
}

/// What the record job needs to know once the reports are folded.
///
/// On stdout as JSON so the workflow never parses prose, and read back by the step that emits the
/// datapoint — which is why the release name travels in here rather than as a workflow input. A
/// datapoint must not be recordable against a release the canary was not watching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// The release tag, taken from the folded record rather than from an argument.
    pub release: String,
    /// Where in the archive this record belongs.
    pub key_prefix: String,
    /// Whether to archive it at all: only a release that broke earns a permanent record.
    pub record: bool,
    /// [`RunSummary::nothing_ran`] — the release produced no verdict of any kind.
    pub unchecked: bool,
    /// Where the record was written.
    pub summary: String,
}

/// Fold a run's leg matrix and whatever reports came back into one record.
///
/// Reports are matched to legs by id. A report for a leg not in the matrix is ignored rather than
/// erroring: the matrix is regenerated here, so a mismatch means the toolchain span moved under us,
/// and dropping an unknown leg is better than refusing to write the record at all.
pub fn summarize(run: Run, legs: &[Leg], reports: &[Report]) -> RunSummary {
    let by_id: BTreeMap<&str, &Report> = reports.iter().map(|r| (r.leg_id.as_str(), r)).collect();

    let mut red = Vec::new();
    let mut green = Vec::new();
    let mut superseded = Vec::new();
    let mut unreported = Vec::new();

    for leg in legs {
        let Some(report) = by_id.get(leg.id.as_str()) else {
            unreported.push(leg.id.clone());
            continue;
        };
        match report.verdict {
            Some(verdict) => red.push(RedLeg {
                leg_id: leg.id.clone(),
                manifest: leg.manifest.clone(),
                toolchain: leg.toolchain.to_string(),
                toolchain_version: leg.toolchain_version.clone(),
                target: leg.target.clone(),
                verdict,
                failed_step: report.failed_step,
                diagnosis: report.diagnosis.clone(),
                elapsed_secs: report.elapsed.as_secs(),
                induced: leg.induced.clone(),
            }),
            // No verdict is green only if the leg actually ran. A superseded leg has no opinion,
            // and the distinction is the same one `Report::is_green` makes.
            None if report.is_green() => green.push(leg.id.clone()),
            None => superseded.push(leg.id.clone()),
        }
    }

    let mut outcomes = BTreeMap::new();
    for leg in &red {
        *outcomes.entry(leg.verdict.to_string()).or_insert(0) += 1;
    }
    for (label, count) in [
        ("GREEN", green.len()),
        ("SUPERSEDED", superseded.len()),
        ("UNREPORTED", unreported.len()),
    ] {
        if count > 0 {
            outcomes.insert(label.to_string(), count);
        }
    }

    RunSummary {
        run,
        legs_total: legs.len(),
        outcomes,
        red,
        green,
        superseded,
        unreported,
    }
}

/// Read every `report.json` under a directory tree.
///
/// Walks rather than globbing a fixed depth, because the layout is whatever
/// `actions/download-artifact` produced: one directory per artifact, each containing a report and
/// possibly the leg's sources. A file that will not parse is reported and skipped — one malformed
/// report must not cost the record of the other fifty-nine.
pub fn read_reports(root: &std::path::Path) -> anyhow::Result<Vec<Report>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("skipping {}: {e}", dir.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "report.json") {
                match std::fs::read_to_string(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|t| Ok(serde_json::from_str::<Report>(&t)?))
                {
                    Ok(report) => out.push(report),
                    Err(e) => eprintln!("ignoring unreadable {}: {e}", path.display()),
                }
            }
        }
    }
    out.sort_by(|a, b| a.leg_id.cmp(&b.leg_id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leg::Stopped;
    use crate::lockfile::Duplicated;
    use crate::scenarios;
    use crate::test_support::tempdir;
    use crate::toolchain::{self, Version};
    use std::time::Duration;

    fn run() -> Run {
        Run {
            release: "release-2026-11-09".into(),
            run_id: "42920335827".into(),
            run_attempt: 1,
            run_url: "https://github.com/smithy-lang/smithy-rs/actions/runs/42920335827".into(),
            tier: "dispatch".into(),
        }
    }

    /// The matrix a record is ever built from: the release matrix, excluding the test-registry
    /// legs. Those are dispatched by name when somebody is testing the canary, never by a release,
    /// so counting them would put five permanently `unreported` legs in every record -- the signal
    /// reserved for a leg that died silently.
    fn matrix() -> Vec<Leg> {
        let clamped = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .expect("a real span");
        scenarios::release_legs(&clamped.kept)
    }

    fn green_report(leg_id: &str) -> Report {
        Report {
            leg_id: leg_id.into(),
            verdict: None,
            failed_step: None,
            diagnosis: None,
            stopped: None,
            induced: false,
            attempts: vec![],
            elapsed: Duration::from_secs(60),
        }
    }

    fn red_report(leg_id: &str, verdict: Verdict) -> Report {
        Report {
            leg_id: leg_id.into(),
            verdict: Some(verdict),
            failed_step: Some(Step::Build),
            diagnosis: Some(Diagnosis {
                codes: vec!["E0308".into()],
                summary: Some("mismatched types".into()),
                target: Some("aws_config".into()),
                target_version: Some("1.10.1".into()),
                duplicate_versions: vec![Duplicated {
                    name: "aws-smithy-schema".into(),
                    versions: vec!["0.2.1".into(), "1.2.0".into()],
                }],
            }),
            stopped: Some(Stopped::AttemptsExhausted),
            induced: false,
            attempts: vec![],
            elapsed: Duration::from_secs(65),
        }
    }

    #[test]
    fn the_key_is_per_release_and_per_attempt() {
        let mut r = run();
        assert_eq!(
            key_prefix(&r).unwrap(),
            "runs/release-2026-11-09/42920335827-1"
        );
        // A re-run must not overwrite the first attempt's record.
        r.run_attempt = 2;
        assert_eq!(
            key_prefix(&r).unwrap(),
            "runs/release-2026-11-09/42920335827-2"
        );
    }

    #[test]
    fn a_key_cannot_escape_the_prefix() {
        for bad in ["../evil", "a/b", "..", ""] {
            let mut r = run();
            r.release = bad.into();
            assert!(
                key_prefix(&r).is_err(),
                "release {bad:?} should be rejected"
            );

            let mut r = run();
            r.run_id = bad.into();
            assert!(key_prefix(&r).is_err(), "run id {bad:?} should be rejected");
        }
    }

    /// A run with two manifests red at five toolchains each, and the rest green.
    #[test]
    fn it_counts_the_four_buckets_and_they_add_up() {
        let legs = matrix();
        let total = legs.len();

        let red_ids: Vec<String> = legs
            .iter()
            .filter(|l| l.manifest == "aws-esdk" || l.manifest == "tower-sessions-dynamodb-store")
            .map(|l| l.id.clone())
            .collect();
        assert_eq!(red_ids.len(), 10, "two manifests at five toolchains");

        let reports: Vec<Report> = legs
            .iter()
            .map(|l| match red_ids.contains(&l.id) {
                true => red_report(&l.id, Verdict::CompileFail),
                false => green_report(&l.id),
            })
            .collect();

        let s = summarize(run(), &legs, &reports);
        assert_eq!(s.legs_total, total);
        assert_eq!(s.red.len(), 10);
        assert_eq!(s.green.len(), total - 10);
        assert!(s.superseded.is_empty());
        assert!(s.unreported.is_empty(), "{:?}", s.unreported);
        assert_eq!(
            s.outcomes,
            BTreeMap::from([("COMPILE_FAIL".into(), 10), ("GREEN".into(), total - 10)])
        );
        assert_eq!(
            s.red.len() + s.green.len() + s.superseded.len() + s.unreported.len(),
            s.legs_total,
            "the buckets have to partition the matrix"
        );
    }

    /// The rule the module exists to protect. A leg that said nothing is not evidence of success,
    /// and recording it as green would bury the only trace of a canary defect.
    #[test]
    fn a_leg_with_no_report_is_unreported_not_green() {
        let legs = matrix();
        let reports: Vec<Report> = legs.iter().skip(1).map(|l| green_report(&l.id)).collect();

        let s = summarize(run(), &legs, &reports);
        assert_eq!(s.unreported, [legs[0].id.clone()]);
        assert_eq!(s.green.len(), legs.len() - 1);
        assert!(!s.green.contains(&legs[0].id));
        assert_eq!(s.outcomes.get("UNREPORTED"), Some(&1));
    }

    /// A superseded leg did not build. It is neither green nor red, and lumping it in with green
    /// would say the release was checked when it was not.
    #[test]
    fn a_superseded_leg_is_neither_green_nor_red() {
        let legs = matrix();
        let reports: Vec<Report> = legs.iter().map(|l| Report::superseded(&l.id)).collect();

        let s = summarize(run(), &legs, &reports);
        assert_eq!(s.superseded.len(), legs.len());
        assert!(s.green.is_empty(), "nothing was proven green");
        assert!(s.red.is_empty());
        assert!(!s.had_a_hard_verdict(), "so there is nothing to record");
    }

    /// The write trigger. A runner flake is not a release defect, and archiving it forever would
    /// make the archive useless for the one question it answers.
    #[test]
    fn an_infra_fail_alone_is_not_worth_recording() {
        let legs = matrix();
        let infra: Vec<Report> = legs
            .iter()
            .map(|l| red_report(&l.id, Verdict::InfraFail))
            .collect();
        assert!(!summarize(run(), &legs, &infra).had_a_hard_verdict());

        // But one hard verdict among them is.
        let mut mixed = infra;
        mixed[0] = red_report(&legs[0].id, Verdict::CompileFail);
        assert!(summarize(run(), &legs, &mixed).had_a_hard_verdict());
    }

    #[test]
    fn every_hard_verdict_triggers_a_record() {
        let legs = matrix();
        for verdict in [
            Verdict::ResolveFail,
            Verdict::CompileFail,
            Verdict::HarnessFail,
            Verdict::RuntimeFail,
        ] {
            let reports = vec![red_report(&legs[0].id, verdict)];
            assert!(
                summarize(run(), &legs, &reports).had_a_hard_verdict(),
                "{verdict} should be recorded"
            );
        }
    }

    /// The record has to carry the versions, because the artifact holding the lockfile expires
    /// before the record does.
    #[test]
    fn the_record_names_the_version_pair() {
        let legs = matrix();
        let reports = vec![red_report(&legs[0].id, Verdict::CompileFail)];
        let s = summarize(run(), &legs, &reports);

        let d = s.red[0].diagnosis.as_ref().expect("a diagnosis");
        assert_eq!(d.target_version.as_deref(), Some("1.10.1"));
        assert_eq!(
            d.duplicate_versions[0].to_string(),
            "aws-smithy-schema 0.2.1 and 1.2.0"
        );

        // And it survives the round trip to JSON, which is what actually gets stored.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("aws-smithy-schema"), "{json}");
        assert!(json.contains("1.10.1"));
        // `flatten` on the run has to put the identity at the top level, not under a `run` key.
        assert!(json.contains("\"release\":\"release-2026-11-09\""));
        assert!(json.contains("\"run_attempt\":1"));
    }

    /// The condition nothing else in this system can see.
    ///
    /// Every other signal starts from a leg reaching a verdict. When no leg reaches anything there
    /// is no verdict to be red, green or `INFRA_FAIL`, so the alarm sees nothing, the archive
    /// writes nothing, and the per-leg "no report" warning cannot fire because no leg job ran to
    /// print it. A release going entirely unverified is the thing this canary exists to prevent,
    /// and it was the one thing it would not have reported.
    #[test]
    fn a_release_where_no_leg_reported_is_recognised_as_unchecked() {
        let legs = matrix();
        let s = summarize(run(), &legs, &[]);

        assert!(s.nothing_ran(), "not one leg of {} reported", legs.len());
        assert_eq!(s.unreported.len(), legs.len());
        assert!(
            !s.had_a_hard_verdict(),
            "and the existing trigger is blind to it, which is the point"
        );
    }

    /// Deliberately all-or-nothing rather than a threshold. One leg short of total silence is
    /// alarming, but it still produced verdicts — so the alarm already saw them and the per-leg
    /// warning names the missing one. The all-unreported case differs in kind: nothing anywhere
    /// observed the release.
    #[test]
    fn one_leg_reporting_is_not_nothing_running() {
        let legs = matrix();
        let s = summarize(run(), &legs, &[green_report(&legs[0].id)]);

        assert!(!s.nothing_ran());
        assert_eq!(
            s.unreported.len(),
            legs.len() - 1,
            "and the rest is visible"
        );
    }

    /// A superseded release did not run its legs either, and must not be reported as unchecked: its
    /// legs say `superseded` rather than going silent, which is exactly what that report is for,
    /// and the newer release provides the coverage.
    #[test]
    fn a_superseded_release_is_not_unchecked() {
        let legs = matrix();
        let reports: Vec<Report> = legs.iter().map(|l| Report::superseded(&l.id)).collect();
        let s = summarize(run(), &legs, &reports);

        assert!(!s.nothing_ran(), "superseded is a report, not silence");
        assert!(s.unreported.is_empty());
    }

    /// The decision is written by one step and read by the step that emits the datapoint, so it has
    /// to survive the round trip — and it carries the release, so that a datapoint cannot be
    /// recorded against a release the canary was not watching.
    #[test]
    fn the_decision_round_trips_and_carries_the_release() {
        let d = Decision {
            release: "release-2026-11-11".into(),
            key_prefix: "runs/release-2026-11-11/42-1".into(),
            record: false,
            unchecked: true,
            summary: "/tmp/record/summary.json".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Decision>(&json).unwrap(), d);
        assert!(json.contains("release-2026-11-11"));
    }

    /// The one thing a reader of an old record cannot reconstruct for themselves.
    ///
    /// A rigged release looks exactly like a broken one — same verdict, same diagnosis, same
    /// ticket — and the commit that rigged it was reverted before the next release, so nothing else
    /// that survives says which it was. The record is read to answer "how often does this happen",
    /// and counting our own tests as breaks would answer it wrongly in the direction that matters.
    #[test]
    fn the_record_says_when_a_red_leg_was_rigged() {
        // Cleared explicitly, not taken as it comes: during the stage that tests the paging chain
        // one manifest really is rigged, and a fixture that changed meaning then would fail the
        // very commit that rigs it.
        let plain = Leg {
            induced: None,
            ..matrix().remove(0)
        };
        let reports = vec![red_report(&plain.id, Verdict::CompileFail)];

        let rigged = Leg {
            induced: Some("rigged to exercise the paging chain".to_string()),
            ..plain.clone()
        };
        let s = summarize(run(), &[rigged], &reports);
        assert_eq!(
            s.red[0].induced.as_deref(),
            Some("rigged to exercise the paging chain")
        );
        assert!(
            serde_json::to_string(&s).unwrap().contains("rigged"),
            "and the reason has to survive into the stored JSON"
        );

        // A real break says nothing at all, rather than `false` on every leg of every red release,
        // so the marker stands out instead of becoming noise.
        let s = summarize(run(), &[plain], &reports);
        assert_eq!(s.red[0].induced, None);
        assert!(!serde_json::to_string(&s).unwrap().contains("induced"));
    }

    /// A report naming a leg the matrix does not have must not stop the record being written.
    #[test]
    fn a_report_for_an_unknown_leg_is_ignored() {
        let legs = matrix();
        let mut reports = vec![green_report("a-leg-that-does-not-exist")];
        reports.extend(legs.iter().map(|l| green_report(&l.id)));

        let s = summarize(run(), &legs, &reports);
        assert_eq!(s.green.len(), legs.len());
        assert!(s.unreported.is_empty());
    }

    /// Reading back what a leg wrote, at the depth `download-artifact` produces.
    #[test]
    fn it_reads_reports_out_of_an_artifact_tree() {
        let dir = tempdir("summary-reports");
        let root = dir.path();

        for (leg, report) in [
            ("new-project--stable", green_report("new-project--stable")),
            (
                "ssstar--stable",
                red_report("ssstar--stable", Verdict::CompileFail),
            ),
        ] {
            let d = root.join(format!("evidence-{leg}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("report.json"),
                serde_json::to_string(&report).unwrap(),
            )
            .unwrap();
            // The sort of neighbour a real artifact has, which must not be read as a report.
            std::fs::write(d.join("Cargo.lock"), "version = 4\n").unwrap();
        }

        let reports = read_reports(root).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].leg_id, "new-project--stable");
        assert_eq!(reports[1].verdict, Some(Verdict::CompileFail));
        // The full diagnosis has to survive serialization, since that is how it reaches the record.
        assert_eq!(
            reports[1].diagnosis.as_ref().unwrap().duplicate_versions[0].name,
            "aws-smithy-schema"
        );
    }

    /// One unreadable report must not cost the record of the others. The leg it belonged to then
    /// shows up as unreported, which is the honest outcome.
    #[test]
    fn one_malformed_report_does_not_discard_the_rest() {
        let dir = tempdir("summary-malformed");
        let root = dir.path();

        let good = root.join("evidence-good");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("report.json"),
            serde_json::to_string(&green_report("new-project--stable")).unwrap(),
        )
        .unwrap();

        let bad = root.join("evidence-bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("report.json"), "{ truncated").unwrap();

        let reports = read_reports(root).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].leg_id, "new-project--stable");
    }

    #[test]
    fn an_empty_tree_reads_as_no_reports() {
        let dir = tempdir("summary-empty");
        assert!(read_reports(dir.path()).unwrap().is_empty());
    }

    /// The prefix is duplicated into TypeScript, where it scopes the IAM grant. A disagreement
    /// would deny every write and leave the archive silently empty.
    #[test]
    fn the_prefix_matches_the_cdk_stack() {
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
                let rest = l.trim().strip_prefix("export const RECORD_PREFIX = ")?;
                Some(rest.trim_end_matches(';').trim_matches('"').to_string())
            })
            .expect("RECORD_PREFIX not found in the CDK stack");

        assert_eq!(
            declared, PREFIX,
            "the Rust key prefix and the IAM grant's prefix have diverged"
        );
    }
}

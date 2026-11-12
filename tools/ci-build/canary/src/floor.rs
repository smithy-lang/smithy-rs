/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The floor tier's stop rule: whether this hour's dispatch is still worth making.
//!
//! The floor tier checks the published graph hourly for a day after a release, because index
//! propagation lags and a graph that resolved cleanly at minute 15 can resolve differently at
//! minute 75. Each hour decides whether to bother. This module is that decision.
//!
//! # What the rule used to be, and why it had to change
//!
//! It asked: *has any `build-canary.yml` run for this release tag ever concluded `failure`?* That
//! sentence has four problems, and they are all the same problem — it asks about the tag's history
//! when the useful question is about this release against what was already true.
//!
//! **It could not tell a new break from a standing one.** Any red leg answered yes, which is only
//! equivalent to "this release is broken" while red is exceptional. With one leg red for a reason
//! nobody has fixed, hour one answers yes on every release forever and hours 2–24 never run — so
//! the tier that exists to catch a break appearing late is the one tier guaranteed not to.
//!
//! **It read the run's conclusion, not the verdict.** A job fails for reasons that have nothing to
//! do with the published graph: checkout, the toolchain install, the matrix expander. `INFRA_FAIL`
//! was deliberately given exit 0 so infrastructure could not stop the window, and this bypassed
//! that for any infrastructure failure happening before the classifier ran.
//!
//! **It counted the dispatch tier's runs.** Those run *during* publishing, when crates arrive one
//! at a time and intermediate graphs genuinely do break. A transient at minute 15 disabled the whole
//! day — on the exact reasoning the floor tier exists to refute.
//!
//! **It was a latch, not a state.** One hundred runs of history with no time window, so a break
//! that later cleared still answered yes for the rest of the day.
//!
//! # What it is now
//!
//! > Skip this hour if the most recent completed full-matrix run found a hard verdict on a leg that
//! > was not already red before this release.
//!
//! Each clause fixes one of the four:
//!
//! * *a hard verdict* — read from the verdict-owning step, so infrastructure cannot stop the window
//! * *full-matrix run* — the floor tier's own dispatches, never the publish window's
//! * *most recent completed* — a state, so a cleared break stops latching
//! * *not already red before this release* — the baseline, so a standing red leg is not news
//!
//! # Where "before this release" comes from
//!
//! The previous release's own floor runs, read exactly the same way. No new storage and no artifact
//! download: a run is retained for 90 days and releases are at most nineteen days apart, so the
//! previous release's runs are always there.
//!
//! When there is no previous release to compare against — the first release ever to run this, or a
//! gap long enough to age the runs out — every red leg counts as news and the rule behaves exactly
//! as it did before. That is the deliberate choice: an absent baseline degrades to the old
//! behaviour rather than to something new and untested.
//!
//! # What this does not decide
//!
//! Whether to page. A verdict reaches an alarm from the leg that found it, independently of this;
//! skipping an hour suppresses *checking*, never reporting. So a release that is broken still pages
//! from the hour that found it, whatever this module says afterwards.

use std::collections::BTreeSet;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The step in the leg workflow that owns the verdict.
///
/// Reading this rather than the job's conclusion is what keeps infrastructure from stopping the
/// window: a leg that failed to check out never reaches this step, so the step is `skipped` and
/// not `failure`. Duplicated from the YAML and cross-checked against it by a test.
pub const LEG_STEP: &str = "Run the leg";

/// The `scenario` input a full-matrix dispatch uses, which is what distinguishes the floor tier's
/// runs from the publish window's in a run listing.
pub const FLOOR_SCENARIO: &str = "all";

/// The legs that reached a hard verdict in one dispatched run.
///
/// Takes `gh run view --json jobs`. A leg is red when its verdict-owning step concluded `failure`;
/// every other outcome — green, `INFRA_FAIL`, superseded, deliberately rigged — exits 0 and so
/// succeeds. Jobs without that step are not legs: the matrix expander is one.
pub fn red_legs(run_view: &str) -> anyhow::Result<BTreeSet<String>> {
    #[derive(Deserialize)]
    struct Step {
        name: String,
        conclusion: Option<String>,
    }
    #[derive(Deserialize)]
    struct Job {
        name: String,
        #[serde(default)]
        steps: Vec<Step>,
    }
    #[derive(Deserialize)]
    struct View {
        jobs: Vec<Job>,
    }

    let view: View = serde_json::from_str(run_view).context("parsing the run view")?;
    Ok(view
        .jobs
        .into_iter()
        .filter(|j| {
            j.steps
                .iter()
                .any(|s| s.name == LEG_STEP && s.conclusion.as_deref() == Some("failure"))
        })
        .map(|j| j.name)
        .collect())
}

/// One run in a `gh run list` listing, reduced to what the gate needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorRun {
    pub id: u64,
    /// The ref it ran from, which for these is always a release tag.
    pub tag: String,
}

/// The newest completed full-matrix runs: one for this release, one for the release before it.
///
/// Both come from a single listing, so the gate needs one call to find them. Sorted here by
/// `createdAt` rather than trusting the listing's order, because "newest" is the whole basis of
/// reading a state instead of a latch.
pub fn newest_floor_runs(
    listing: &str,
    tag: &str,
) -> anyhow::Result<(Option<FloorRun>, Option<FloorRun>)> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        database_id: u64,
        conclusion: Option<String>,
        display_title: String,
        head_branch: String,
        created_at: String,
    }

    let mut entries: Vec<Entry> =
        serde_json::from_str(listing).context("parsing the run listing")?;
    // Newest first. String ordering is correct for the RFC 3339 timestamps the API returns.
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let wanted_title = crate::watch::dispatched_run_title(FLOOR_SCENARIO);
    let usable = |e: &Entry| {
        e.display_title == wanted_title
            // Only `success` and `failure` say anything about the legs. `cancelled` reached no
            // verdict about anything, and the rest -- `timed_out`, `stale`, `action_required` --
            // are the same. A still-running run has no conclusion at all, so this also excludes it:
            // checking `status` as well would be dead code, since a non-null conclusion implies a
            // completed run.
            && matches!(e.conclusion.as_deref(), Some("success") | Some("failure"))
    };

    let mine = entries
        .iter()
        .find(|e| usable(e) && e.head_branch == tag)
        .map(|e| FloorRun {
            id: e.database_id,
            tag: e.head_branch.clone(),
        });

    // The previous release: the newest usable run from some *other* release tag. Anything that is
    // not a release tag is excluded, so a run dispatched by hand from a branch cannot become the
    // baseline every release is then measured against.
    let previous = entries
        .iter()
        .find(|e| usable(e) && e.head_branch != tag && e.head_branch.starts_with("release-"))
        .map(|e| FloorRun {
            id: e.database_id,
            tag: e.head_branch.clone(),
        });

    Ok((mine, previous))
}

/// What a floor hour should do, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    /// Whether to dispatch this hour's legs.
    pub dispatch: bool,
    /// Legs red in this release that were not red before it. Empty whenever `dispatch` is true.
    pub news: Vec<String>,
    /// Said in full, because this ends up in a job summary that someone reads months later while
    /// wondering why a window stopped early.
    pub reason: String,
}

/// Decide whether this hour is still worth dispatching.
///
/// `this_release` is the red set of the most recent completed full-matrix run for this release;
/// `baseline` is the previous release's, or `None` when there is no previous release to compare
/// against. See the module docs for why `None` degrades to "every red leg is news".
pub fn decide(
    this_release: Option<&BTreeSet<String>>,
    baseline: Option<&BTreeSet<String>>,
) -> Gate {
    let Some(red) = this_release else {
        return Gate {
            dispatch: true,
            news: vec![],
            reason: "no completed full-matrix run for this release yet, so there is nothing to \
                     compare and this hour is the first look"
                .to_string(),
        };
    };
    if red.is_empty() {
        return Gate {
            dispatch: true,
            news: vec![],
            reason: "the last full-matrix run found nothing wrong".to_string(),
        };
    }

    let empty = BTreeSet::new();
    let known = baseline.unwrap_or(&empty);
    let news: Vec<String> = red.difference(known).cloned().collect();

    if news.is_empty() {
        return Gate {
            dispatch: true,
            news,
            reason: format!(
                "{} leg(s) are red and every one was already red before this release, so none of \
                 it is news about this release and the window keeps checking: {}",
                red.len(),
                comma(red.iter().map(String::as_str))
            ),
        };
    }

    // The rest of the red set was already red before this release. Naming it matters: without it a
    // reader sees "3 legs are new" next to a run showing 23 red and cannot tell which is which.
    let already: Vec<&String> = red.intersection(known).collect();
    let reason = match baseline.is_some() {
        true => format!(
            "{} leg(s) went red that were not red before this release, so it has been reported and \
             a later hour would dedupe into the same ticket: {}. Already red beforehand, and \
             therefore not counted: {}",
            news.len(),
            comma(news.iter().map(String::as_str)),
            comma(already.into_iter().map(String::as_str)),
        ),
        false => format!(
            "{} leg(s) are red and there is no previous release to compare against, so all of it \
             counts as news: {}",
            news.len(),
            comma(news.iter().map(String::as_str)),
        ),
    };
    Gate {
        dispatch: false,
        news,
        reason,
    }
}

/// Name a few legs and count the rest.
///
/// Capped because these reasons go into a `::notice` and a job summary a person reads. Twenty leg
/// ids is eight hundred characters of near-identical text, which is how a message stops being read
/// at all — and the ones that matter are in `news`, which is never capped and is right there in the
/// JSON.
fn comma<'a>(it: impl Iterator<Item = &'a str>) -> String {
    const SHOW: usize = 5;
    let v: Vec<&str> = it.collect();
    match v.len() {
        0 => "none".to_string(),
        n if n <= SHOW => v.join(", "),
        n => format!("{}, and {} more", v[..SHOW].join(", "), n - SHOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// Captured from `gh run view --json jobs` on a real 60-leg run: the verdict-owning step fails
    /// for a hard verdict, and the matrix expander has no such step at all.
    const VIEW: &str = r#"{"jobs":[
        {"name":"Expand the matrix","steps":[
            {"name":"Set up job","conclusion":"success"},
            {"name":"Generate","conclusion":"success"}]},
        {"name":"new-project--stable","steps":[
            {"name":"Run actions/checkout@v4","conclusion":"success"},
            {"name":"Install Rust","conclusion":"success"},
            {"name":"Run the leg","conclusion":"success"},
            {"name":"Report the verdict","conclusion":"success"}]},
        {"name":"ssstar--stable","steps":[
            {"name":"Run actions/checkout@v4","conclusion":"success"},
            {"name":"Install Rust","conclusion":"success"},
            {"name":"Run the leg","conclusion":"failure"},
            {"name":"Report the verdict","conclusion":"success"}]},
        {"name":"aws-esdk--stable","steps":[
            {"name":"Run actions/checkout@v4","conclusion":"failure"},
            {"name":"Install Rust","conclusion":"skipped"},
            {"name":"Run the leg","conclusion":"skipped"}]}
    ]}"#;

    /// The red set is the legs whose *verdict* failed, which is not the same as the legs whose job
    /// failed. `aws-esdk--stable` above failed at checkout: its job is red and it reached no verdict
    /// at all, so the published graph is not implicated and it must not stop the window.
    ///
    /// This is the clause that restores a property the old rule broke. `INFRA_FAIL` was given exit
    /// 0 precisely so infrastructure could not stop the window, and reading the job conclusion
    /// bypassed that for any infrastructure failure happening before the classifier ran.
    #[test]
    fn a_leg_is_red_only_when_its_verdict_failed() {
        let red = red_legs(VIEW).unwrap();
        assert_eq!(red, set(&["ssstar--stable"]));
        assert!(
            !red.contains("aws-esdk--stable"),
            "it failed at checkout, not at a verdict"
        );
        assert!(
            !red.contains("Expand the matrix"),
            "the matrix expander is not a leg"
        );
    }

    /// An unparseable view must not read as "nothing is red", which would silently turn the gate
    /// into "always dispatch" and hide a broken release behind a `gh` problem.
    #[test]
    fn an_unreadable_run_view_is_an_error() {
        for bad in ["", "not json", "[]", r#"{"jobs":"nope"}"#] {
            assert!(red_legs(bad).is_err(), "{bad:?} must not read as empty");
        }
    }

    /// Every run that must be *excluded* is deliberately **newer** than the one that should win, so
    /// that dropping any one filter changes the answer. An earlier version of this fixture put the
    /// exclusions older than the winner, which meant three of the filters could be deleted with
    /// every test still passing. Found by mutation, not by reading.
    const LISTING: &str = r#"[
        {"databaseId":10,"status":"in_progress","conclusion":null,
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
         "createdAt":"2026-11-11T13:00:00Z"},
        {"databaseId":9,"status":"completed","conclusion":"cancelled",
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
         "createdAt":"2026-11-11T12:30:00Z"},
        {"databaseId":8,"status":"completed","conclusion":"failure",
         "displayTitle":"Build canary - new-project,wasm","headBranch":"release-2026-11-11",
         "createdAt":"2026-11-11T12:00:00Z"},
        {"databaseId":7,"status":"completed","conclusion":"failure",
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
         "createdAt":"2026-11-11T11:00:00Z"},
        {"databaseId":6,"status":"completed","conclusion":"success",
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
         "createdAt":"2026-11-11T10:00:00Z"},
        {"databaseId":5,"status":"completed","conclusion":"success",
         "displayTitle":"Build canary - all","headBranch":"some/feature-branch",
         "createdAt":"2026-11-11T12:45:00Z"},
        {"databaseId":4,"status":"completed","conclusion":"success",
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-10",
         "createdAt":"2026-11-10T09:00:00Z"},
        {"databaseId":3,"status":"completed","conclusion":"success",
         "displayTitle":"Build canary - all","headBranch":"release-2026-11-09",
         "createdAt":"2026-11-09T08:00:00Z"}
    ]"#;

    /// Five selection rules at once, each of which fixes one of the old rule's defects.
    #[test]
    fn it_picks_the_newest_completed_full_matrix_run_for_each_release() {
        let (mine, previous) = newest_floor_runs(LISTING, "release-2026-11-11").unwrap();

        // Everything newer than 7 is disqualified, and each for a different reason: 10 is still
        // running and has concluded nothing, 9 was cancelled and so reached no verdict about
        // anything, and 8 is a publish-window dispatch rather than a floor hour — the window where
        // intermediate graphs break on purpose.
        assert_eq!(mine.unwrap().id, 7);

        // The baseline is the previous *release*. Run 5 is newer than 4 and would win on time
        // alone, but it was dispatched by hand from a branch — which must never become the thing
        // every release is then measured against. And 3 is a release, but an older one.
        let p = previous.unwrap();
        assert_eq!(p.id, 4);
        assert_eq!(p.tag, "release-2026-11-10");
    }

    /// Order is established here rather than inherited from the listing, because reading a state
    /// instead of a latch is the entire point. Runs 7 and 6 are both usable for this release, so
    /// trusting the listing's order would pick whichever came first in it.
    #[test]
    fn newest_is_decided_by_timestamp_not_by_listing_order() {
        let reversed: serde_json::Value = {
            let mut v: Vec<serde_json::Value> = serde_json::from_str(LISTING).unwrap();
            v.reverse();
            v.into()
        };
        let (mine, previous) =
            newest_floor_runs(&reversed.to_string(), "release-2026-11-11").unwrap();
        assert_eq!(mine.unwrap().id, 7, "not 6, which is older");
        assert_eq!(previous.unwrap().id, 4, "not 3, which is older");
    }

    #[test]
    fn a_release_with_no_completed_run_yet_dispatches() {
        let g = decide(None, Some(&set(&["ssstar--stable"])));
        assert!(g.dispatch);
        assert!(g.news.is_empty());
        assert!(g.reason.contains("nothing to compare"), "{}", g.reason);
    }

    #[test]
    fn a_green_release_keeps_checking() {
        let g = decide(Some(&set(&[])), Some(&set(&["ssstar--stable"])));
        assert!(g.dispatch);
        assert!(g.reason.contains("nothing wrong"), "{}", g.reason);
    }

    /// The defect this module exists for. One leg red for a reason nobody has fixed made hour one
    /// answer "already broken" on every release forever, so hours 2–24 never ran — the tier that
    /// exists to catch a late break was the one tier guaranteed not to.
    #[test]
    fn a_leg_that_was_already_red_is_not_news() {
        let standing = set(&["ssstar--stable", "ssstar--stable-1"]);
        let g = decide(Some(&standing), Some(&standing));

        assert!(g.dispatch, "{}", g.reason);
        assert!(g.news.is_empty());
        assert!(
            g.reason.contains("already red before this release"),
            "and it has to say so, because a reader months later will ask why a red release kept \
             checking: {}",
            g.reason
        );
    }

    /// And the rule still does its original job: a break that is genuinely new stops the window,
    /// because the release has been reported and a later hour would dedupe into the same ticket.
    #[test]
    fn a_new_break_still_stops_the_window() {
        let g = decide(
            Some(&set(&["ssstar--stable", "new-project--stable"])),
            Some(&set(&["ssstar--stable"])),
        );
        assert!(!g.dispatch);
        assert_eq!(g.news, ["new-project--stable"]);
        // Both halves have to be readable: which legs are new, and which were already known.
        assert!(g.reason.contains("new-project--stable"), "{}", g.reason);
        assert!(
            g.reason.contains("Already red beforehand") && g.reason.contains("ssstar--stable"),
            "{}",
            g.reason
        );
    }

    /// With no previous release to compare against, every red leg counts as news — which is exactly
    /// what the old rule did. An absent baseline degrades to the old behaviour rather than to
    /// something new and untested.
    #[test]
    fn no_baseline_degrades_to_the_old_behaviour() {
        let g = decide(Some(&set(&["ssstar--stable"])), None);
        assert!(!g.dispatch);
        assert_eq!(g.news, ["ssstar--stable"]);
        assert!(
            g.reason.contains("no previous release"),
            "and it must say which rule it applied: {}",
            g.reason
        );
    }

    /// A break that cleared stops latching. The old rule read a hundred runs of history with no
    /// time window, so one failure answered yes for the rest of the day even after the graph
    /// settled.
    #[test]
    fn a_break_that_cleared_no_longer_stops_the_window() {
        // Hour 3 found something; hour 4 is the newest completed run and found nothing.
        let g = decide(Some(&set(&[])), Some(&set(&[])));
        assert!(g.dispatch, "{}", g.reason);
    }

    /// The step name is duplicated from the YAML, and a drift would make every leg look green —
    /// turning the gate into "always dispatch" and losing the stop rule altogether, quietly.
    #[test]
    fn the_step_name_matches_what_the_leg_workflow_declares() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root");
        let yaml = std::fs::read_to_string(root.join(".github/workflows/build-canary.yml"))
            .expect("the leg workflow should be readable");
        assert!(
            yaml.contains(&format!("- name: {LEG_STEP}")),
            "no step named {LEG_STEP:?} in the leg workflow"
        );
        // And it must be the step that runs the leg, not merely a step with that name.
        let after = yaml
            .split(&format!("- name: {LEG_STEP}"))
            .nth(1)
            .expect("a step body");
        assert!(
            after.contains("run-leg"),
            "the {LEG_STEP:?} step must be the one invoking `run-leg`"
        );
    }
}

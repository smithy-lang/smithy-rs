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
//! # The rule
//!
//! > Skip this hour if the most recent completed full-matrix run found a hard verdict on a leg that
//! > was not already red before this release.
//!
//! Every clause carries its own weight:
//!
//! * *a hard verdict* — read from the verdict-owning step rather than the job's conclusion, so an
//!   infrastructure failure before the classifier runs cannot stop the window that `INFRA_FAIL`'s
//!   exit 0 exists to protect
//! * *full-matrix run* — the floor tier's own dispatches, never the publish window's, where crates
//!   arrive one at a time and intermediate graphs genuinely do break
//! * *most recent completed* — a state rather than a latch, so a break that later cleared stops
//!   counting
//! * *not already red before this release* — a leg red for a reason nobody has fixed is not news,
//!   and without this clause the tier that exists to catch a late break is the one guaranteed not
//!   to run
//!
//! # Where "before this release" comes from
//!
//! The previous release's own floor runs, read exactly the same way. No new storage and no artifact
//! download: a run is retained for 90 days and releases are at most nineteen days apart, so the
//! previous release's runs are always there.
//!
//! When there is no previous release to compare against — the first release ever to run this, or a
//! gap long enough to age the runs out — whether a red leg is news cannot be decided, so the window
//! keeps checking. Fail-open is the safe direction: it costs runner minutes and checks everything,
//! where the opposite would skip hours nobody asked it to skip.
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

    // The earliest run for *this* release, usable or not. Any run of ours existing means our tag
    // did, which is what makes the cutoff below meaningful. Not filtered by `usable`, because a
    // cancelled or still-running run of ours is just as much evidence that the tag was there.
    let first_mine = entries
        .iter()
        .filter(|e| e.head_branch == tag)
        .map(|e| e.created_at.as_str())
        .min();

    // The previous release: the newest usable run from some *other* release tag, dispatched before
    // this release existed.
    //
    // That last clause is the one that is easy to leave out. Releases land within a day of each
    // other about one time in eight, and when they do, the earlier release's floor tier keeps
    // completing runs whose legs all exit `superseded` — no verdict, exit 0, so the run concludes
    // `success` with nothing red in it. It is then both usable and the newest, so it becomes the
    // baseline, and a green baseline makes every standing red leg look like news about *this*
    // release. The window stops after one hour, which is the outcome the "already red" clause
    // exists to prevent, reached through a run that built nothing.
    //
    // A run from the previous release started in the couple of minutes before ours could still have
    // seen our tag and come back superseded, and still passes this filter. So this narrows the
    // window rather than closing it.
    //
    // Only date-shaped tags count, so a stray `release-*` tag's run cannot become the baseline
    // either — the same reason `watch::is_release_tag` exists.
    let previous = entries
        .iter()
        .find(|e| {
            usable(e)
                && e.head_branch != tag
                && crate::watch::is_release_tag(&e.head_branch)
                && first_mine.is_none_or(|first| e.created_at.as_str() < first)
        })
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

    // No baseline means *unknown*, not "nothing was red before". Treating it as a clean previous
    // release would make a standing red look like news and stop the remaining hours — which is the
    // one thing the `not already red` clause exists to prevent, in the case with least to go on.
    // Checking anyway costs runner minutes; not checking is the failure this canary is for.
    let Some(known) = baseline else {
        return Gate {
            dispatch: true,
            // Nothing can be called news without something to compare against, and a reader acting
            // on this field would be acting on a guess. The reason names the red legs instead.
            news: vec![],
            reason: format!(
                "{} leg(s) are red and there is no previous release to compare against, so \
                 whether any of it is news about this release is unknown and the window keeps \
                 checking: {}",
                red.len(),
                comma(red.iter().map(String::as_str))
            ),
        };
    };
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
    Gate {
        dispatch: false,
        reason: format!(
            "{} leg(s) went red that were not red before this release, so it has been reported and \
             a later hour would dedupe into the same ticket: {}. Already red beforehand, and \
             therefore not counted: {}",
            news.len(),
            comma(news.iter().map(String::as_str)),
            comma(already.into_iter().map(String::as_str)),
        ),
        news,
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
    /// `INFRA_FAIL` exits 0 so infrastructure cannot stop the window. Reading the job's
    /// conclusion instead of this step's would bypass that for any infrastructure failure
    /// happening before the classifier runs.
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
    /// that dropping any one filter changes the answer. With the exclusions older than the winner
    /// instead, three of the filters can be deleted with every test still passing.
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

    /// A release landing mid-window must not hand the next release a baseline that built nothing;
    /// [`newest_floor_runs`] has the reason. This asserts that the previous release's real run
    /// wins over its superseded one.
    #[test]
    fn a_superseded_run_from_the_previous_release_is_not_the_baseline() {
        // 11-11 is ours, first dispatched at 10:00. 11-10's 09:00 run predates that and is real;
        // its 11:00 run came after our tag existed, so its legs were superseded and it is green
        // without having built anything.
        const OVERLAPPING: &str = r#"[
            {"databaseId":30,"status":"completed","conclusion":"success",
             "displayTitle":"Build canary - all","headBranch":"release-2026-11-10",
             "createdAt":"2026-11-11T11:00:00Z"},
            {"databaseId":20,"status":"completed","conclusion":"failure",
             "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
             "createdAt":"2026-11-11T10:00:00Z"},
            {"databaseId":10,"status":"completed","conclusion":"failure",
             "displayTitle":"Build canary - all","headBranch":"release-2026-11-10",
             "createdAt":"2026-11-10T09:00:00Z"}
        ]"#;

        let (mine, previous) = newest_floor_runs(OVERLAPPING, "release-2026-11-11").unwrap();
        assert_eq!(mine.unwrap().id, 20);

        let previous = previous.expect("the previous release's real run is still a baseline");
        assert_eq!(
            previous.id, 10,
            "run 30 is newer and from another release, but it started after our tag existed, so \
             its legs were superseded and its greenness says nothing"
        );
        assert_eq!(previous.tag, "release-2026-11-10");
    }

    /// And the cutoff must not throw away a legitimate baseline. Without a run of our own there is
    /// nothing to compare against, so every previous-release run stays eligible.
    #[test]
    fn with_no_run_of_our_own_the_cutoff_excludes_nothing() {
        const NO_RUN_YET: &str = r#"[
            {"databaseId":40,"status":"completed","conclusion":"success",
             "displayTitle":"Build canary - all","headBranch":"release-2026-11-10",
             "createdAt":"2026-11-10T09:00:00Z"}
        ]"#;
        let (mine, previous) = newest_floor_runs(NO_RUN_YET, "release-2026-11-11").unwrap();
        assert!(mine.is_none());
        assert_eq!(previous.expect("still eligible").id, 40);
    }

    /// A stray tag's run cannot become the baseline either, for the same reason a stray tag cannot
    /// answer "what is the newest release".
    #[test]
    fn a_run_from_a_tag_that_is_not_a_release_is_not_the_baseline() {
        const STRAY: &str = r#"[
            {"databaseId":50,"status":"completed","conclusion":"success",
             "displayTitle":"Build canary - all","headBranch":"release-test",
             "createdAt":"2026-11-10T23:00:00Z"},
            {"databaseId":20,"status":"completed","conclusion":"failure",
             "displayTitle":"Build canary - all","headBranch":"release-2026-11-11",
             "createdAt":"2026-11-11T10:00:00Z"}
        ]"#;
        let (_, previous) = newest_floor_runs(STRAY, "release-2026-11-11").unwrap();
        assert!(
            previous.is_none(),
            "release-test is not a release, so its run is not a baseline"
        );
    }

    /// Five selection rules at once, each of which has to hold on its own.
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

    /// The defect this module exists for. A leg red for a reason nobody has fixed must not make
    /// hour one answer "already broken", or hours 2–24 never run — and the tier that exists to
    /// catch a late break would be the one guaranteed not to.
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

    /// With no previous release to compare against, whether a red leg is news is unknowable. The
    /// window keeps checking rather than guessing: treating the absence as a clean baseline would
    /// make a standing red stop hours 2-24, which is exactly what the `not already red` clause is
    /// there to prevent.
    #[test]
    fn no_baseline_keeps_checking() {
        let g = decide(Some(&set(&["ssstar--stable"])), None);
        assert!(g.dispatch);
        assert!(
            g.news.is_empty(),
            "nothing is news without something to compare against: {:?}",
            g.news
        );
        assert!(
            g.reason.contains("no previous release"),
            "and it must say which rule it applied: {}",
            g.reason
        );
    }

    /// A break that cleared stops latching. Reading a hundred runs of history with no time window
    /// would let one failure answer yes for the rest of the day, even after the graph settled.
    #[test]
    fn a_break_that_cleared_does_not_stop_the_window() {
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

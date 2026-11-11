/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The watcher: one run per release, resolving every 15 minutes while the crates arrive.
//!
//! A release publishes its crates one at a time over minutes, and the graph a consumer resolves
//! changes under them as it goes. The watcher covers that window. It resolves the thirteen
//! manifests on a tick, dispatches legs for the ones whose graph moved, and stops when the release
//! has finished publishing.
//!
//! # Why this is one run and not a cron
//!
//! A standing 15-minute poll would be 96 jobs a day. At the measured ~1% per-run transient rate
//! that is about one spurious `INFRA_FAIL` daily — more noise than the thing it would insure
//! against, which is a dispatch that silently failed to fire. That premise is removed instead by
//! making the dispatch a checked step, so its failure is loud at the source.
//!
//! So the watcher is *armed* by a release rather than being a tier of its own, and the 15-minute
//! resolve happens inside one run. That is also what keeps the baselines a directory: they never
//! have to outlive the run that wrote them.
//!
//! # The three rules that are easy to get backwards
//!
//! **The first tick is not diff-gated.** There is no baseline on the first sample, so every manifest
//! dispatches. That is not a special case here — [`crate::baseline::Movement::NeverSeen`] already
//! dispatches — but it is the behaviour the window depends on, so it has a test.
//!
//! **A manifest that fails to resolve is dispatched, not skipped.** The watcher's resolve is a
//! cheap proxy for the leg's; when it fails, the honest response is to let a leg run and report
//! `RESOLVE_FAIL` properly, with its retries and its evidence. Treating it as "nothing moved" would
//! silence the one verdict that says consumers cannot resolve the release at all.
//!
//! **A dispatch that fails stops the watch.** Every other failure here degrades towards doing more
//! work; this one degrades towards doing none, because legs that were never dispatched emit no
//! datapoint and an absent datapoint cannot raise an alarm. It is the one error the watcher refuses
//! to continue past.

use std::fmt;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;

use crate::baseline::{self, BaselineDir, Movement};
use crate::leg::Clock;

/// How often the graph is re-resolved.
///
/// Fifteen minutes is the stated detection target, not a derived number. The evidence establishes
/// only that a daily cadence is too coarse; every sub-daily setting captures nearly all the
/// available improvement, and 15 minutes is the only one that leaves room for the work inside a
/// 30-minute budget. Changing it is a conscious trade against that target.
pub const INTERVAL: Duration = Duration::from_secs(15 * 60);

/// The ceiling on one watcher run.
///
/// The window should end when the release finishes publishing. This bounds the case where that
/// signal never arrives — which is itself a finding, not a reason to watch forever.
pub const CEILING: Duration = Duration::from_secs(3 * 60 * 60);

/// How often to look, and for how long.
///
/// Passed in rather than read from the constants directly so that a test can compress a three-hour
/// window into microseconds, and so the two numbers are visible together at the call site where
/// their relationship matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    pub interval: Duration,
    pub ceiling: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            interval: INTERVAL,
            ceiling: CEILING,
        }
    }
}

/// What one tick decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tick {
    pub number: u32,
    /// Elapsed time into the run, in seconds, so a tick's place in the window is readable.
    pub at_secs: u64,
    /// Manifests whose graph moved, and which therefore had legs dispatched.
    pub dispatched: Vec<String>,
    /// Manifests whose graph was identical to the previous tick's.
    pub unchanged: Vec<String>,
    /// Manifests the watcher could not resolve. Dispatched anyway, so a leg reports it properly.
    pub unresolved: Vec<String>,
}

impl Tick {
    /// Whether this tick asked for any work at all.
    pub fn dispatched_anything(&self) -> bool {
        !self.dispatched.is_empty()
    }
}

/// Why the watcher stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stopped {
    /// Every declared runtime crate has arrived on crates.io. The expected ending.
    PublishingComplete,
    /// The ceiling was reached with crates still outstanding. Worth reporting: it means a release
    /// took longer than any observed one, or stalled partway.
    CeilingReached,
    /// A dispatch call failed. The watch cannot continue, because the legs it would have asked for
    /// are the only thing that produces a verdict.
    DispatchFailed,
}

/// What the watcher did across the whole window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchReport {
    pub tag: String,
    pub ticks: Vec<Tick>,
    pub stopped: Stopped,
}

impl WatchReport {
    /// How many dispatches were made, counting one per manifest per tick.
    pub fn dispatches(&self) -> usize {
        self.ticks.iter().map(|t| t.dispatched.len()).sum()
    }

    /// Whether the window ended the way it should have.
    pub fn ended_cleanly(&self) -> bool {
        self.stopped == Stopped::PublishingComplete
    }

    /// The process exit code. A failed dispatch is the only hard failure: the ceiling is a finding
    /// worth a look but not a reason to fail the job, since the floor tier still runs after it.
    pub fn exit_code(&self) -> i32 {
        match self.stopped {
            Stopped::DispatchFailed => 1,
            Stopped::PublishingComplete | Stopped::CeilingReached => 0,
        }
    }
}

impl fmt::Display for WatchReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} tick(s), {} dispatch(es), stopped because ",
            self.tag,
            self.ticks.len(),
            self.dispatches()
        )?;
        match self.stopped {
            Stopped::PublishingComplete => f.write_str("publishing finished"),
            // Deliberately not naming a duration. The ceiling is configurable, and the first
            // version of this line said "the 3-hour ceiling" while running against an
            // eight-second one.
            Stopped::CeilingReached => {
                f.write_str("the ceiling was reached with crates still outstanding")
            }
            Stopped::DispatchFailed => f.write_str("a dispatch call failed"),
        }
    }
}

/// Resolves a manifest's graph afresh. Injected so the tick logic is testable without cargo.
pub trait Resolver {
    /// The `Cargo.lock` this manifest resolves to right now, or an error if it will not resolve.
    fn resolve(&mut self, manifest_id: &str) -> anyhow::Result<String>;
}

/// Asks for legs to be run. Injected so the tick logic is testable without GitHub.
pub trait Dispatcher {
    /// Run the legs backed by these manifests, against this release tag.
    fn dispatch(&mut self, manifests: &[String], tag: &str) -> anyhow::Result<()>;
}

/// Answers whether the release has finished publishing.
pub trait PublishWatch {
    fn is_complete(&mut self) -> anyhow::Result<bool>;
}

/// Everything the watcher needs from outside its own decision: cargo, GitHub, crates.io and the
/// clock.
///
/// One struct rather than four parameters because they always travel together, and because what is
/// worth reading at a call site is the decision being made, not the list of things it talks to.
pub struct Seams<'a> {
    pub resolver: &'a mut dyn Resolver,
    pub dispatcher: &'a mut dyn Dispatcher,
    pub publishing: &'a mut dyn PublishWatch,
    pub clock: &'a mut dyn Clock,
}

/// Run the watcher over one release's publish window.
pub fn watch(
    tag: &str,
    manifests: &[String],
    baselines: &BaselineDir,
    cadence: Cadence,
    seams: &mut Seams<'_>,
) -> WatchReport {
    let Seams {
        resolver,
        dispatcher,
        publishing,
        clock,
    } = seams;
    let mut ticks = Vec::new();
    let mut number = 0u32;

    let stopped = loop {
        number += 1;
        let mut tick = Tick {
            number,
            at_secs: clock.elapsed().as_secs(),
            dispatched: vec![],
            unchanged: vec![],
            unresolved: vec![],
        };

        for manifest in manifests {
            match resolver.resolve(manifest) {
                Ok(fresh) => {
                    // `update: true` -- the next tick compares against what this one saw, which is
                    // what makes a quiet tick cheap.
                    match baseline::gate(baselines, manifest, &fresh, true) {
                        Ok(Movement::Unchanged) => tick.unchanged.push(manifest.clone()),
                        Ok(_) => tick.dispatched.push(manifest.clone()),
                        // A baseline that cannot be written is not a reason to skip the legs.
                        Err(e) => {
                            eprintln!("{manifest}: could not record the baseline: {e}");
                            tick.dispatched.push(manifest.clone());
                        }
                    }
                }
                Err(e) => {
                    // Let a leg report this properly, with its retries and its evidence, rather
                    // than deciding here that nothing moved.
                    eprintln!("{manifest}: did not resolve, dispatching anyway: {e}");
                    tick.unresolved.push(manifest.clone());
                    tick.dispatched.push(manifest.clone());
                }
            }
        }

        if tick.dispatched_anything() {
            if let Err(e) = dispatcher.dispatch(&tick.dispatched, tag) {
                eprintln!("dispatch failed: {e}");
                ticks.push(tick);
                break Stopped::DispatchFailed;
            }
        }
        ticks.push(tick);

        // Asked after dispatching, so the final state of the graph is always checked by a tick
        // before the window closes. Asking first would end the watch on the tick that completed
        // publication and leave its last few crates unbuilt until the floor tier.
        match publishing.is_complete() {
            Ok(true) => break Stopped::PublishingComplete,
            Ok(false) => {}
            // Not fatal. crates.io being briefly unreachable is the same class of event the legs
            // retry through, and the ceiling still bounds the run.
            Err(e) => eprintln!("could not read publish status, continuing: {e}"),
        }

        // Checked before sleeping rather than after, so the run cannot overshoot the ceiling by an
        // interval.
        if clock.elapsed() + cadence.interval >= cadence.ceiling {
            break Stopped::CeilingReached;
        }
        clock.sleep(cadence.interval);
    };

    WatchReport {
        tag: tag.to_string(),
        ticks,
        stopped,
    }
}

/// Resolves a manifest by rendering it and running `cargo generate-lockfile`.
///
/// Renders from the manifest's *first* leg. Every leg of a manifest renders the same dependencies —
/// the body is a property of the scenario, so all its legs share it, and the only per-leg difference
/// is a comment naming the leg. A test asserts that, because if it stopped being true this resolve
/// would be answering a question about a different graph than the legs build.
pub struct CargoResolver {
    /// One subdirectory per manifest. Reused across ticks, which keeps the registry cache warm.
    pub root: std::path::PathBuf,
    /// The leg matrix, to find a leg per manifest.
    pub legs: Vec<crate::scenarios::Leg>,
    /// Bound on one resolve, so a stalled registry read cannot hang the tick.
    pub budget: Duration,
    pub verbose: bool,
}

impl Resolver for CargoResolver {
    fn resolve(&mut self, manifest_id: &str) -> anyhow::Result<String> {
        let leg = self
            .legs
            .iter()
            .find(|l| l.manifest == manifest_id)
            .ok_or_else(|| anyhow::anyhow!("no leg uses manifest {manifest_id:?}"))?;
        let scenario = crate::scenarios::manifest(manifest_id)
            .ok_or_else(|| anyhow::anyhow!("unknown manifest {manifest_id:?}"))?;

        let dir = self.root.join(manifest_id);
        crate::manifest::materialize(&crate::manifest::render(leg, scenario), &dir)?;

        // Remove the previous tick's lockfile first. Leaving it in place would let cargo keep the
        // versions it already recorded, so a tick could resolve the graph it saw fifteen minutes ago
        // and report that nothing moved -- which is the one wrong answer the gate can give.
        let lockfile = dir.join("Cargo.lock");
        if lockfile.exists() {
            std::fs::remove_file(&lockfile)
                .with_context(|| format!("removing {}", lockfile.display()))?;
        }

        // Reuses the leg executor for its bounded wait and its pipe draining, rather than
        // reimplementing either. No `--offline`: this is the step that reaches the registry on
        // purpose.
        let mut exec = crate::leg::RealExec {
            dir: dir.clone(),
            verbose: self.verbose,
        };
        let command = crate::leg::Command {
            step: crate::classify::Step::GenerateLockfile,
            program: "cargo".into(),
            args: vec!["generate-lockfile".into()],
            json: false,
        };
        let outcome = crate::leg::Exec::run(&mut exec, &command, self.budget);
        anyhow::ensure!(
            outcome.succeeded(),
            "cargo generate-lockfile failed for {manifest_id}: {}",
            outcome.stderr.trim()
        );

        std::fs::read_to_string(&lockfile)
            .with_context(|| format!("reading {}", lockfile.display()))
    }
}

/// Asks crates.io whether every declared runtime crate has arrived.
///
/// The declared set is read from the checkout once: it is a property of the tag being released, so
/// re-reading it every tick would be thirteen file reads an hour for an answer that cannot change.
/// Only the crates.io lookups repeat.
pub struct CratesIoWatch {
    declared: Vec<crate::publish::RuntimeCrate>,
    index: crate::publish::CratesIo,
}

impl CratesIoWatch {
    pub fn new(repo_root: &std::path::Path) -> anyhow::Result<Self> {
        Ok(Self {
            declared: crate::publish::declared(repo_root)?,
            index: crate::publish::CratesIo::default(),
        })
    }

    /// How many crates the release is waiting on, for the log.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }
}

impl PublishWatch for CratesIoWatch {
    fn is_complete(&mut self) -> anyhow::Result<bool> {
        let status = crate::publish::status(&self.declared, &self.index)?;
        if !status.is_complete() {
            eprintln!(
                "  {} of {} published; waiting on {}",
                status.arrived,
                status.expected,
                status
                    .outstanding
                    .iter()
                    .map(|c| format!("{} {}", c.name, c.version))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Ok(status.is_complete())
    }
}

/// Dispatches the leg workflow through the `gh` CLI.
///
/// One call per tick carrying every manifest that moved, rather than one call per manifest. The leg
/// workflow's concurrency key includes the scenario, so two ticks that dispatch different sets
/// cannot cancel each other, while a repeated dispatch of the same set supersedes its predecessor.
pub struct GhDispatcher {
    pub workflow: String,
    pub repo: String,
    /// Recorded on the run so a leg's provenance is readable.
    pub tier: String,
    /// Print the command instead of running it. For exercising the watcher without dispatching.
    pub dry_run: bool,
}

impl GhDispatcher {
    /// The argv, separated from running it so it can be asserted without a token.
    fn argv(&self, manifests: &[String], tag: &str) -> Vec<String> {
        vec![
            "workflow".into(),
            "run".into(),
            self.workflow.clone(),
            "--repo".into(),
            self.repo.clone(),
            // The legs build the release, so they run from its tag. This needs the workflow to
            // exist on the default branch, which is what dispatching against any other ref
            // requires.
            "--ref".into(),
            tag.to_string(),
            "-f".into(),
            format!("tier={}", self.tier),
            "-f".into(),
            format!("scenario={}", manifests.join(",")),
            "-f".into(),
            format!("tag={tag}"),
        ]
    }
}

impl Dispatcher for GhDispatcher {
    fn dispatch(&mut self, manifests: &[String], tag: &str) -> anyhow::Result<()> {
        let argv = self.argv(manifests, tag);
        if self.dry_run {
            eprintln!("  would dispatch: gh {}", argv.join(" "));
            return Ok(());
        }
        let out = std::process::Command::new("gh")
            .args(&argv)
            .output()
            .context("running gh workflow run")?;
        anyhow::ensure!(
            out.status.success(),
            "gh workflow run failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        eprintln!("  dispatched {} manifest(s)", manifests.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;

    /// Resolves each manifest to a scripted sequence of lockfiles, one per tick.
    struct FakeResolver {
        /// Per manifest, the lockfile at each tick. The last entry repeats once exhausted.
        script: Vec<(String, Vec<String>)>,
        /// Manifests that fail to resolve, always.
        broken: Vec<String>,
        calls: u32,
        tick: usize,
    }

    impl FakeResolver {
        fn steady(manifests: &[String]) -> Self {
            Self {
                script: manifests
                    .iter()
                    .map(|m| (m.clone(), vec![lock(m, "1.0.0")]))
                    .collect(),
                broken: vec![],
                calls: 0,
                tick: 0,
            }
        }
    }

    impl Resolver for FakeResolver {
        fn resolve(&mut self, manifest_id: &str) -> anyhow::Result<String> {
            // The watcher asks for every manifest in order, so seeing the first one again means a
            // new tick has started. Advancing here rather than from the clock keeps the two fakes
            // independent.
            if manifest_id == self.script[0].0 && self.calls > 0 {
                self.tick += 1;
            }
            self.calls += 1;
            if self.broken.iter().any(|b| b == manifest_id) {
                anyhow::bail!("error: failed to select a version");
            }
            let versions = &self
                .script
                .iter()
                .find(|(m, _)| m == manifest_id)
                .expect("unscripted manifest")
                .1;
            let i = self.tick.min(versions.len() - 1);
            Ok(versions[i].clone())
        }
    }

    fn lock(name: &str, version: &str) -> String {
        format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n")
    }

    #[derive(Default)]
    struct FakeDispatcher {
        calls: Vec<Vec<String>>,
        tags: Vec<String>,
        fail_on_call: Option<usize>,
    }

    impl Dispatcher for FakeDispatcher {
        fn dispatch(&mut self, manifests: &[String], tag: &str) -> anyhow::Result<()> {
            self.calls.push(manifests.to_vec());
            self.tags.push(tag.to_string());
            if self.fail_on_call == Some(self.calls.len()) {
                anyhow::bail!("HTTP 403: Resource not accessible by integration");
            }
            Ok(())
        }
    }

    /// Reports incomplete for `incomplete_for` calls, then complete.
    struct FakePublishing {
        incomplete_for: u32,
        calls: u32,
        error_on_call: Option<u32>,
    }

    impl FakePublishing {
        fn completes_after(n: u32) -> Self {
            Self {
                incomplete_for: n,
                calls: 0,
                error_on_call: None,
            }
        }
        fn never() -> Self {
            Self {
                incomplete_for: u32::MAX,
                calls: 0,
                error_on_call: None,
            }
        }
    }

    impl PublishWatch for FakePublishing {
        fn is_complete(&mut self) -> anyhow::Result<bool> {
            self.calls += 1;
            if self.error_on_call == Some(self.calls) {
                anyhow::bail!("curl: (6) Could not resolve host: crates.io");
            }
            Ok(self.calls > self.incomplete_for)
        }
    }

    /// A clock that also advances the resolver, so "a tick later" means "a different graph".
    #[derive(Default)]
    struct FakeClock {
        now: Duration,
        slept: Vec<Duration>,
    }

    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, d: Duration) {
            self.slept.push(d);
            self.now += d;
        }
    }

    fn manifests(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("manifest-{i}")).collect()
    }

    /// The rule the whole window rests on: with no baseline, everything dispatches.
    #[test]
    fn the_first_tick_dispatches_every_manifest() {
        let dir = tempdir("watch-first");
        let ms = manifests(13);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::completes_after(1);
        let mut c = FakeClock::default();

        let report = watch(
            "release-2026-11-09",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.ticks[0].dispatched.len(), 13, "not diff-gated");
        assert!(report.ticks[0].unchanged.is_empty());
        assert_eq!(d.calls[0].len(), 13);
        assert_eq!(d.tags[0], "release-2026-11-09");
    }

    /// The reason the gate exists: a tick with no publish in it must ask for no work.
    #[test]
    fn a_tick_with_no_movement_dispatches_nothing() {
        let dir = tempdir("watch-quiet");
        let ms = manifests(13);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::completes_after(3);
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.ticks.len(), 4, "three incomplete, then complete");
        assert_eq!(report.ticks[0].dispatched.len(), 13, "first sight");
        for tick in &report.ticks[1..] {
            assert!(
                tick.dispatched.is_empty(),
                "tick {} should have been quiet: {:?}",
                tick.number,
                tick.dispatched
            );
            assert_eq!(tick.unchanged.len(), 13);
        }
        assert_eq!(d.calls.len(), 1, "only the first tick asked for work");
        assert_eq!(report.dispatches(), 13);
        assert!(report.ended_cleanly());
    }

    /// And when a crate does land mid-window, only the manifests it touched are dispatched.
    #[test]
    fn only_the_manifests_that_moved_are_dispatched() {
        let dir = tempdir("watch-moved");
        let ms = manifests(3);
        let mut r = FakeResolver::steady(&ms);
        // manifest-1's graph changes on the second tick; the others do not.
        r.script[1].1 = vec![lock("manifest-1", "1.0.0"), lock("manifest-1", "1.1.0")];
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::completes_after(2);
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.ticks[0].dispatched.len(), 3, "first sight");
        assert_eq!(
            report.ticks[1].dispatched,
            ["manifest-1"],
            "only the one that moved"
        );
        assert_eq!(report.ticks[1].unchanged.len(), 2);
        assert_eq!(d.calls.len(), 2);
        assert_eq!(d.calls[1], ["manifest-1"]);
    }

    /// A manifest that will not resolve must still get legs, so that `RESOLVE_FAIL` is reported by
    /// the thing that knows how to report it. Treating it as "unchanged" would silence the verdict
    /// that says consumers cannot resolve the release at all.
    #[test]
    fn a_manifest_that_does_not_resolve_is_still_dispatched() {
        let dir = tempdir("watch-broken");
        let ms = manifests(3);
        let mut r = FakeResolver::steady(&ms);
        r.broken = vec!["manifest-2".to_string()];
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::completes_after(1);
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.ticks[0].unresolved, ["manifest-2"]);
        assert!(
            report.ticks[0]
                .dispatched
                .contains(&"manifest-2".to_string()),
            "it must still be dispatched"
        );
    }

    /// The one error the watcher refuses to continue past: legs that were never dispatched emit no
    /// datapoint, and an absent datapoint cannot raise an alarm.
    #[test]
    fn a_failed_dispatch_stops_the_watch_and_fails_the_job() {
        let dir = tempdir("watch-dispatch-fail");
        let ms = manifests(2);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher {
            fail_on_call: Some(1),
            ..Default::default()
        };
        let mut p = FakePublishing::never();
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.stopped, Stopped::DispatchFailed);
        assert_eq!(report.ticks.len(), 1, "it stops immediately");
        assert_eq!(report.exit_code(), 1);
        assert!(!report.ended_cleanly());
    }

    /// A release that never reports complete is bounded, and the ending says so.
    #[test]
    fn the_ceiling_bounds_a_release_that_never_completes() {
        let dir = tempdir("watch-ceiling");
        let ms = manifests(1);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::never();
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.stopped, Stopped::CeilingReached);
        // 3 h at 15-minute ticks: the twelfth tick is the one with no room to sleep again.
        assert_eq!(report.ticks.len(), 12);
        assert!(
            c.elapsed() < CEILING,
            "it must not overshoot: {:?}",
            c.elapsed()
        );
        assert_eq!(
            report.exit_code(),
            0,
            "a slow release is a finding, not a job failure -- the floor tier still runs"
        );
    }

    /// Completion is asked *after* dispatching, so the tick that sees the final graph still runs.
    /// Asking first would leave a release's last crates unbuilt until the floor tier.
    #[test]
    fn the_tick_that_completes_publishing_still_dispatches_first() {
        let dir = tempdir("watch-order");
        let ms = manifests(1);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher::default();
        // Complete on the very first question.
        let mut p = FakePublishing::completes_after(0);
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.stopped, Stopped::PublishingComplete);
        assert_eq!(report.ticks.len(), 1);
        assert_eq!(
            d.calls.len(),
            1,
            "the window must not close before its first dispatch"
        );
    }

    /// crates.io being briefly unreachable is the class of event the legs retry through, so it must
    /// not end the window.
    #[test]
    fn an_unreadable_publish_status_does_not_end_the_window() {
        let dir = tempdir("watch-status-error");
        let ms = manifests(1);
        let mut r = FakeResolver::steady(&ms);
        let mut d = FakeDispatcher::default();
        let mut p = FakePublishing::completes_after(2);
        p.error_on_call = Some(1);
        let mut c = FakeClock::default();

        let report = watch(
            "release-x",
            &ms,
            &BaselineDir::new(dir.path()),
            Cadence::default(),
            &mut Seams {
                resolver: &mut r,
                dispatcher: &mut d,
                publishing: &mut p,
                clock: &mut c,
            },
        );

        assert_eq!(report.stopped, Stopped::PublishingComplete);
        assert!(report.ticks.len() > 1, "it kept going past the error");
    }

    /// The precondition `CargoResolver` rests on: resolving a manifest from its first leg is only
    /// meaningful if every leg of that manifest renders the same dependencies. The body is a
    /// property of the scenario, so they do — but if that ever stopped being true, the watcher
    /// would be asking about a different graph than the legs build, and nothing else would notice.
    #[test]
    fn every_leg_of_a_manifest_renders_the_same_dependencies() {
        use std::collections::BTreeMap;

        let clamped = crate::toolchain::resolve(
            crate::toolchain::Version::new(1, 99, 1),
            crate::toolchain::Version::new(1, 94, 1),
        )
        .expect("a real span");
        let legs = crate::scenarios::legs(&clamped.kept);

        let mut by_manifest: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
        for leg in &legs {
            let scenario = crate::scenarios::manifest(&leg.manifest).expect("a known manifest");
            let rendered = crate::manifest::render(leg, scenario);
            // Drop the header comments, which name the leg on purpose.
            let deps: String = rendered
                .cargo_toml
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n");
            by_manifest
                .entry(&leg.manifest)
                .or_default()
                .push((leg.id.clone(), deps));
        }

        for (manifest, rendered) in by_manifest {
            let (first_id, first) = &rendered[0];
            for (id, deps) in &rendered[1..] {
                assert_eq!(
                    first, deps,
                    "{manifest}: {first_id} and {id} render different manifests, so resolving \
                     from one leg would not describe the other"
                );
            }
        }
    }

    /// The dispatch argv, asserted rather than run, since running it needs a token.
    #[test]
    fn the_dispatch_names_the_moved_manifests_and_the_tag() {
        let d = GhDispatcher {
            workflow: "build-canary.yml".into(),
            repo: "smithy-lang/smithy-rs".into(),
            tier: "dispatch".into(),
            dry_run: true,
        };
        let argv = d.argv(
            &["new-project".to_string(), "ssstar".to_string()],
            "release-2026-11-09",
        );

        assert_eq!(
            argv,
            [
                "workflow",
                "run",
                "build-canary.yml",
                "--repo",
                "smithy-lang/smithy-rs",
                "--ref",
                "release-2026-11-09",
                "-f",
                "tier=dispatch",
                // Comma-joined, which is what the leg workflow's filter accepts.
                "-f",
                "scenario=new-project,ssstar",
                "-f",
                "tag=release-2026-11-09",
            ]
        );
        // The legs must build the release, not whatever the default branch happens to be.
        let after_ref = argv.iter().position(|a| a == "--ref").unwrap() + 1;
        assert_eq!(argv[after_ref], "release-2026-11-09");
    }

    /// A dry run must not shell out, so the watcher can be exercised end to end without
    /// dispatching anything.
    #[test]
    fn a_dry_run_dispatch_succeeds_without_running_gh() {
        let mut d = GhDispatcher {
            workflow: "build-canary.yml".into(),
            repo: "nonexistent/repo".into(),
            tier: "dispatch".into(),
            dry_run: true,
        };
        assert!(d
            .dispatch(&["new-project".to_string()], "release-x")
            .is_ok());
    }

    #[test]
    fn the_interval_and_ceiling_are_consistent_with_the_leg_budget() {
        // A leg's own deadline has to be under the tick, or a leg could outlive the tick that
        // dispatched it and be re-dispatched alongside itself.
        assert!(
            crate::leg::BUDGET < INTERVAL,
            "leg budget {:?} must be under the tick {:?}",
            crate::leg::BUDGET,
            INTERVAL
        );
        // And the ceiling has to be a whole number of ticks' worth, or the last one is wasted.
        assert_eq!(CEILING.as_secs() % INTERVAL.as_secs(), 0);
        assert_eq!(CEILING.as_secs() / INTERVAL.as_secs(), 12);
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The last graph each manifest resolved to, and whether it has moved since.
//!
//! The dispatch tier wakes every 15 minutes for the length of a release. A tick with no publish in
//! it resolves exactly what the previous tick did, so running the whole matrix to re-prove that
//! graph buys nothing. So each tick resolves every manifest, compares each against the baseline,
//! and dispatches legs only where the graph actually moved.
//!
//! # Why this is a directory and not a bucket
//!
//! A baseline never has to outlive the release it was taken during. The first tick of a release has
//! no baseline and dispatches everything, which is the right answer anyway — a release just
//! published, so the graph has certainly moved — and the floor tier that follows has no gate at all.
//! So the whole useful life of a baseline is one orchestrator job, which ticks every 15 minutes for
//! at most three hours.
//!
//! That makes this a directory. Where the directory lives is a workflow decision and not a decision
//! this module has to know about: `RUNNER_TEMP` is enough for a single job, and wrapping it in
//! `actions/cache` — which caches a directory — adds survival across a manual re-run for one line
//! of YAML. Cache entries are evicted after seven days without access and under a size cap, neither
//! of which can reach a baseline being read twelve times an hour, and an evicted entry just
//! dispatches everything.
//!
//! # Why the key is per manifest and not per leg
//!
//! Five legs share each manifest, one per compiler version, and they all resolve the same graph: the
//! rendered manifests pin `edition = "2021"`, which pins the resolver to v2. Resolver v3 is
//! MSRV-aware and *would* resolve differently per toolchain — which is why the edition is pinned,
//! and the same pin is what makes one baseline per manifest correct rather than a simplification.
//!
//! # What counts as movement
//!
//! Any package changing version, appearing or disappearing — not just the crates this repository
//! publishes. This is the opposite of [`crate::lockfile::Resolved::duplicated_crates_under_test`],
//! which filters hard, and the difference is deliberate: that filter answers "who should a
//! responder look at", where third-party noise buries the answer, while this one answers "could the
//! build differ now", where a third-party crate bumping is just as capable of breaking a consumer
//! and is a real post-publish failure class. Filtering here would make the canary blind to it on
//! every tick of the dispatch tier.
//!
//! The comparison is on the parsed graph rather than the file's bytes, so a lockfile format change
//! does not read as a whole matrix's worth of movement. The bytes are what get stored, though, so a
//! responder gets the real file.
//!
//! # Every failure dispatches
//!
//! A baseline that is absent, unreadable or unparseable reports [`Movement::NeverSeen`], and so
//! dispatches. That is the safe direction in every case: the tier falls back to the behaviour it
//! would have had with no gate at all, which costs runner minutes and detects everything. The one
//! outcome that must never happen by accident is the opposite — reading a broken baseline as
//! "unchanged" and skipping legs that never ran.
//!
//! # A manifest that will not resolve at all
//!
//! The gate above compares graphs, so it only has anything to say when a manifest *resolved*. A
//! manifest that does not resolve has no graph to compare, and the watcher dispatches it so a leg
//! reports the failure properly rather than the watcher deciding nothing moved.
//!
//! Dispatching it on *every* tick, though, re-runs the same matrix to reach the same verdict. A
//! three-hour window on a 15-minute tick is twelve ticks, so one manifest that cannot resolve for
//! the length of a release asked for twelve identical matrices — measured, by running the watcher
//! against a registry that could not serve six of the manifests: all six appeared in `dispatched`
//! on all six ticks.
//!
//! That is what [`gate_failure`] is for. The failure is recorded, and a failure identical to the
//! *previous* tick's is not dispatched again. Three properties make it safe:
//!
//! - **Only an identical, consecutive failure is suppressed.** A different error dispatches, so a
//!   failure that changes shape — including one that changes because the publish window moved on —
//!   is never mistaken for the one already reported.
//! - **The record carries the tick it was written on**, which is what makes a stale record
//!   harmless. There is nothing to clear when a manifest recovers: a record from three ticks ago
//!   cannot suppress anything, because suppression requires the tick immediately before this one.
//! - **A store that cannot be read or written dispatches**, exactly as for the graph baseline.
//!
//! The lockfile baseline is deliberately *not* touched by a failure, so when a manifest recovers,
//! the recovery is compared against the last graph that actually resolved.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;

use crate::lockfile::Resolved;

/// The path a manifest's baseline is stored at, relative to the store's root.
///
/// Stable across ticks: the same manifest yields the same path, which is what lets a read be one
/// lookup with no listing, no sorting and no clock.
pub fn relative_path(manifest_id: &str) -> anyhow::Result<PathBuf> {
    usable_as_a_path(manifest_id)?;
    Ok(PathBuf::from(manifest_id).join("Cargo.lock"))
}

/// The path a manifest's recorded resolve failure is stored at, relative to the store's root.
///
/// A sibling of the lockfile rather than the same file. An error message is not a graph: stored
/// where a lockfile is expected it would parse to an empty package set, and two unrelated failures
/// would then compare equal — the one answer that must never be reached by accident.
pub fn failure_relative_path(manifest_id: &str) -> anyhow::Result<PathBuf> {
    usable_as_a_path(manifest_id)?;
    Ok(PathBuf::from(manifest_id).join("resolve-error"))
}

/// Manifest ids come from a compile-time table, so a traversal here is not a live attack; it is
/// rejected anyway, because the id is being pasted into a filesystem path and a bad one would
/// silently read or write somewhere else.
fn usable_as_a_path(manifest_id: &str) -> anyhow::Result<()> {
    let ok = !manifest_id.is_empty()
        && manifest_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !manifest_id.contains("..");
    if !ok {
        bail!("manifest id {manifest_id:?} is not usable as a path component");
    }
    Ok(())
}

/// One package whose resolution differs between the baseline and now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub name: String,
    /// Versions in the baseline. Empty means the package was not in the graph before.
    pub before: Vec<String>,
    /// Versions now. Empty means it has dropped out.
    pub after: Vec<String>,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |v: &[String]| match v {
            [] => "absent".to_string(),
            some => some.join(", "),
        };
        write!(
            f,
            "{}: {} -> {}",
            self.name,
            list(&self.before),
            list(&self.after)
        )
    }
}

/// Whether a manifest's graph has moved since the baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "movement", rename_all = "kebab-case")]
pub enum Movement {
    /// No usable baseline. Dispatch, because nothing is known about this manifest — this is the
    /// first tick of a release, and what every read failure degrades to.
    NeverSeen,
    /// The same graph. Do not dispatch; the previous tick already proved this one.
    Unchanged,
    /// The graph moved, and here is how.
    Moved { changes: Vec<Change> },
}

impl Movement {
    /// Whether legs should be dispatched for this manifest. Everything except an established,
    /// unchanged graph dispatches.
    pub fn should_dispatch(&self) -> bool {
        !matches!(self, Movement::Unchanged)
    }
}

impl fmt::Display for Movement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Movement::NeverSeen => f.write_str("no baseline, so dispatching"),
            Movement::Unchanged => f.write_str("graph unchanged, not dispatching"),
            Movement::Moved { changes } => {
                write!(f, "{} package(s) moved: ", changes.len())?;
                let shown: Vec<String> = changes.iter().take(5).map(Change::to_string).collect();
                f.write_str(&shown.join("; "))?;
                if changes.len() > shown.len() {
                    write!(f, "; and {} more", changes.len() - shown.len())?;
                }
                Ok(())
            }
        }
    }
}

/// Whether a manifest's failure to resolve is worth dispatching legs for.
///
/// See the module docs: a manifest that will not resolve is a finding, but it is the same finding
/// on every tick of the window until something about it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolveFailure {
    /// The first failure, a failure different from the previous tick's, or a failure after a tick
    /// that resolved. Dispatch: a leg reports it properly, with its retries and its evidence.
    New,
    /// The previous tick failed in exactly the same way, and legs were dispatched for it then.
    /// Dispatching again would re-run the same matrix to reach the same verdict.
    Repeated,
}

impl ResolveFailure {
    /// Whether legs should be dispatched. Only an identical, consecutive repeat is suppressed.
    pub fn should_dispatch(&self) -> bool {
        matches!(self, ResolveFailure::New)
    }
}

impl fmt::Display for ResolveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveFailure::New => f.write_str("a resolve failure not seen on the previous tick"),
            ResolveFailure::Repeated => {
                f.write_str("the same resolve failure as the previous tick, already dispatched")
            }
        }
    }
}

/// The stored form of a failure: the tick it happened on, then the error.
///
/// The tick number is load-bearing rather than decorative. It is what lets a record be left behind
/// when a manifest recovers: suppression requires the stored tick to be the one immediately before
/// the current one, so there is nothing to clear on success and a failed write degrades to
/// dispatching rather than to skipping.
fn encode_failure(tick: u32, error: &str) -> String {
    format!("{tick}\n{error}")
}

/// Split a stored record back into its tick and its error, or `None` if it is not one.
fn decode_failure(record: &str) -> Option<(u32, &str)> {
    let (first, rest) = record.split_once('\n')?;
    Some((first.trim().parse().ok()?, rest))
}

/// Compare a freshly resolved lockfile against a stored baseline.
///
/// An absent *or unparseable* baseline is [`Movement::NeverSeen`]: a baseline that cannot be read
/// is not evidence that nothing changed, and treating it as such would skip the legs.
pub fn compare(baseline: Option<&str>, fresh: &str) -> anyhow::Result<Movement> {
    let fresh = Resolved::parse(fresh).context("parsing the freshly resolved lockfile")?;
    let Some(baseline) = baseline.and_then(|b| Resolved::parse(b).ok()) else {
        return Ok(Movement::NeverSeen);
    };

    let mut changes = Vec::new();
    for (name, after) in fresh.packages() {
        let before = baseline.versions_of(name);
        if before != after {
            changes.push(Change {
                name: name.to_string(),
                before: before.to_vec(),
                after: after.to_vec(),
            });
        }
    }
    // Packages that dropped out entirely. Walking only the fresh graph would miss them, and a
    // dependency disappearing changes what compiles just as much as one appearing.
    for (name, before) in baseline.packages() {
        if fresh.versions_of(name).is_empty() {
            changes.push(Change {
                name: name.to_string(),
                before: before.to_vec(),
                after: vec![],
            });
        }
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(match changes.is_empty() {
        true => Movement::Unchanged,
        false => Movement::Moved { changes },
    })
}

/// Where the baselines are kept: a directory, one subdirectory per manifest.
///
/// See the module docs for why a directory and not a bucket.
pub struct BaselineDir {
    pub root: PathBuf,
}

impl BaselineDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, manifest_id: &str) -> anyhow::Result<PathBuf> {
        Ok(self.root.join(relative_path(manifest_id)?))
    }

    /// The stored baseline, or `None` if there is not one to be had.
    ///
    /// A read that fails for a reason other than absence says so on stderr and still answers
    /// `None`, because dispatching is the safe response to every read failure and there is nothing
    /// better to do with the error.
    pub fn get(&self, manifest_id: &str) -> anyhow::Result<Option<String>> {
        Ok(read_if_present(
            &self.path(manifest_id)?,
            "baseline",
            manifest_id,
        ))
    }

    /// Replace the baseline. Errors, deliberately: a store that cannot be written to must not look
    /// like a store that is merely empty, or every tick would read "never seen" and the gate would
    /// appear to work while doing nothing.
    pub fn put(&self, manifest_id: &str, lockfile: &str) -> anyhow::Result<()> {
        write_replacing(&self.path(manifest_id)?, lockfile)
    }

    fn failure_path(&self, manifest_id: &str) -> anyhow::Result<PathBuf> {
        Ok(self.root.join(failure_relative_path(manifest_id)?))
    }

    /// The failure recorded for this manifest, or `None`.
    ///
    /// Answers `None` on any read failure other than absence, for the same reason [`Self::get`]
    /// does: without a record the failure reads as new and dispatches, which is the safe direction.
    pub fn get_failure(&self, manifest_id: &str) -> anyhow::Result<Option<String>> {
        Ok(read_if_present(
            &self.failure_path(manifest_id)?,
            "failure record",
            manifest_id,
        ))
    }

    /// Record that this manifest failed to resolve on this tick, with this error.
    ///
    /// Errors, deliberately, like [`Self::put`]: a store that silently accepts nothing would make
    /// every failure read as new, which is merely wasteful — but the same silence on the read side
    /// is what would make a failure read as a repeat, so neither side may be guessed at.
    pub fn put_failure(&self, manifest_id: &str, tick: u32, error: &str) -> anyhow::Result<()> {
        write_replacing(
            &self.failure_path(manifest_id)?,
            &encode_failure(tick, error),
        )
    }
}

/// Read a stored record, treating absence and unreadability alike.
///
/// `what` names the record in the message, because "no usable baseline" and "no usable failure
/// record" lead a reader to different places.
fn read_if_present(path: &Path, what: &str, manifest_id: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!(
                "no usable {what} for {manifest_id} at {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Write a record, creating the manifest's directory if this is its first one.
fn write_replacing(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Resolve the gate for one manifest: compare, and optionally make the fresh graph the new baseline.
///
/// Updating is the caller's choice because the two uses want different things. The dispatch tier
/// updates, so the next tick's comparison is against what it last saw. A one-off run that is only
/// asking "has anything moved" must not move the baseline, or it would silence the next real tick.
pub fn gate(
    store: &BaselineDir,
    manifest_id: &str,
    fresh: &str,
    update: bool,
) -> anyhow::Result<Movement> {
    let baseline = store.get(manifest_id)?;
    let movement = compare(baseline.as_deref(), fresh)?;
    if update {
        store.put(manifest_id, fresh)?;
    }
    Ok(movement)
}

/// Resolve the gate for a manifest that would not resolve at all.
///
/// `tick` is the current tick's number, counting from one. A failure is a [`ResolveFailure::Repeated`]
/// only when the record says the *immediately preceding* tick failed with the same error — so a
/// record left behind by a manifest that has since recovered suppresses nothing, and no cleanup is
/// needed on the success path.
///
/// Does not touch the lockfile baseline. A manifest that recovers should be compared against the
/// last graph that actually resolved, not against nothing.
pub fn gate_failure(
    store: &BaselineDir,
    manifest_id: &str,
    error: &str,
    tick: u32,
    update: bool,
) -> anyhow::Result<ResolveFailure> {
    let previous = store.get_failure(manifest_id)?;
    let repeated = previous
        .as_deref()
        .and_then(decode_failure)
        .is_some_and(|(at, text)| tick.checked_sub(1) == Some(at) && text == error);
    if update {
        store.put_failure(manifest_id, tick, error)?;
    }
    Ok(match repeated {
        true => ResolveFailure::Repeated,
        false => ResolveFailure::New,
    })
}

/// Read a lockfile from disk, for the CLI.
pub fn read_lockfile(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;

    const BASE: &str = r#"
version = 4

[[package]]
name = "aws-smithy-types"
version = "1.9.4"

[[package]]
name = "serde"
version = "1.0.230"
"#;

    fn lock(packages: &[(&str, &str)]) -> String {
        let mut s = String::from("version = 4\n");
        for (n, v) in packages {
            s.push_str(&format!(
                "\n[[package]]\nname = \"{n}\"\nversion = \"{v}\"\n"
            ));
        }
        s
    }

    #[test]
    fn the_path_is_stable_and_per_manifest() {
        assert_eq!(
            relative_path("prior-aws-config-1-10").unwrap(),
            PathBuf::from("prior-aws-config-1-10/Cargo.lock")
        );
        // Called twice, a tick apart, it has to be the same path -- that is the whole point.
        assert_eq!(
            relative_path("new-project").unwrap(),
            relative_path("new-project").unwrap()
        );
    }

    /// Every id the scenario table actually uses has to be usable as a path component.
    #[test]
    fn every_real_manifest_id_yields_a_path() {
        let manifests = crate::scenarios::manifests();
        assert_eq!(manifests.len(), 10, "the table has ten live manifests");
        for scenario in manifests {
            let id = scenario.id;
            let p = relative_path(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(p.components().count(), 2, "{p:?} should be <id>/Cargo.lock");
            assert!(p.ends_with("Cargo.lock"), "{p:?}");
        }
    }

    #[test]
    fn a_path_cannot_escape_the_store() {
        for bad in [
            "../evil",
            "a/b",
            "..",
            "",
            "x/../../y",
            "with space",
            "/abs",
        ] {
            assert!(relative_path(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn no_baseline_means_dispatch() {
        let m = compare(None, BASE).unwrap();
        assert_eq!(m, Movement::NeverSeen);
        assert!(m.should_dispatch());
    }

    /// The case the gate exists to catch: a tick with no publish in it.
    #[test]
    fn an_identical_graph_does_not_dispatch() {
        let m = compare(Some(BASE), BASE).unwrap();
        assert_eq!(m, Movement::Unchanged);
        assert!(!m.should_dispatch());
    }

    /// Byte-for-byte equality is not the test. Reformatting, reordering or a lockfile version bump
    /// must not read as movement, or a cargo release would dispatch everything.
    #[test]
    fn the_same_graph_in_a_different_shape_is_still_unchanged() {
        let reordered = r#"
version = 3

[[package]]
name = "serde"
version = "1.0.230"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "aws-smithy-types"
version = "1.9.4"
dependencies = ["serde"]
"#;
        assert_ne!(reordered.trim(), BASE.trim(), "the bytes do differ");
        assert_eq!(compare(Some(BASE), reordered).unwrap(), Movement::Unchanged);
    }

    #[test]
    fn a_bumped_version_is_movement_and_says_what_moved() {
        let fresh = lock(&[("aws-smithy-types", "1.9.5"), ("serde", "1.0.230")]);
        let Movement::Moved { changes } = compare(Some(BASE), &fresh).unwrap() else {
            panic!("a version bump has to move");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].to_string(), "aws-smithy-types: 1.9.4 -> 1.9.5");
    }

    /// The deliberate difference from the diagnosis filter. A third-party crate bumping can break a
    /// consumer's build exactly as well as one of ours, so the gate must fire on it; filtering here
    /// would make the dispatch tier blind to that failure class on every tick.
    #[test]
    fn a_third_party_crate_moving_also_dispatches() {
        let fresh = lock(&[("aws-smithy-types", "1.9.4"), ("serde", "1.0.231")]);
        let m = compare(Some(BASE), &fresh).unwrap();
        assert!(m.should_dispatch(), "{m}");
        assert_eq!(
            m,
            Movement::Moved {
                changes: vec![Change {
                    name: "serde".into(),
                    before: vec!["1.0.230".into()],
                    after: vec!["1.0.231".into()],
                }]
            }
        );
    }

    #[test]
    fn an_added_package_is_movement() {
        let fresh = lock(&[
            ("aws-smithy-types", "1.9.4"),
            ("serde", "1.0.230"),
            ("tokio", "1.49.0"),
        ]);
        let Movement::Moved { changes } = compare(Some(BASE), &fresh).unwrap() else {
            panic!("a new dependency has to move");
        };
        assert_eq!(changes[0].to_string(), "tokio: absent -> 1.49.0");
    }

    /// Walking only the fresh graph would miss this, and a dependency dropping out changes what
    /// compiles just as much as one appearing.
    #[test]
    fn a_removed_package_is_movement() {
        let fresh = lock(&[("aws-smithy-types", "1.9.4")]);
        let Movement::Moved { changes } = compare(Some(BASE), &fresh).unwrap() else {
            panic!("a dropped dependency has to move");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].to_string(), "serde: 1.0.230 -> absent");
    }

    /// A duplicated major appearing is movement even though the crate was already in the graph.
    #[test]
    fn a_second_version_of_an_existing_package_is_movement() {
        let fresh = lock(&[
            ("aws-smithy-types", "1.9.4"),
            ("aws-smithy-types", "0.60.0"),
            ("serde", "1.0.230"),
        ]);
        let Movement::Moved { changes } = compare(Some(BASE), &fresh).unwrap() else {
            panic!("a new duplicate has to move");
        };
        assert_eq!(
            changes[0].to_string(),
            "aws-smithy-types: 1.9.4 -> 0.60.0, 1.9.4"
        );
    }

    /// A baseline that cannot be parsed is not evidence that nothing changed. Treating it as
    /// unchanged would skip the legs, which is the one outcome that must never happen by accident.
    #[test]
    fn an_unparseable_baseline_dispatches_rather_than_skipping() {
        let m = compare(Some("this is not a lockfile {{{"), BASE).unwrap();
        assert_eq!(m, Movement::NeverSeen);
        assert!(m.should_dispatch());
    }

    /// The fresh side is the opposite: it is produced by cargo in this job, so if it will not parse
    /// something is wrong here and now, and guessing would hide it.
    #[test]
    fn an_unparseable_fresh_lockfile_is_an_error() {
        assert!(compare(Some(BASE), "name = ").is_err());
    }

    #[test]
    fn a_round_trip_returns_what_was_written() {
        let dir = tempdir("baseline-roundtrip");
        let store = BaselineDir::new(dir.path());

        assert_eq!(store.get("new-project").unwrap(), None, "nothing yet");
        store.put("new-project", BASE).unwrap();
        assert_eq!(store.get("new-project").unwrap().as_deref(), Some(BASE));

        // Written at the stable path, so a reader does not have to search.
        assert!(dir.path().join("new-project/Cargo.lock").is_file());

        // And overwriting replaces in place rather than accumulating.
        let fresh = lock(&[("aws-smithy-types", "1.9.5")]);
        store.put("new-project", &fresh).unwrap();
        assert_eq!(store.get("new-project").unwrap().as_deref(), Some(&*fresh));
    }

    #[test]
    fn baselines_for_different_manifests_do_not_collide() {
        let dir = tempdir("baseline-collide");
        let store = BaselineDir::new(dir.path());
        store.put("new-project", BASE).unwrap();
        store
            .put("prior-aws-config-1-10", &lock(&[("aws-config", "1.10.1")]))
            .unwrap();

        assert_eq!(store.get("new-project").unwrap().as_deref(), Some(BASE));
        assert!(store
            .get("prior-aws-config-1-10")
            .unwrap()
            .unwrap()
            .contains("aws-config"));
    }

    /// The gate's two modes. Updating is what makes the *next* tick meaningful; not updating is
    /// what keeps a one-off query from silencing it.
    #[test]
    fn the_gate_updates_the_baseline_only_when_asked() {
        let dir = tempdir("baseline-gate");
        let store = BaselineDir::new(dir.path());
        let fresh = lock(&[("aws-smithy-types", "1.9.5")]);

        // First sight dispatches, and without --update the store stays empty, so a second query
        // still says never seen rather than unchanged.
        assert_eq!(
            gate(&store, "new-project", &fresh, false).unwrap(),
            Movement::NeverSeen
        );
        assert_eq!(
            gate(&store, "new-project", &fresh, false).unwrap(),
            Movement::NeverSeen,
            "a read-only gate must not silence the next tick"
        );

        // With --update the same graph then reads as unchanged -- the tick that saves a whole matrix.
        assert_eq!(
            gate(&store, "new-project", &fresh, true).unwrap(),
            Movement::NeverSeen
        );
        assert_eq!(
            gate(&store, "new-project", &fresh, true).unwrap(),
            Movement::Unchanged
        );

        // And a publish lands: movement, then quiet again.
        let bumped = lock(&[("aws-smithy-types", "1.9.6")]);
        assert!(gate(&store, "new-project", &bumped, true)
            .unwrap()
            .should_dispatch());
        assert_eq!(
            gate(&store, "new-project", &bumped, true).unwrap(),
            Movement::Unchanged
        );
    }

    const FAILURE: &str = "error: no matching package named `aws-sdk-s3` found";

    /// What the failure gate exists for: a manifest that cannot resolve for the length of the
    /// window is one finding, not one per tick.
    #[test]
    fn an_identical_failure_on_the_next_tick_does_not_dispatch() {
        let dir = tempdir("failure-repeat");
        let store = BaselineDir::new(dir.path());

        let first = gate_failure(&store, "new-project", FAILURE, 1, true).unwrap();
        assert_eq!(first, ResolveFailure::New);
        assert!(first.should_dispatch(), "{first}");

        let second = gate_failure(&store, "new-project", FAILURE, 2, true).unwrap();
        assert_eq!(second, ResolveFailure::Repeated);
        assert!(!second.should_dispatch(), "{second}");

        // And it stays suppressed for as long as it keeps failing the same way: twelve ticks is a
        // full three-hour window.
        for tick in 3..=12 {
            assert_eq!(
                gate_failure(&store, "new-project", FAILURE, tick, true).unwrap(),
                ResolveFailure::Repeated,
                "tick {tick}"
            );
        }
    }

    #[test]
    fn a_different_failure_dispatches() {
        let dir = tempdir("failure-changed");
        let store = BaselineDir::new(dir.path());
        gate_failure(&store, "new-project", FAILURE, 1, true).unwrap();
        assert_eq!(
            gate_failure(
                &store,
                "new-project",
                "error: something else entirely",
                2,
                true
            )
            .unwrap(),
            ResolveFailure::New
        );
    }

    /// A failure recorded on a tick that is not the immediately preceding one is a new finding.
    /// That is how a manifest which recovered and then broke again is handled, with nothing to
    /// clear on the success path.
    #[test]
    fn a_failure_that_skips_a_tick_dispatches() {
        let dir = tempdir("failure-gap");
        let store = BaselineDir::new(dir.path());
        gate_failure(&store, "new-project", FAILURE, 1, true).unwrap();
        assert_eq!(
            gate_failure(&store, "new-project", FAILURE, 3, true).unwrap(),
            ResolveFailure::New,
            "tick 2 resolved, so this is not the failure already reported"
        );
        // From there the consecutive rule applies again.
        assert_eq!(
            gate_failure(&store, "new-project", FAILURE, 4, true).unwrap(),
            ResolveFailure::Repeated
        );
    }

    /// The record is for a responder too, who should be able to read what would not resolve.
    #[test]
    fn the_failure_record_sits_beside_the_lockfile_and_is_readable() {
        let dir = tempdir("failure-record");
        let store = BaselineDir::new(dir.path());
        store.put_failure("new-project", 7, FAILURE).unwrap();

        let path = dir.path().join("new-project/resolve-error");
        assert!(path.is_file(), "{path:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(FAILURE), "{text}");
        assert!(text.starts_with("7\n"), "the tick comes first: {text:?}");
    }

    /// Unreadable means "no record", which dispatches. The opposite reading — a corrupt record
    /// counting as a repeat — would skip legs for a failure nobody has seen.
    #[test]
    fn an_unusable_failure_record_dispatches() {
        let dir = tempdir("failure-corrupt");
        let store = BaselineDir::new(dir.path());
        for junk in ["", "no newline here", "not-a-number\nerror: x", "\n"] {
            write_replacing(&dir.path().join("new-project/resolve-error"), junk).unwrap();
            assert_eq!(
                gate_failure(&store, "new-project", "error: x", 2, false).unwrap(),
                ResolveFailure::New,
                "{junk:?} must not read as a repeat"
            );
        }
    }

    #[test]
    fn the_failure_gate_records_only_when_asked() {
        let dir = tempdir("failure-noupdate");
        let store = BaselineDir::new(dir.path());
        assert_eq!(
            gate_failure(&store, "new-project", FAILURE, 1, false).unwrap(),
            ResolveFailure::New
        );
        assert_eq!(
            store.get_failure("new-project").unwrap(),
            None,
            "nothing should have been written"
        );
        assert_eq!(
            gate_failure(&store, "new-project", FAILURE, 2, false).unwrap(),
            ResolveFailure::New,
            "a read-only gate must not suppress the next tick"
        );
    }

    /// A failure must not disturb the graph baseline: when the manifest recovers, the recovery is
    /// compared against the last graph that actually resolved.
    #[test]
    fn recording_a_failure_leaves_the_graph_baseline_alone() {
        let dir = tempdir("failure-keeps-baseline");
        let store = BaselineDir::new(dir.path());
        store.put("new-project", BASE).unwrap();
        gate_failure(&store, "new-project", FAILURE, 1, true).unwrap();
        assert_eq!(store.get("new-project").unwrap().as_deref(), Some(BASE));
        // So recovering to the same graph is correctly quiet -- those legs already ran.
        assert_eq!(
            gate(&store, "new-project", BASE, true).unwrap(),
            Movement::Unchanged
        );
    }

    /// A store that cannot be written to must not be able to answer "repeat". It is the read side
    /// that degrades silently to `New`; the write side errors, so the caller dispatches.
    #[test]
    fn a_failure_store_that_cannot_be_written_errors_rather_than_guessing() {
        let dir = tempdir("failure-unwritable");
        // A file where the manifest's own directory would go, so creating it cannot succeed.
        std::fs::write(dir.path().join("new-project"), "in the way").unwrap();
        let store = BaselineDir::new(dir.path());

        assert!(store.put_failure("new-project", 1, FAILURE).is_err());
        assert!(
            gate_failure(&store, "new-project", FAILURE, 1, true).is_err(),
            "the gate must propagate it, not answer Repeated"
        );
    }

    #[test]
    fn a_failure_path_cannot_escape_the_store() {
        for bad in ["../evil", "a/b", "..", "", "/abs", "with space"] {
            assert!(
                failure_relative_path(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert_eq!(
            failure_relative_path("new-project").unwrap(),
            PathBuf::from("new-project/resolve-error")
        );
    }

    /// The two records for one manifest must not be the same file: an error message stored where a
    /// lockfile is expected parses to an empty package set, so two unrelated failures would compare
    /// equal and a recovery would look like every package appearing at once.
    #[test]
    fn the_two_records_are_different_files() {
        assert_ne!(
            relative_path("new-project").unwrap(),
            failure_relative_path("new-project").unwrap()
        );
        let dir = tempdir("failure-distinct");
        let store = BaselineDir::new(dir.path());
        store.put("new-project", BASE).unwrap();
        store.put_failure("new-project", 1, FAILURE).unwrap();
        assert_eq!(store.get("new-project").unwrap().as_deref(), Some(BASE));
        assert!(store
            .get_failure("new-project")
            .unwrap()
            .unwrap()
            .contains(FAILURE));
    }
}

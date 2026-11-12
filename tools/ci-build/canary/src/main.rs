/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! CLI for the post-publish build canary. See the crate docs for what it is for.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};

use canary::baseline;
use canary::floor;
use canary::leg;
use canary::manifest;
use canary::metric;
use canary::publish;
use canary::scenarios;
use canary::summary;
use canary::toolchain::{self, Version};
use canary::watch;

#[derive(Parser)]
#[command(name = "canary", about = "Post-publish build canary for smithy-rs")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Emit the leg matrix as JSON, for a GitHub Actions `matrix` and for eyeballing.
    ///
    /// Takes the toolchain bounds as arguments rather than discovering them, so the matrix is
    /// reproducible off-runner and testable without a network.
    GenMatrix {
        /// The current stable, e.g. `1.99.1`. Normally `rustc +stable --version`.
        #[arg(long)]
        stable: String,
        /// The `rust-version` the crates under test declare, e.g. `1.94.1`. The span is clamped
        /// here: a compiler below it is not a supported configuration.
        #[arg(long)]
        msrv: String,
        /// Print the matrix indented rather than on one line.
        #[arg(long)]
        pretty: bool,
    },
    /// Print the toolchain span and what the MSRV clamp dropped. Coverage shrinking unnoticed is
    /// the failure this guards, so it is worth a command of its own.
    Toolchains {
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
    },
    /// Render a leg and run it: the six commands, the retries, the deadline.
    ///
    /// Prints the report as JSON on stdout and exits 0 for green or `INFRA_FAIL`, non-zero for any
    /// hard verdict — which is how a floor hour's exit code carries the stop decision.
    RunLeg {
        #[arg(long)]
        leg: String,
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
        /// Where to render the crate. Created if absent.
        #[arg(long)]
        dir: PathBuf,
        /// The release tag this leg was dispatched for. When given, the leg exits 0 without
        /// building if a newer `release-*` tag exists, because a dispatched run outlives the
        /// cancellation of its dispatcher. Omit when running a leg outside a release.
        #[arg(long)]
        tag: Option<String>,
        /// Override the 14-minute self-deadline. For testing only.
        #[arg(long)]
        budget_secs: Option<u64>,
        /// Run whatever is already in `--dir` instead of rendering over it.
        ///
        /// Local debugging only, and never in a release run: a leg's guarantee is that it resolves
        /// the published graph afresh, and reusing a directory means reusing whatever lockfile is
        /// sitting in it. It exists so a leg can be perturbed by hand — a broken assertion, an
        /// unsatisfiable requirement — and the real pipeline run over it, which is how the verdicts
        /// that no scenario produces on purpose get induced.
        #[arg(long)]
        reuse_crate: bool,
    },
    /// Report which runtime crates this checkout declares that crates.io does not yet have.
    ///
    /// The first tier's end condition: empty means the release has finished publishing and the
    /// 24-hour floor tier can start. Exits non-zero while anything is outstanding, so a shell loop
    /// can poll it.
    PublishStatus {
        /// The repository root to read `rust-runtime/*/Cargo.toml` from. Defaults to the current
        /// checkout, which in a release run is the tag.
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Compare a manifest's freshly resolved graph against the stored baseline.
    ///
    /// The dispatch tier's gate: a tick with no publish in it resolves exactly what the previous
    /// tick did, and running the whole matrix to re-prove that buys nothing. Prints it as JSON
    /// on stdout.
    Baseline {
        /// The manifest id, as `gen-matrix` emits it in each leg's `manifest` field.
        #[arg(long)]
        manifest: String,
        /// The freshly resolved `Cargo.lock` to compare.
        #[arg(long)]
        lockfile: PathBuf,
        /// The directory holding the baselines, one subdirectory per manifest.
        ///
        /// A baseline never outlives the release it was taken during, so this is `RUNNER_TEMP` in
        /// CI. Wrapping it in `actions/cache` buys survival across a re-run and changes nothing
        /// here.
        #[arg(long)]
        store_dir: PathBuf,
        /// Make the fresh graph the new baseline.
        ///
        /// The dispatch tier passes this, so the next tick compares against what it last saw.
        /// Leave it off to ask the question without answering it for the next tick, which would
        /// otherwise read as "unchanged" and skip legs that were never run.
        #[arg(long)]
        update: bool,
    },
    /// Record a leg's verdict as the datapoint an alarm can see.
    ///
    /// Writes nothing for a leg that reached no verdict -- green, or superseded. That silence is
    /// what the alarm's `notBreaching` treatment turns back into `OK`, and writing a zero instead
    /// would make "it passed" indistinguishable from "it died without reporting".
    EmitVerdict {
        /// The leg's `report.json`.
        #[arg(long)]
        report: PathBuf,
        /// The release tag, which is the dimension the alarm watches.
        #[arg(long)]
        tag: String,
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
        /// Where to write the payload the AWS CLI reads. `RUNNER_TEMP` in CI.
        #[arg(long)]
        payload_dir: PathBuf,
        /// Print the command and payload instead of calling AWS.
        #[arg(long)]
        dry_run: bool,
    },
    /// Record a watcher failure that left a release unchecked.
    ///
    /// Reads the report `watch` wrote. Emits only for a failed dispatch: the legs it would have
    /// asked for are the only thing that produces a verdict, so a dispatch that did not happen is a
    /// release proceeding with no verification at all. A ceiling reached with crates still
    /// outstanding emits nothing -- that is a finding to read, not a release left unchecked.
    EmitWatchFailure {
        /// The report `watch` wrote.
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        payload_dir: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },

    /// Pick the two full-matrix runs a floor hour's gate compares.
    ///
    /// Prints two lines, each a run id or `-`: this release's most recent completed full-matrix
    /// run, then the previous release's. Two lines rather than JSON because the workflow reads them
    /// straight into shell variables and then has to fetch each one.
    FloorRuns {
        /// `gh run list --json databaseId,status,conclusion,displayTitle,headBranch,createdAt`
        #[arg(long)]
        listing: PathBuf,
        /// The release this watch is for.
        #[arg(long)]
        tag: String,
    },

    /// Decide whether a floor hour is still worth dispatching.
    ///
    /// Skips the hour only when a leg went red that was **not** already red before this release.
    /// See `floor` for the four defects in the rule this replaces, each of which came from asking
    /// about the tag's history rather than about this release against what was already true.
    FloorGate {
        /// `gh run view --json jobs` for this release's run. Omit when there is none yet.
        #[arg(long)]
        this: Option<PathBuf>,
        /// The same for the previous release's run. Omit when there is no previous release, which
        /// degrades the rule to its old behaviour rather than to something untested.
        #[arg(long)]
        baseline: Option<PathBuf>,
    },

    /// Record that a release was never checked, which nothing else can notice.
    ///
    /// Every other signal here starts from a leg reaching a verdict. When no leg reaches anything
    /// there is no verdict to be red, green or even `INFRA_FAIL`, so the alarm sees nothing, the
    /// archive writes nothing, and the per-leg "no report" warning cannot fire because no leg job
    /// ran to print it. A release going entirely unverified is the condition this canary exists to
    /// remove, and without this it is the one condition it would not report.
    ///
    /// Reads the decision `summarize` wrote. Emits nothing unless every leg is unreported, so it is
    /// safe to run on every release, and the release name comes from the decision rather than an
    /// argument so a datapoint cannot be recorded against a release the canary was not watching.
    EmitUnchecked {
        /// The decision `summarize` printed on stdout.
        #[arg(long)]
        decision: PathBuf,
        #[arg(long)]
        payload_dir: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Watch a release's publish window, dispatching legs for the manifests whose graph moved.
    ///
    /// One run per release. Resolves every manifest on a 15-minute tick and stops when
    /// every declared runtime crate has arrived, or at a three-hour ceiling.
    Watch {
        /// The release tag being watched. Legs are dispatched against it.
        #[arg(long)]
        tag: String,
        /// The repository root, to read the declared runtime crates from.
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
        /// Where to render and resolve the manifests. One subdirectory each, reused across ticks so
        /// the registry cache stays warm.
        #[arg(long)]
        work_dir: PathBuf,
        /// Where the per-manifest baselines live. `RUNNER_TEMP` in CI; see the baseline module for
        /// why this is a directory and not a bucket.
        #[arg(long)]
        baselines: PathBuf,
        /// Print the dispatch commands instead of running them. Exercises everything else --
        /// rendering, resolving, the gate, the end condition -- without asking for any legs.
        #[arg(long)]
        dry_run: bool,
        /// The workflow to dispatch.
        #[arg(long, default_value = "build-canary.yml")]
        workflow: String,
        /// The repository to dispatch in.
        #[arg(long, default_value = "smithy-lang/smithy-rs")]
        gh_repo: String,
        /// Override the 15-minute tick. For testing only.
        #[arg(long)]
        interval_secs: Option<u64>,
        /// Override the 3-hour ceiling. For testing only.
        #[arg(long)]
        ceiling_secs: Option<u64>,
    },
    /// Fold a run's leg reports into one durable record, for a release that broke.
    ///
    /// Writes `summary.json` into `--out` and prints, on stdout, the key it belongs under and
    /// whether it is worth archiving at all. A run where nothing blamed the release is not.
    Summarize {
        /// The `release-*` tag the legs built against.
        #[arg(long)]
        release: String,
        #[arg(long)]
        run_id: String,
        /// A re-run keeps the id and increments this, and both are in the key, so re-running
        /// cannot overwrite the first attempt's record.
        #[arg(long, default_value_t = 1)]
        run_attempt: u32,
        #[arg(long)]
        run_url: String,
        /// `dispatch`, `floor` or `manual`.
        #[arg(long)]
        tier: String,
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
        /// The downloaded artifacts, searched recursively for `report.json`.
        #[arg(long)]
        reports: PathBuf,
        /// Where to write `summary.json`. Created if absent.
        #[arg(long)]
        out: PathBuf,
    },
    /// Materialize one leg's crate into a directory, ready to build.
    GenLeg {
        /// The leg id, as `gen-matrix` emits it. For example `new-project--stable` or `powerpc64`.
        #[arg(long)]
        leg: String,
        #[arg(long)]
        stable: String,
        #[arg(long)]
        msrv: String,
        /// Where to write it.
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::GenMatrix {
            stable,
            msrv,
            pretty,
        } => {
            let clamped = resolve(&stable, &msrv)?;
            eprintln!("{}", clamped.report());
            let legs = scenarios::legs(&clamped.kept);
            eprintln!("legs: {}", legs.len());
            let json = if pretty {
                serde_json::to_string_pretty(&legs)?
            } else {
                serde_json::to_string(&legs)?
            };
            println!("{json}");
        }
        Command::Toolchains { stable, msrv } => {
            println!("{}", resolve(&stable, &msrv)?.report());
        }
        Command::RunLeg {
            leg,
            stable,
            msrv,
            dir,
            tag,
            budget_secs,
            reuse_crate,
        } => {
            if let Some(tag) = &tag {
                if let Some(newest) = newest_release_tag()? {
                    if &newest != tag {
                        eprintln!("superseded by {newest}, not building");
                        // Report it rather than exiting quietly. The only check on a leg that dies
                        // without emitting anything is a check for an absent report, and a
                        // superseded release trips it on every one of its legs — which would train
                        // everyone to ignore the one warning that catches a silent leg.
                        println!("{}", serde_json::to_string(&leg::Report::superseded(&leg))?);
                        return Ok(());
                    }
                }
            }
            let clamped = resolve(&stable, &msrv)?;
            let legs = scenarios::legs(&clamped.kept);
            let found = find_leg(&legs, &leg)?;
            if reuse_crate {
                eprintln!(
                    "WARNING: --reuse-crate: running {} as it already exists on disk. The \
                     published graph is NOT being resolved afresh, so this run proves nothing \
                     about a release.",
                    dir.display()
                );
                anyhow::ensure!(
                    dir.join("Cargo.toml").is_file(),
                    "--reuse-crate needs a crate in {}; run `gen-leg --out` there first",
                    dir.display()
                );
            } else {
                let scenario = scenarios::manifest(&found.manifest)
                    .ok_or_else(|| anyhow::anyhow!("unknown manifest {:?}", found.manifest))?;
                manifest::materialize(&manifest::render(found, scenario), &dir)?;
            }

            let budget = budget_secs.map(Duration::from_secs).unwrap_or(leg::BUDGET);
            let mut exec = leg::RealExec {
                dir: dir.clone(),
                verbose: true,
            };
            let mut clock = leg::RealClock::default();
            let report = leg::run(found, &mut exec, &mut clock, budget);

            println!("{}", serde_json::to_string(&report)?);
            match report.verdict {
                None => eprintln!("{} GREEN in {:?}", report.leg_id, report.elapsed),
                Some(v) => eprintln!(
                    "{} {} at {:?} after {:?} ({:?})",
                    report.leg_id,
                    v,
                    report.failed_step.unwrap(),
                    report.elapsed,
                    report.stopped.unwrap(),
                ),
            }
            // Said in the job log, because that is the first thing anyone opens and a rigged leg
            // is otherwise indistinguishable from a real break there. Said for a green one too: a
            // manifest rigged to break that came back green is a finding about the rig.
            if let Some(why) = &found.induced {
                eprintln!(
                    "NOTE: {} builds a manifest deliberately rigged to break ({why}). This \
                     verdict tests the path from a verdict to a pager; it says nothing about the \
                     release. Exiting 0 so the rest of the window still runs.",
                    report.leg_id
                );
            }
            std::process::exit(report.exit_code());
        }
        Command::PublishStatus { repo } => {
            let declared = publish::declared(&repo)?;
            let status = publish::status(&declared, &publish::CratesIo::default())?;

            eprintln!(
                "{} of {} runtime crates published",
                status.arrived, status.expected
            );
            for c in &status.outstanding {
                eprintln!("  outstanding: {} {}", c.name, c.version);
            }
            for c in &status.yanked {
                eprintln!(
                    "  YANKED, so consumers cannot resolve it: {} {}",
                    c.name, c.version
                );
            }
            println!("{}", serde_json::to_string(&status)?);
            if !status.is_complete() {
                std::process::exit(1);
            }
        }
        Command::Baseline {
            manifest,
            lockfile,
            store_dir,
            update,
        } => {
            anyhow::ensure!(
                scenarios::manifest(&manifest).is_some(),
                "no manifest {manifest:?}; run `gen-matrix` to see the ids"
            );
            let store = baseline::BaselineDir::new(store_dir);
            let fresh = baseline::read_lockfile(&lockfile)?;
            let movement = baseline::gate(&store, &manifest, &fresh, update)?;

            eprintln!("{manifest}: {movement}");
            println!("{}", serde_json::to_string(&movement)?);
        }
        Command::EmitVerdict {
            report,
            tag,
            stable,
            msrv,
            payload_dir,
            dry_run,
        } => {
            let text = std::fs::read_to_string(&report)
                .with_context(|| format!("reading {}", report.display()))?;
            let report: leg::Report = serde_json::from_str(&text)
                .with_context(|| "parsing the leg report".to_string())?;

            let clamped = resolve(&stable, &msrv)?;
            let legs = scenarios::legs(&clamped.kept);
            let found = find_leg(&legs, &report.leg_id)?;

            let mut emitter = metric::AwsCli {
                payload_dir,
                dry_run,
            };
            let written = metric::record(&tag, found, report.verdict, &mut emitter)?;

            match written.is_empty() {
                true => eprintln!(
                    "{}: no verdict, so nothing to record{}",
                    report.leg_id,
                    match report.reached_a_verdict() {
                        true => " (green)",
                        false => " (superseded, which is not green)",
                    }
                ),
                false => {
                    for d in &written {
                        eprintln!("  {d}");
                    }
                }
            }
            println!("{}", serde_json::to_string(&written)?);
        }
        Command::EmitWatchFailure {
            report,
            payload_dir,
            dry_run,
        } => {
            let text = std::fs::read_to_string(&report)
                .with_context(|| format!("reading {}", report.display()))?;
            let report: watch::WatchReport =
                serde_json::from_str(&text).context("parsing the watch report")?;

            // Only a failed dispatch. The tag comes from the report rather than an argument, so a
            // datapoint cannot be recorded against a release the watcher was not watching.
            if report.stopped != watch::Stopped::DispatchFailed {
                eprintln!(
                    "{}: stopped as `{}`, which left the legs dispatched; nothing to record",
                    report.tag,
                    serde_json::to_value(report.stopped)?
                        .as_str()
                        .unwrap_or("unknown")
                );
                println!("[]");
                return Ok(());
            }

            let mut emitter = metric::AwsCli {
                payload_dir,
                dry_run,
            };
            let written = metric::record_component_failure(
                &report.tag,
                metric::WATCHER,
                "dispatch-failed",
                &mut emitter,
            )?;
            for d in &written {
                eprintln!("  {d}");
            }
            println!("{}", serde_json::to_string(&written)?);
        }
        Command::FloorRuns { listing, tag } => {
            let text = std::fs::read_to_string(&listing)
                .with_context(|| format!("reading {}", listing.display()))?;
            let (mine, previous) = floor::newest_floor_runs(&text, &tag)?;

            let show = |r: &Option<floor::FloorRun>| match r {
                Some(r) => r.id.to_string(),
                None => "-".to_string(),
            };
            eprintln!(
                "this release ({tag}): {}; previous release: {}",
                show(&mine),
                match &previous {
                    Some(r) => format!("{} ({})", r.id, r.tag),
                    None => "none found, so every red leg will count as news".to_string(),
                }
            );
            println!("{}", show(&mine));
            println!("{}", show(&previous));
        }
        Command::FloorGate { this, baseline } => {
            let red = |p: &Option<PathBuf>| -> anyhow::Result<Option<_>> {
                match p {
                    None => Ok(None),
                    Some(p) => {
                        let text = std::fs::read_to_string(p)
                            .with_context(|| format!("reading {}", p.display()))?;
                        Ok(Some(floor::red_legs(&text)?))
                    }
                }
            };
            let mine = red(&this)?;
            let known = red(&baseline)?;
            let gate = floor::decide(mine.as_ref(), known.as_ref());

            eprintln!("{}", gate.reason);
            println!("{}", serde_json::to_string(&gate)?);
        }
        Command::EmitUnchecked {
            decision,
            payload_dir,
            dry_run,
        } => {
            let text = std::fs::read_to_string(&decision)
                .with_context(|| format!("reading {}", decision.display()))?;
            let decision: summary::Decision =
                serde_json::from_str(&text).context("parsing the decision")?;

            if !decision.unchecked {
                eprintln!(
                    "{}: legs reported, so the release was checked; nothing to record",
                    decision.release
                );
                println!("[]");
                return Ok(());
            }

            let mut emitter = metric::AwsCli {
                payload_dir,
                dry_run,
            };
            let written = metric::record_component_failure(
                &decision.release,
                metric::RECORD,
                "nothing-ran",
                &mut emitter,
            )?;
            for d in &written {
                eprintln!("  {d}");
            }
            println!("{}", serde_json::to_string(&written)?);
        }
        Command::Watch {
            tag,
            repo,
            stable,
            msrv,
            work_dir,
            baselines,
            dry_run,
            workflow,
            gh_repo,
            interval_secs,
            ceiling_secs,
        } => {
            let clamped = resolve(&stable, &msrv)?;
            let legs = scenarios::legs(&clamped.kept);
            let manifests: Vec<String> = scenarios::manifests()
                .iter()
                .map(|s| s.id.to_string())
                .collect();

            let mut publishing = watch::CratesIoWatch::new(&repo)?;
            eprintln!(
                "watching {tag}: {} manifests, {} declared runtime crates, tick {}s, ceiling {}s",
                manifests.len(),
                publishing.declared_count(),
                interval_secs.unwrap_or_else(|| watch::INTERVAL.as_secs()),
                ceiling_secs.unwrap_or_else(|| watch::CEILING.as_secs()),
            );
            if dry_run {
                eprintln!("--dry-run: no legs will actually be dispatched");
            }

            let mut resolver = watch::CargoResolver {
                root: work_dir,
                legs,
                // Generous relative to a measured resolve of about a second, because the bound is
                // here to turn a stalled registry read into an error rather than to be tight.
                budget: Duration::from_secs(300),
                verbose: true,
            };
            let mut dispatcher = watch::GhDispatcher {
                workflow,
                repo: gh_repo,
                tier: "dispatch".to_string(),
                dry_run,
            };
            let default = watch::Cadence::default();
            let cadence = watch::Cadence {
                interval: interval_secs
                    .map(Duration::from_secs)
                    .unwrap_or(default.interval),
                ceiling: ceiling_secs
                    .map(Duration::from_secs)
                    .unwrap_or(default.ceiling),
            };
            let mut clock = leg::RealClock::default();

            let report = watch::watch(
                &tag,
                &manifests,
                &baseline::BaselineDir::new(baselines),
                cadence,
                &mut watch::Seams {
                    resolver: &mut resolver,
                    dispatcher: &mut dispatcher,
                    publishing: &mut publishing,
                    clock: &mut clock,
                },
            );

            eprintln!("{report}");
            println!("{}", serde_json::to_string(&report)?);
            std::process::exit(report.exit_code());
        }
        Command::Summarize {
            release,
            run_id,
            run_attempt,
            run_url,
            tier,
            stable,
            msrv,
            reports,
            out,
        } => {
            let clamped = resolve(&stable, &msrv)?;
            let legs = scenarios::legs(&clamped.kept);
            let reports = summary::read_reports(&reports)?;
            let run = summary::Run {
                release,
                run_id,
                run_attempt,
                run_url,
                tier,
            };
            let key_prefix = summary::key_prefix(&run)?;
            let record = summary::summarize(run, &legs, &reports);

            std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
            let path = out.join("summary.json");
            std::fs::write(&path, serde_json::to_string_pretty(&record)?)
                .with_context(|| format!("writing {}", path.display()))?;

            eprintln!(
                "{} of {} legs red; outcomes {:?}",
                record.red.len(),
                record.legs_total,
                record.outcomes
            );
            for leg in &record.red {
                eprintln!("  {} {} {}", leg.leg_id, leg.verdict, describe(leg));
            }
            if !record.unreported.is_empty() {
                eprintln!(
                    "  WARNING: {} leg(s) sent no report, which is a canary defect: {}",
                    record.unreported.len(),
                    record.unreported.join(", ")
                );
            }
            if !record.had_a_hard_verdict() {
                eprintln!("nothing blamed the release, so there is nothing to archive");
            }
            if record.nothing_ran() {
                eprintln!(
                    "  ALARM: not one of {} legs reported, so nothing checked {} at all. Either \
                     the legs were never dispatched, or they were dispatched and never ran.",
                    record.legs_total, record.run.release
                );
            }

            // The decision, for the steps that emit and upload. Keeping it on stdout as JSON means
            // the workflow never has to parse prose, and the record itself is the file in --out.
            println!(
                "{}",
                serde_json::to_string(&summary::Decision {
                    // From the folded record, which is also what gets archived, so the
                    // decision and the record cannot disagree about which release this is.
                    release: record.run.release.clone(),
                    key_prefix,
                    record: record.had_a_hard_verdict(),
                    unchecked: record.nothing_ran(),
                    summary: path.display().to_string(),
                })?
            );
        }
        Command::GenLeg {
            leg,
            stable,
            msrv,
            out,
        } => {
            let clamped = resolve(&stable, &msrv)?;
            let all = scenarios::legs(&clamped.kept);
            let found = all.iter().find(|l| l.id == leg).ok_or_else(|| {
                anyhow::anyhow!(
                    "no leg {leg:?}. {} legs available; try `gen-matrix` to list them",
                    all.len()
                )
            })?;
            let scenario = scenarios::manifest(&found.manifest).ok_or_else(|| {
                anyhow::anyhow!("leg {leg:?} names unknown manifest {:?}", found.manifest)
            })?;
            let rendered = manifest::render(found, scenario);
            manifest::materialize(&rendered, &out)?;
            eprintln!(
                "leg {} -> {}  (toolchain {} {}, {:?}, checks {:?})",
                found.id,
                out.display(),
                found.toolchain,
                found.toolchain_version,
                found.target.as_deref().unwrap_or("native"),
                found.checks,
            );
            println!("{}", serde_json::to_string(found)?);
        }
    }
    Ok(())
}

/// The one-line version of a red leg for the job log: what broke, and between which versions.
fn describe(leg: &summary::RedLeg) -> String {
    let Some(d) = &leg.diagnosis else {
        return String::new();
    };
    let mut parts: Vec<String> = d.summary.clone().into_iter().collect();
    parts.extend(d.duplicate_versions.iter().map(ToString::to_string));
    parts.join(" | ")
}

fn find_leg<'a>(legs: &'a [scenarios::Leg], id: &str) -> anyhow::Result<&'a scenarios::Leg> {
    legs.iter().find(|l| l.id == id).ok_or_else(|| {
        anyhow::anyhow!(
            "no leg {id:?}. {} legs available; run `gen-matrix` to list them",
            legs.len()
        )
    })
}

/// The newest `release-*` tag on the remote.
///
/// `release-YYYY-MM-DD` sorts lexically into chronological order, so the last line is the current
/// release. `ls-remote` needs no fetch depth and no credential on a public repo, and `--refs` drops
/// the `^{}` dereference lines annotated tags produce.
fn newest_release_tag() -> anyhow::Result<Option<String>> {
    let out = std::process::Command::new("git")
        .args([
            "ls-remote",
            "--tags",
            "--refs",
            "origin",
            "refs/tags/release-*",
        ])
        .output()
        .context("running git ls-remote")?;
    if !out.status.success() {
        // Not fatal: this check is thrift, not correctness. The Tag dimension is what actually
        // guarantees a straggler's emission cannot page.
        eprintln!("could not list remote tags; skipping the supersession check");
        return Ok(None);
    }
    let mut tags: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.rsplit('/').next().map(str::to_string))
        .collect();
    tags.sort();
    Ok(tags.pop())
}

fn resolve(stable: &str, msrv: &str) -> anyhow::Result<toolchain::Clamped> {
    let stable: Version = stable
        .parse()
        .with_context(|| format!("--stable {stable:?} is not a version"))?;
    let msrv: Version = msrv
        .parse()
        .with_context(|| format!("--msrv {msrv:?} is not a version"))?;
    toolchain::resolve(stable, msrv)
}

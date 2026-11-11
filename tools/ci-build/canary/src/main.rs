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
use canary::leg;
use canary::manifest;
use canary::publish;
use canary::scenarios;
use canary::summary;
use canary::toolchain::{self, Version};

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
    /// tick did, and running sixty legs to re-prove that buys nothing. Prints the movement as JSON
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

            // The decision, for the step that does the upload. Keeping it on stdout as JSON means
            // the workflow never has to parse prose, and the record itself is the file in --out.
            println!(
                "{}",
                serde_json::json!({
                    "key_prefix": key_prefix,
                    "record": record.had_a_hard_verdict(),
                    "summary": path,
                })
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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use clap::Parser;
use dialoguer::Confirm;
use semver::Version;
use smithy_rs_tool_common::index::CratesIndex;
use smithy_rs_tool_common::shell::ShellOperation;
use std::fmt;
use std::io::IsTerminal;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::info;

#[derive(Parser, Debug)]
pub struct YankArgs {
    /// Versions to yank, as `name@version`, in the order to yank them.
    #[clap(required = true)]
    crates: Vec<CrateVersion>,
    /// Print the plan and send nothing.
    #[clap(long)]
    dry_run: bool,
    /// Do not ask for confirmation. CodeBuild has no terminal.
    #[clap(short('y'))]
    skip_confirmation: bool,
    /// Unyank instead, to undo a mistake.
    #[clap(long)]
    undo: bool,
}

/// The two crates.io calls this subcommand makes.
#[async_trait]
trait Registry {
    /// Whether the index marks this version yanked. Fails if the index does not list it.
    async fn is_yanked(&self, krate: &CrateVersion) -> Result<bool>;
    /// Sends one yank, or one unyank when `undo` is set.
    async fn yank(&self, krate: &CrateVersion, undo: bool) -> Result<()>;
}

/// crates.io itself: the sparse index that cargo reads, and `cargo yank`.
struct CratesIo {
    index: Arc<CratesIndex>,
}

#[async_trait]
impl Registry for CratesIo {
    async fn is_yanked(&self, krate: &CrateVersion) -> Result<bool> {
        let index = self.index.clone();
        let name = krate.name.clone();
        // The index client makes blocking HTTP calls, so they run on tokio's blocking pool.
        let published =
            tokio::task::spawn_blocking(move || index.published_crate_versions(&name)).await??;
        let version = krate.version.to_string();
        published
            .iter()
            .find(|published| published.version == version)
            .map(|published| published.yanked)
            .ok_or_else(|| anyhow!("{krate} is not in the crates.io index"))
    }

    async fn yank(&self, krate: &CrateVersion, undo: bool) -> Result<()> {
        cargo::Yank::new(&krate.name, krate.version.to_string())
            .undo(undo)
            .spawn()
            .await
    }
}

pub async fn subcommand_yank(args: &YankArgs) -> Result<()> {
    cargo::confirm_installed_on_path()?;
    let crates_io = CratesIo {
        index: Arc::new(CratesIndex::real()?),
    };
    run(&crates_io, args, std::io::stdin().is_terminal()).await
}

async fn run(registry: &impl Registry, args: &YankArgs, interactive: bool) -> Result<()> {
    let want_yanked = !args.undo;
    // Look every version up first, so a typo stops the run before anything is yanked.
    let mut todo = Vec::new();
    for krate in &args.crates {
        if registry.is_yanked(krate).await? != want_yanked {
            todo.push(krate);
        }
    }
    print_plan(&args.crates, &todo, args.undo);
    if args.dry_run || todo.is_empty() {
        return Ok(());
    }
    if !args.skip_confirmation {
        confirm(interactive, args.undo)?;
    }
    for (i, krate) in todo.iter().enumerate() {
        if let Err(err) = yank(registry, krate, args.undo).await {
            bail!(
                "stopped at {krate}: {err:#}\nnot done: {}",
                list(&todo[i..])
            );
        }
    }
    // Cargo reads the sparse index, so finish only when the index shows the change.
    wait_for_index(registry, &todo, want_yanked).await
}

/// Upper bound on the wait for the index to show the change. Index updates can lag
/// (rust-lang/crates.io#11588).
const INDEX_TIMEOUT: Duration = Duration::from_secs(600);
const INDEX_POLL_INTERVAL: Duration = Duration::from_secs(5);

async fn wait_for_index(
    registry: &impl Registry,
    crates: &[&CrateVersion],
    want_yanked: bool,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + INDEX_TIMEOUT;
    let mut waiting = crates.to_vec();
    loop {
        let mut still_waiting = Vec::new();
        for krate in waiting {
            if registry.is_yanked(krate).await? != want_yanked {
                still_waiting.push(krate);
            }
        }
        if still_waiting.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let state = if want_yanked { "yanked" } else { "not yanked" };
            bail!(
                "after {INDEX_TIMEOUT:?} the index still does not show as {state}: {}",
                list(&still_waiting)
            );
        }
        waiting = still_waiting;
        tokio::time::sleep(INDEX_POLL_INTERVAL).await;
    }
}

/// Sends one yank, and waits out crates.io's rate limit as long as it says.
async fn yank(registry: &impl Registry, krate: &CrateVersion, undo: bool) -> Result<()> {
    loop {
        let err = match registry.yank(krate, undo).await {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };
        // A 429 says when crates.io allows the next yank, about one a minute once the burst
        // is used up. Any other error stops the run; a rerun skips versions already yanked.
        let Some(time) = retry_after(&err) else {
            return Err(err);
        };
        // The time is cut to whole seconds, so wait one second more.
        let wait =
            time.duration_since(SystemTime::now()).unwrap_or_default() + Duration::from_secs(1);
        info!("crates.io rate limit; waiting {wait:?} to send {krate}");
        tokio::time::sleep(wait).await;
    }
}

fn print_plan(crates: &[CrateVersion], todo: &[&CrateVersion], undo: bool) {
    let action = if undo { "unyank" } else { "yank" };
    if !todo.is_empty() {
        info!("Will {action}, in this order: {}", list(todo));
    }
    let skipped: Vec<_> = crates
        .iter()
        .filter(|krate| !todo.contains(krate))
        .collect();
    if !skipped.is_empty() {
        info!("Already {action}ed, so skipped: {}", list(&skipped));
    }
}

fn confirm(interactive: bool, undo: bool) -> Result<()> {
    // Without a terminal nobody can answer the prompt, and the run would hang.
    if !interactive {
        bail!("there is no terminal to confirm on; pass -y to go ahead without asking");
    }
    let action = if undo { "Unyank" } else { "Yank" };
    if Confirm::new()
        .with_prompt(format!("{action} these versions on crates.io?"))
        .interact()?
    {
        Ok(())
    } else {
        bail!("aborted")
    }
}

/// Reads the time from "Please try again after <date> and see https://crates.io/docs/rate-limits".
fn retry_after(err: &anyhow::Error) -> Option<SystemTime> {
    let message = format!("{err:#}");
    let (_, rest) = message.split_once("Please try again after ")?;
    let (time, _) = rest.split_once(" and see ")?;
    httpdate::parse_http_date(time).ok()
}

fn list(crates: &[&CrateVersion]) -> String {
    crates
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One crate version, written `name@version`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CrateVersion {
    name: String,
    version: Version,
}

impl FromStr for CrateVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        s.split_once('@')
            .filter(|(name, _)| !name.is_empty())
            .and_then(|(name, version)| {
                Version::parse(version).ok().map(|version| CrateVersion {
                    name: name.into(),
                    version,
                })
            })
            .ok_or_else(|| {
                anyhow!("`{s}` is not a crate version; write it as name@version, such as aws-smithy-types@1.7.0")
            })
    }
}

impl fmt::Display for CrateVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    /// crates.io in memory: the index state of each version, the errors queued for the
    /// next requests about a version, and every request sent. After a yank, the index
    /// shows the old state for `lag` more reads.
    struct FakeRegistry {
        yanked: Mutex<HashMap<String, bool>>,
        errors: Mutex<HashMap<String, VecDeque<String>>>,
        sent: Mutex<Vec<String>>,
        lag: usize,
        pending: Mutex<HashMap<String, (bool, usize)>>,
    }

    impl FakeRegistry {
        fn new(versions: &[(&str, bool)]) -> FakeRegistry {
            FakeRegistry {
                yanked: Mutex::new(versions.iter().map(|(v, y)| (v.to_string(), *y)).collect()),
                errors: Mutex::new(HashMap::new()),
                sent: Mutex::new(Vec::new()),
                lag: 0,
                pending: Mutex::new(HashMap::new()),
            }
        }

        fn with_lag(mut self, reads: usize) -> FakeRegistry {
            self.lag = reads;
            self
        }

        /// The next requests about `krate` fail with these messages, one each.
        fn failing(self, krate: &str, messages: &[&str]) -> FakeRegistry {
            self.errors.lock().unwrap().insert(
                krate.to_string(),
                messages.iter().map(|m| m.to_string()).collect(),
            );
            self
        }

        fn sent(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Registry for FakeRegistry {
        async fn is_yanked(&self, krate: &CrateVersion) -> Result<bool> {
            let key = krate.to_string();
            let mut pending = self.pending.lock().unwrap();
            if let Some((new, reads_left)) = pending.get_mut(&key) {
                if *reads_left > 0 {
                    *reads_left -= 1;
                } else {
                    self.yanked.lock().unwrap().insert(key.clone(), *new);
                    pending.remove(&key);
                }
            }
            self.yanked
                .lock()
                .unwrap()
                .get(&key)
                .copied()
                .ok_or_else(|| anyhow!("{krate} is not in the index"))
        }

        async fn yank(&self, krate: &CrateVersion, undo: bool) -> Result<()> {
            let action = if undo { "unyank" } else { "yank" };
            self.sent.lock().unwrap().push(format!("{action} {krate}"));
            let queued = self
                .errors
                .lock()
                .unwrap()
                .get_mut(&krate.to_string())
                .and_then(VecDeque::pop_front);
            if let Some(message) = queued {
                return Err(anyhow!(message));
            }
            if self.lag == 0 {
                self.yanked.lock().unwrap().insert(krate.to_string(), !undo);
            } else {
                self.pending
                    .lock()
                    .unwrap()
                    .insert(krate.to_string(), (!undo, self.lag));
            }
            Ok(())
        }
    }

    fn args(crates: &[&str]) -> YankArgs {
        YankArgs {
            crates: crates.iter().map(|c| c.parse().unwrap()).collect(),
            dry_run: false,
            skip_confirmation: true,
            undo: false,
        }
    }

    #[tokio::test]
    async fn yanks_in_the_given_order_and_skips_versions_already_yanked() {
        let registry =
            FakeRegistry::new(&[("a@1.0.0", false), ("b@1.0.0", true), ("c@1.0.0", false)]);
        run(&registry, &args(&["c@1.0.0", "b@1.0.0", "a@1.0.0"]), false)
            .await
            .unwrap();
        assert_eq!(vec!["yank c@1.0.0", "yank a@1.0.0"], registry.sent());
    }

    #[tokio::test]
    async fn checks_every_version_before_yanking_any() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let err = run(&registry, &args(&["a@1.0.0", "a@1.0.1"]), false)
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("a@1.0.1"), "{err:#}");
        assert!(registry.sent().is_empty());
    }

    #[tokio::test]
    async fn stops_at_the_first_error_and_names_what_is_not_done() {
        let registry =
            FakeRegistry::new(&[("a@1.0.0", false), ("b@1.0.0", false), ("c@1.0.0", false)])
                .failing("b@1.0.0", &["boom"]);
        let err = run(&registry, &args(&["a@1.0.0", "b@1.0.0", "c@1.0.0"]), false)
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("stopped at b@1.0.0: boom"), "{err}");
        assert!(err.contains("not done: b@1.0.0, c@1.0.0"), "{err}");
        assert_eq!(vec!["yank a@1.0.0", "yank b@1.0.0"], registry.sent());
    }

    #[tokio::test]
    async fn does_nothing_and_asks_nothing_when_every_version_is_done() {
        let registry = FakeRegistry::new(&[("a@1.0.0", true)]);
        let mut args = args(&["a@1.0.0"]);
        args.skip_confirmation = false;
        run(&registry, &args, false).await.unwrap();
        assert!(registry.sent().is_empty());
    }

    #[tokio::test]
    async fn dry_run_sends_nothing() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let mut args = args(&["a@1.0.0"]);
        args.dry_run = true;
        run(&registry, &args, false).await.unwrap();
        assert!(registry.sent().is_empty());
    }

    #[tokio::test]
    async fn without_y_and_without_a_terminal_refuses_instead_of_waiting() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let mut args = args(&["a@1.0.0"]);
        args.skip_confirmation = false;
        let err = run(&registry, &args, false).await.unwrap_err();
        assert!(err.to_string().contains("-y"), "{err}");
        assert!(registry.sent().is_empty());
    }

    #[tokio::test]
    async fn undo_unyanks_only_versions_that_are_yanked() {
        let registry = FakeRegistry::new(&[("a@1.0.0", true), ("b@1.0.0", false)]);
        let mut args = args(&["a@1.0.0", "b@1.0.0"]);
        args.undo = true;
        run(&registry, &args, false).await.unwrap();
        assert_eq!(vec!["unyank a@1.0.0"], registry.sent());
    }

    #[tokio::test]
    async fn crates_io_reads_the_yanked_flag_from_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(
            &path,
            r#"
            [crates]
            aws-smithy-types = ["1.6.3", "1.7.0"]

            [crate_version_metadata."aws-smithy-types@1.7.0"]
            yanked = true
            "#,
        )
        .unwrap();
        let crates_io = CratesIo {
            index: Arc::new(CratesIndex::fake(&path)),
        };
        let version = |v: &str| v.parse::<CrateVersion>().unwrap();
        assert!(crates_io
            .is_yanked(&version("aws-smithy-types@1.7.0"))
            .await
            .unwrap());
        assert!(!crates_io
            .is_yanked(&version("aws-smithy-types@1.6.3"))
            .await
            .unwrap());
        for missing in ["aws-smithy-types@1.7.1", "aws-smithy-typos@1.7.0"] {
            let err = crates_io.is_yanked(&version(missing)).await.unwrap_err();
            assert!(err.to_string().contains(missing), "{err}");
        }
    }

    #[test]
    fn parses_the_command_line() {
        let args =
            YankArgs::try_parse_from(["yank", "-y", "--undo", "a@1.0.0", "b@2.0.0"]).unwrap();
        assert!(args.skip_confirmation && args.undo && !args.dry_run);
        assert_eq!(
            vec!["a@1.0.0", "b@2.0.0"],
            args.crates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert!(
            YankArgs::try_parse_from(["yank", "--dry-run", "a@1.0.0"])
                .unwrap()
                .dry_run
        );
        // A version list is required, and every entry must be name@version.
        assert!(YankArgs::try_parse_from(["yank", "-y"]).is_err());
        assert!(YankArgs::try_parse_from(["yank", "-y", "aws-smithy-types"]).is_err());
    }

    /// What `cargo yank` prints for a 429 from crates.io (real cargo against a stub that
    /// copies crates.io's response), as `capture_error` wraps it.
    fn rate_limited(time: &str) -> String {
        format!(
            "Failed to cargo yank:\nStatus: 101\nStdout: \n\nStderr:     Updating crates.io index\n\
             error: failed to yank from the registry at https://crates.io\n\n\
             Caused by:\n  the remote server responded with an error (status 429 Too Many Requests): \
             You have yanked or unyanked too many versions in a short period of time. \
             Please try again after {time} and see https://crates.io/docs/rate-limits for more details.\n"
        )
    }

    #[tokio::test(start_paused = true)]
    async fn waits_out_a_rate_limit_then_retries_the_same_version() {
        let in_30s = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(30));
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("b@1.0.0", false)])
            .failing("a@1.0.0", &[&rate_limited(&in_30s)]);
        let start = tokio::time::Instant::now();
        run(&registry, &args(&["a@1.0.0", "b@1.0.0"]), false)
            .await
            .unwrap();
        assert_eq!(
            vec!["yank a@1.0.0", "yank a@1.0.0", "yank b@1.0.0"],
            registry.sent()
        );
        // The time is whole seconds, plus the one second added on top.
        let waited = start.elapsed();
        assert!(
            waited >= Duration::from_secs(30) && waited <= Duration::from_secs(32),
            "{waited:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn waits_until_the_index_shows_the_change() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]).with_lag(3);
        let start = tokio::time::Instant::now();
        run(&registry, &args(&["a@1.0.0"]), false).await.unwrap();
        // Three reads still show the old state, one poll interval apart.
        assert_eq!(3 * INDEX_POLL_INTERVAL, start.elapsed());
        assert_eq!(vec!["yank a@1.0.0"], registry.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn fails_when_the_index_never_shows_the_change() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]).with_lag(usize::MAX);
        let start = tokio::time::Instant::now();
        let err = run(&registry, &args(&["a@1.0.0"]), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("a@1.0.0"), "{err}");
        assert!(start.elapsed() >= INDEX_TIMEOUT);
    }

    #[test]
    fn retry_after_reads_the_time_from_a_rate_limit_error() {
        let err = anyhow!(rate_limited("Sat, 21 Nov 2026 01:16:28 GMT"));
        assert_eq!(
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_223_788)),
            retry_after(&err)
        );
    }

    #[test]
    fn retry_after_is_none_for_other_errors() {
        assert_eq!(None, retry_after(&anyhow!("boom")));
        assert_eq!(None, retry_after(&anyhow!(rate_limited("tomorrow"))));
    }

    #[test]
    fn crate_version_parses_name_at_version() {
        let krate: CrateVersion = "aws-smithy-types@1.7.0".parse().unwrap();
        assert_eq!("aws-smithy-types", krate.name);
        assert_eq!(Version::new(1, 7, 0), krate.version);
        assert_eq!("aws-smithy-types@1.7.0", krate.to_string());
    }

    #[test]
    fn crate_version_rejects_a_missing_name_or_version() {
        for bad in [
            "aws-smithy-types",
            "@1.7.0",
            "aws-smithy-types@",
            "aws-smithy-types@1.7",
        ] {
            let err = bad.parse::<CrateVersion>().unwrap_err().to_string();
            assert!(err.contains("name@version"), "{bad}: {err}");
        }
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use clap::Parser;
use dialoguer::Confirm;
use semver::{Version, VersionReq};
use serde::Deserialize;
use smithy_rs_tool_common::index::CratesIndex;
use smithy_rs_tool_common::shell::ShellOperation;
use std::collections::HashMap;
use std::fmt;
use std::io::IsTerminal;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

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
    /// Yank from this registry in cargo's config instead of crates.io, such as a local test
    /// registry. Its sparse index URL is read from `CARGO_REGISTRIES_<NAME>_INDEX`, as cargo does.
    #[clap(long)]
    registry: Option<String>,
}

/// The crates.io calls this subcommand makes.
#[async_trait]
trait Registry {
    /// Whether the index marks this version yanked. Fails if the index does not list it.
    async fn is_yanked(&self, krate: &CrateVersion) -> Result<bool>;
    /// Sends one yank, or one unyank when `undo` is set.
    async fn yank(&self, krate: &CrateVersion, undo: bool) -> Result<()>;
    /// Every version of `name` in the index, and whether it is yanked.
    async fn versions(&self, name: &str) -> Result<Vec<(Version, bool)>>;
    /// The newest version of each crate that depends on `name`, other than as a dev
    /// dependency, with what it requires of `name`.
    async fn dependents(&self, name: &str) -> Result<Vec<Dependent>>;
}

const CRATES_IO_INDEX: &str = "sparse+https://index.crates.io/";
/// crates.io asks API clients to say who they are.
const USER_AGENT: &str = "smithy-rs publisher (https://github.com/smithy-lang/smithy-rs)";
/// The most reverse dependencies crates.io returns in one page.
const PER_PAGE: usize = 100;
/// How long one API request may take, from connecting until its body is read. A lookup that
/// takes longer becomes a warning, and the yank goes ahead.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

fn http_client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
}

/// crates.io itself, or the registry named by `--registry`: the sparse index that cargo reads,
/// the registry's API, and `cargo yank`.
struct CratesIo {
    index: Arc<CratesIndex>,
    index_url: String,
    http: reqwest::Client,
    registry: Option<String>,
}

impl CratesIo {
    /// The registry's API URL, from its index's config.json, where cargo reads it.
    async fn api_url(&self) -> Result<String> {
        let index = self
            .index_url
            .trim_start_matches("sparse+")
            .trim_end_matches('/');
        let config = self
            .http
            .get(format!("{index}/config.json"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        api_from_config(&config)
    }
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
            .registry(self.registry.clone())
            .spawn()
            .await
    }

    async fn versions(&self, name: &str) -> Result<Vec<(Version, bool)>> {
        let index = self.index.clone();
        let name = name.to_string();
        let published =
            tokio::task::spawn_blocking(move || index.published_crate_versions(&name)).await??;
        Ok(published
            .into_iter()
            .filter_map(|published| {
                Some((Version::parse(&published.version).ok()?, published.yanked))
            })
            .collect())
    }

    async fn dependents(&self, name: &str) -> Result<Vec<Dependent>> {
        let api = self.api_url().await?;
        let http = &self.http;
        read_pages(|page| {
            let url = format!(
                "{api}/api/v1/crates/{name}/reverse_dependencies?per_page={PER_PAGE}&page={page}"
            );
            async move {
                Ok(http
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await?)
            }
        })
        .await
    }
}

/// Reads `reverse_dependencies` pages, numbered from 1, until it has read as many entries as
/// crates.io counts, or a page comes back empty.
async fn read_pages<F>(mut fetch: impl FnMut(usize) -> F) -> Result<Vec<Dependent>>
where
    F: std::future::Future<Output = Result<String>>,
{
    let mut dependents = Vec::new();
    let mut read = 0;
    for number in 1.. {
        if number > 1 {
            // crates.io allows API clients one request a second.
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let page = parse_reverse_dependencies(&fetch(number).await?)?;
        read += page.entries;
        dependents.extend(page.dependents);
        if page.entries == 0 || read >= page.total {
            break;
        }
    }
    Ok(dependents)
}

pub async fn subcommand_yank(args: &YankArgs) -> Result<()> {
    cargo::confirm_installed_on_path()?;
    let (index, index_url) = match &args.registry {
        Some(name) => {
            let url = registry_index_url(name)?;
            (CratesIndex::sparse(&url)?, url)
        }
        None => (CratesIndex::real()?, CRATES_IO_INDEX.to_string()),
    };
    let crates_io = CratesIo {
        index: Arc::new(index),
        index_url,
        http: http_client(HTTP_TIMEOUT)?,
        registry: args.registry.clone(),
    };
    run(&crates_io, args, std::io::stdin().is_terminal()).await
}

/// The `api` URL from an index's config.json.
fn api_from_config(config: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Config {
        api: Option<String>,
    }
    let config: Config = serde_json::from_str(config)?;
    config
        .api
        .map(|api| api.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow!("the index config.json names no API"))
}

/// The sparse index URL of a registry in cargo's config, from the variable cargo reads it from,
/// so the lookups and `cargo yank` go to the same registry.
fn registry_index_url(name: &str) -> Result<String> {
    let var = index_url_var(name);
    std::env::var(&var)
        .map_err(|_| anyhow!("--registry {name} needs {var}, the registry's sparse index URL"))
}

fn index_url_var(name: &str) -> String {
    format!(
        "CARGO_REGISTRIES_{}_INDEX",
        name.to_uppercase().replace('-', "_")
    )
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
    // An unyank strands nothing.
    if !args.undo {
        for warning in stranded_warnings(registry, &todo).await {
            warn!("{warning}");
        }
    }
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

/// The newest version of a crate that depends on another, and what it requires of it.
#[derive(Clone, Debug)]
struct Dependent {
    krate: CrateVersion,
    requirement: VersionReq,
}

/// The dependents that no version of their dependency fits once `yanking` is yanked, given
/// every version of the dependency and whether it is yanked already.
fn stranded<'a>(
    dependents: &'a [Dependent],
    versions: &[(Version, bool)],
    yanking: &[&Version],
) -> Vec<&'a Dependent> {
    let left: Vec<_> = versions
        .iter()
        .filter(|(version, yanked)| !yanked && !yanking.contains(&version))
        .map(|(version, _)| version)
        .collect();
    dependents
        .iter()
        .filter(|dependent| !left.iter().any(|v| dependent.requirement.matches(v)))
        .collect()
}

/// One warning per crate in the yank that strands others: crates whose newest version only
/// fits the versions being yanked, which cargo then cannot pick. This tells whether yanking
/// one crate breaks hundreds of others until a fix ships. A failed lookup is a warning too,
/// and never stops the yank.
async fn stranded_warnings(registry: &impl Registry, todo: &[&CrateVersion]) -> Vec<String> {
    let mut names: Vec<&str> = Vec::new();
    for krate in todo {
        if !names.contains(&krate.name.as_str()) {
            names.push(&krate.name);
        }
    }
    let mut warnings = Vec::new();
    for name in names {
        let yanking: Vec<_> = todo
            .iter()
            .filter(|krate| krate.name == name)
            .map(|krate| &krate.version)
            .collect();
        let lookup = async {
            let versions = registry.versions(name).await?;
            let dependents = registry.dependents(name).await?;
            anyhow::Ok(
                stranded(&dependents, &versions, &yanking)
                    .into_iter()
                    // Those being yanked as well are taken care of.
                    .filter(|dependent| !todo.contains(&&dependent.krate))
                    .map(|dependent| dependent.krate.to_string())
                    .collect::<Vec<_>>(),
            )
        };
        match lookup.await {
            Ok(stranded) if stranded.is_empty() => {}
            Ok(stranded) => warnings.push(stranded_message(name, &stranded)),
            Err(err) => warnings.push(format!(
                "Could not check which crates need the {name} versions being yanked: {err:#}"
            )),
        }
    }
    warnings
}

fn stranded_message(name: &str, stranded: &[String]) -> String {
    const SHOWN: usize = 5;
    let crates = if stranded.len() == 1 {
        "crate"
    } else {
        "crates"
    };
    let mut listed = stranded[..stranded.len().min(SHOWN)].join(", ");
    if stranded.len() > SHOWN {
        listed += &format!(" and {} more", stranded.len() - SHOWN);
    }
    format!(
        "After this yank, no {name} version fits the newest version of {} {crates}: {listed}. \
         Until a fixed {name} ships, cargo falls back to their older versions.",
        stranded.len()
    )
}

/// One page of crates.io's `reverse_dependencies` response.
struct ReverseDependencies {
    /// The page's dependents, dev dependencies left out.
    dependents: Vec<Dependent>,
    /// How many entries the page held, dev dependencies included.
    entries: usize,
    /// How many entries all the pages hold together.
    total: usize,
}

fn parse_reverse_dependencies(body: &str) -> Result<ReverseDependencies> {
    #[derive(Deserialize)]
    struct Page {
        dependencies: Vec<Dependency>,
        versions: Vec<DependentVersion>,
        meta: Meta,
    }
    #[derive(Deserialize)]
    struct Meta {
        total: usize,
    }
    #[derive(Deserialize)]
    struct Dependency {
        version_id: u64,
        req: String,
        kind: String,
    }
    #[derive(Deserialize)]
    struct DependentVersion {
        id: u64,
        #[serde(rename = "crate")]
        name: String,
        num: String,
    }
    let page: Page = serde_json::from_str(body)?;
    let versions: HashMap<_, _> = page.versions.iter().map(|v| (v.id, v)).collect();
    let dependents = page
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind != "dev")
        .map(|dependency| {
            let version = versions
                .get(&dependency.version_id)
                .ok_or_else(|| anyhow!("no version {} in the page", dependency.version_id))?;
            Ok(Dependent {
                krate: CrateVersion {
                    name: version.name.clone(),
                    version: Version::parse(&version.num)?,
                },
                requirement: VersionReq::parse(&dependency.req)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(ReverseDependencies {
        dependents,
        entries: page.dependencies.len(),
        total: page.meta.total,
    })
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
    /// next requests about a version, every request sent, and the dependents of each crate.
    /// After a yank, the index shows the old state for `lag` more reads.
    struct FakeRegistry {
        yanked: Mutex<HashMap<String, bool>>,
        errors: Mutex<HashMap<String, VecDeque<String>>>,
        sent: Mutex<Vec<String>>,
        lag: usize,
        pending: Mutex<HashMap<String, (bool, usize)>>,
        index_down: bool,
        dependents: HashMap<String, Vec<Dependent>>,
        dependents_error: Option<String>,
        dependents_lookups: Mutex<usize>,
    }

    impl FakeRegistry {
        fn new(versions: &[(&str, bool)]) -> FakeRegistry {
            FakeRegistry {
                yanked: Mutex::new(versions.iter().map(|(v, y)| (v.to_string(), *y)).collect()),
                errors: Mutex::new(HashMap::new()),
                sent: Mutex::new(Vec::new()),
                lag: 0,
                pending: Mutex::new(HashMap::new()),
                index_down: false,
                dependents: HashMap::new(),
                dependents_error: None,
                dependents_lookups: Mutex::new(0),
            }
        }

        /// The newest versions that depend on `name`, as `(name@version, requirement)`.
        fn with_dependents(mut self, name: &str, dependents: &[(&str, &str)]) -> FakeRegistry {
            self.dependents.insert(
                name.to_string(),
                dependents
                    .iter()
                    .map(|(krate, requirement)| Dependent {
                        krate: krate.parse().unwrap(),
                        requirement: requirement.parse().unwrap(),
                    })
                    .collect(),
            );
            self
        }

        /// Every dependents lookup fails with this message.
        fn dependents_failing(mut self, message: &str) -> FakeRegistry {
            self.dependents_error = Some(message.to_string());
            self
        }

        fn dependents_lookups(&self) -> usize {
            *self.dependents_lookups.lock().unwrap()
        }

        fn with_lag(mut self, reads: usize) -> FakeRegistry {
            self.lag = reads;
            self
        }

        /// Once a yank is sent, every index read fails.
        fn index_down_after_yank(mut self) -> FakeRegistry {
            self.index_down = true;
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

        /// Whether the index shows `krate` yanked now, without counting as a read.
        fn shown(&self, krate: &str) -> bool {
            self.yanked.lock().unwrap()[krate]
        }
    }

    #[async_trait]
    impl Registry for FakeRegistry {
        async fn is_yanked(&self, krate: &CrateVersion) -> Result<bool> {
            if self.index_down && !self.sent.lock().unwrap().is_empty() {
                bail!("index unavailable");
            }
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

        async fn versions(&self, name: &str) -> Result<Vec<(Version, bool)>> {
            Ok(self
                .yanked
                .lock()
                .unwrap()
                .iter()
                .map(|(krate, yanked)| (krate.parse::<CrateVersion>().unwrap(), *yanked))
                .filter(|(krate, _)| krate.name == name)
                .map(|(krate, yanked)| (krate.version, yanked))
                .collect())
        }

        async fn dependents(&self, name: &str) -> Result<Vec<Dependent>> {
            *self.dependents_lookups.lock().unwrap() += 1;
            if let Some(message) = &self.dependents_error {
                bail!("{message}");
            }
            Ok(self.dependents.get(name).cloned().unwrap_or_default())
        }
    }

    fn args(crates: &[&str]) -> YankArgs {
        YankArgs {
            crates: crates.iter().map(|c| c.parse().unwrap()).collect(),
            dry_run: false,
            skip_confirmation: true,
            undo: false,
            registry: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn yanks_in_the_given_order_and_skips_versions_already_yanked() {
        let registry =
            FakeRegistry::new(&[("a@1.0.0", false), ("b@1.0.0", true), ("c@1.0.0", false)]);
        run(&registry, &args(&["c@1.0.0", "b@1.0.0", "a@1.0.0"]), false)
            .await
            .unwrap();
        assert_eq!(vec!["yank c@1.0.0", "yank a@1.0.0"], registry.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn checks_every_version_before_yanking_any() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let err = run(&registry, &args(&["a@1.0.0", "a@1.0.1"]), false)
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("a@1.0.1"), "{err:#}");
        assert!(registry.sent().is_empty());
    }

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
    async fn does_nothing_and_asks_nothing_when_every_version_is_done() {
        let registry = FakeRegistry::new(&[("a@1.0.0", true)]);
        let mut args = args(&["a@1.0.0"]);
        args.skip_confirmation = false;
        run(&registry, &args, false).await.unwrap();
        assert!(registry.sent().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn dry_run_sends_nothing() {
        for undo in [false, true] {
            // The version starts in the other state, so there is something to do.
            let registry = FakeRegistry::new(&[("a@1.0.0", undo)]);
            let mut args = args(&["a@1.0.0"]);
            args.dry_run = true;
            args.undo = undo;
            run(&registry, &args, false).await.unwrap();
            assert!(registry.sent().is_empty(), "undo: {undo}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn yanks_every_listed_version_of_one_crate() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("a@1.0.1", false)]);
        run(&registry, &args(&["a@1.0.0", "a@1.0.1"]), false)
            .await
            .unwrap();
        assert_eq!(vec!["yank a@1.0.0", "yank a@1.0.1"], registry.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn fails_when_the_index_cannot_be_read_after_the_yank() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]).index_down_after_yank();
        let err = run(&registry, &args(&["a@1.0.0"]), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("index unavailable"), "{err}");
        assert_eq!(vec!["yank a@1.0.0"], registry.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn without_y_and_without_a_terminal_refuses_instead_of_waiting() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let mut args = args(&["a@1.0.0"]);
        args.skip_confirmation = false;
        let err = run(&registry, &args, false).await.unwrap_err();
        assert!(err.to_string().contains("-y"), "{err}");
        assert!(registry.sent().is_empty());
        // What the yank strands is shown before the prompt asks.
        assert_eq!(1, registry.dependents_lookups());
    }

    #[tokio::test(start_paused = true)]
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
            index_url: CRATES_IO_INDEX.into(),
            http: reqwest::Client::new(),
            registry: None,
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
        assert_eq!(
            vec![
                (Version::new(1, 6, 3), false),
                (Version::new(1, 7, 0), true)
            ],
            crates_io.versions("aws-smithy-types").await.unwrap()
        );
    }

    #[tokio::test]
    async fn an_api_request_that_gets_no_answer_times_out() {
        // A server that accepts connections and never answers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(&path, "[crates]\n").unwrap();
        let crates_io = CratesIo {
            index: Arc::new(CratesIndex::fake(&path)),
            index_url: format!("sparse+http://{addr}/"),
            http: http_client(Duration::from_millis(200)).unwrap(),
            registry: None,
        };
        let err = tokio::time::timeout(Duration::from_secs(10), crates_io.dependents("a"))
            .await
            .expect("the request times out on its own")
            .unwrap_err();
        assert!(
            err.downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_timeout),
            "{err:#}"
        );
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
        assert_eq!(
            Some("mock"),
            YankArgs::try_parse_from(["yank", "--registry", "mock", "a@1.0.0"])
                .unwrap()
                .registry
                .as_deref()
        );
        assert_eq!(
            None,
            YankArgs::try_parse_from(["yank", "a@1.0.0"])
                .unwrap()
                .registry
        );
        // A version list is required, and every entry must be name@version.
        assert!(YankArgs::try_parse_from(["yank", "-y"]).is_err());
        assert!(YankArgs::try_parse_from(["yank", "-y", "aws-smithy-types"]).is_err());
    }

    #[test]
    fn registry_index_comes_from_the_variable_cargo_reads() {
        assert_eq!("CARGO_REGISTRIES_MY_MOCK_INDEX", index_url_var("my-mock"));
        let err = registry_index_url("publisher-yank-test-unset")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("CARGO_REGISTRIES_PUBLISHER_YANK_TEST_UNSET_INDEX"),
            "{err}"
        );
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
    async fn retries_after_a_second_when_the_rate_limit_time_has_passed() {
        let a_minute_ago = httpdate::fmt_http_date(SystemTime::now() - Duration::from_secs(60));
        let registry = FakeRegistry::new(&[("a@1.0.0", false)])
            .failing("a@1.0.0", &[&rate_limited(&a_minute_ago)]);
        let start = tokio::time::Instant::now();
        run(&registry, &args(&["a@1.0.0"]), false).await.unwrap();
        assert_eq!(vec!["yank a@1.0.0", "yank a@1.0.0"], registry.sent());
        assert_eq!(Duration::from_secs(1), start.elapsed());
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
    async fn waits_until_the_index_shows_every_version() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("b@1.0.0", false)]).with_lag(3);
        run(&registry, &args(&["a@1.0.0", "b@1.0.0"]), false)
            .await
            .unwrap();
        assert!(registry.shown("a@1.0.0") && registry.shown("b@1.0.0"));
    }

    #[tokio::test(start_paused = true)]
    async fn undo_waits_until_the_index_shows_the_change() {
        let registry = FakeRegistry::new(&[("a@1.0.0", true)]).with_lag(3);
        let mut args = args(&["a@1.0.0"]);
        args.undo = true;
        let start = tokio::time::Instant::now();
        run(&registry, &args, false).await.unwrap();
        assert_eq!(3 * INDEX_POLL_INTERVAL, start.elapsed());
        assert!(!registry.shown("a@1.0.0"));
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
    fn stranded_keeps_the_dependents_no_version_left_fits() {
        let version = |v: &str| Version::parse(v).unwrap();
        // (requirement, every version and whether it is yanked already, versions being yanked,
        // whether the dependent is stranded)
        let cases = [
            (
                "^1.16.4",
                vec![("1.16.3", false), ("1.16.4", false)],
                vec!["1.16.4"],
                true,
            ),
            (
                "^1.16.3",
                vec![("1.16.3", false), ("1.16.4", false)],
                vec!["1.16.4"],
                false,
            ),
            (
                "^1.16.4",
                vec![("1.16.4", false), ("1.16.5", false)],
                vec!["1.16.4"],
                false,
            ),
            (
                "^1.16.4",
                vec![("1.16.4", false), ("1.16.5", true)],
                vec!["1.16.4"],
                true,
            ),
            (
                "^1.16.3",
                vec![("1.16.3", false), ("1.16.4", false)],
                vec!["1.16.3", "1.16.4"],
                true,
            ),
            (
                "=1.16.3",
                vec![("1.16.3", false), ("1.16.4", false)],
                vec!["1.16.4"],
                false,
            ),
            // An older version left fits, though the newest left does not.
            (
                "^1.16.0",
                vec![("1.16.3", false), ("1.16.4", false), ("2.0.0", false)],
                vec!["1.16.4"],
                false,
            ),
            // A pre-release left does not count: cargo would not pick it.
            (
                "^1.16.4",
                vec![("1.16.4", false), ("1.17.0-rc.1", false)],
                vec!["1.16.4"],
                true,
            ),
        ];
        for (requirement, versions, yanking, want) in cases {
            let dependents = [Dependent {
                krate: "d@1.0.0".parse().unwrap(),
                requirement: requirement.parse().unwrap(),
            }];
            let versions: Vec<_> = versions.iter().map(|(v, y)| (version(v), *y)).collect();
            let yanking: Vec<_> = yanking.iter().map(|v| version(v)).collect();
            let yanking: Vec<_> = yanking.iter().collect();
            let got = !stranded(&dependents, &versions, &yanking).is_empty();
            assert_eq!(
                want, got,
                "{requirement}, {versions:?}, yanking {yanking:?}"
            );
        }
    }

    #[tokio::test]
    async fn warns_about_the_crates_a_yank_strands() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("a@1.1.0", false)])
            .with_dependents("a", &[("x@2.0.0", "^1.1.0"), ("y@3.0.0", "^1.0.0")]);
        let krate = "a@1.1.0".parse().unwrap();
        assert_eq!(
            vec![
                "After this yank, no a version fits the newest version of 1 crate: x@2.0.0. \
                 Until a fixed a ships, cargo falls back to their older versions."
            ],
            stranded_warnings(&registry, &[&krate]).await
        );
    }

    #[tokio::test]
    async fn names_five_stranded_crates_and_counts_the_rest() {
        let names: Vec<_> = (1..=7).map(|i| format!("d{i}@1.0.0")).collect();
        let dependents: Vec<_> = names.iter().map(|d| (d.as_str(), "^1.1.0")).collect();
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("a@1.1.0", false)])
            .with_dependents("a", &dependents);
        let krate = "a@1.1.0".parse().unwrap();
        let warnings = stranded_warnings(&registry, &[&krate]).await;
        assert!(
            warnings[0].contains(
                "of 7 crates: d1@1.0.0, d2@1.0.0, d3@1.0.0, d4@1.0.0, d5@1.0.0 and 2 more."
            ),
            "{warnings:?}"
        );
    }

    #[tokio::test]
    async fn looks_up_each_crate_once_with_all_its_versions_being_yanked() {
        let registry =
            FakeRegistry::new(&[("a@1.0.0", false), ("a@1.0.1", false), ("b@1.0.0", false)])
                .with_dependents("a", &[("x@2.0.0", "^1.0.0")]);
        let todo: Vec<CrateVersion> = ["a@1.0.0", "b@1.0.0", "a@1.0.1"]
            .iter()
            .map(|k| k.parse().unwrap())
            .collect();
        let warnings = stranded_warnings(&registry, &todo.iter().collect::<Vec<_>>()).await;
        assert_eq!(2, registry.dependents_lookups());
        assert!(warnings[0].contains(": x@2.0.0."), "{warnings:?}");
    }

    #[tokio::test]
    async fn leaves_out_stranded_crates_that_are_being_yanked_too() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false), ("a@1.1.0", false)])
            .with_dependents("a", &[("x@2.0.0", "^1.1.0")]);
        let (a, x) = ("a@1.1.0".parse().unwrap(), "x@2.0.0".parse().unwrap());
        assert!(stranded_warnings(&registry, &[&a, &x]).await.is_empty());
        // An older x being yanked does not take care of x's newest version.
        let old_x = "x@1.0.0".parse().unwrap();
        let warnings = stranded_warnings(&registry, &[&a, &old_x]).await;
        assert!(warnings[0].contains(": x@2.0.0."), "{warnings:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_dependents_lookup_is_a_warning_and_the_yank_goes_ahead() {
        let registry =
            FakeRegistry::new(&[("a@1.0.0", false)]).dependents_failing("crates.io is down");
        let krate = "a@1.0.0".parse().unwrap();
        let warnings = stranded_warnings(&registry, &[&krate]).await;
        assert!(warnings[0].contains("crates.io is down"), "{warnings:?}");
        run(&registry, &args(&["a@1.0.0"]), false).await.unwrap();
        assert_eq!(vec!["yank a@1.0.0"], registry.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn looks_for_stranded_crates_in_a_dry_run_but_not_for_an_unyank() {
        let registry = FakeRegistry::new(&[("a@1.0.0", false)]);
        let mut dry_run = args(&["a@1.0.0"]);
        dry_run.dry_run = true;
        run(&registry, &dry_run, false).await.unwrap();
        assert_eq!(1, registry.dependents_lookups());

        for dry_run in [false, true] {
            let registry = FakeRegistry::new(&[("a@1.0.0", true)]);
            let mut undo = args(&["a@1.0.0"]);
            undo.undo = true;
            undo.dry_run = dry_run;
            run(&registry, &undo, false).await.unwrap();
            assert_eq!(0, registry.dependents_lookups(), "dry run: {dry_run}");
        }
    }

    #[test]
    fn reads_dependents_from_a_crates_io_reverse_dependencies_page() {
        // A page as crates.io sends it, cut to four entries and the fields this reads.
        let page = r#"{
            "dependencies": [
                {"version_id": 10, "req": "^1.16.4", "kind": "normal", "optional": false},
                {"version_id": 11, "req": "^1.16.1", "kind": "normal", "optional": true},
                {"version_id": 12, "req": "^1.0", "kind": "dev", "optional": false},
                {"version_id": 13, "req": "^1.16.0", "kind": "build", "optional": false}
            ],
            "versions": [
                {"id": 11, "crate": "aws-smithy-http-client", "num": "1.1.5"},
                {"id": 10, "crate": "aws-config", "num": "1.13.4"},
                {"id": 12, "crate": "some-tests", "num": "0.1.0"},
                {"id": 13, "crate": "some-build", "num": "0.2.0"}
            ],
            "meta": {"total": 4}
        }"#;
        let parsed = parse_reverse_dependencies(page).unwrap();
        assert_eq!((4, 4), (parsed.entries, parsed.total));
        assert_eq!(
            vec![
                ("aws-config@1.13.4".to_string(), "^1.16.4".to_string()),
                (
                    "aws-smithy-http-client@1.1.5".to_string(),
                    "^1.16.1".to_string()
                ),
                ("some-build@0.2.0".to_string(), "^1.16.0".to_string()),
            ],
            parsed
                .dependents
                .iter()
                .map(|d| (d.krate.to_string(), d.requirement.to_string()))
                .collect::<Vec<_>>()
        );
        // A dependency whose version the page leaves out fails the lookup.
        let missing = r#"{
            "dependencies": [{"version_id": 13, "req": "^1.0.0", "kind": "normal"}],
            "versions": [],
            "meta": {"total": 1}
        }"#;
        let err = parse_reverse_dependencies(missing).err().unwrap();
        assert!(err.to_string().contains("no version 13"), "{err}");
    }

    /// A `reverse_dependencies` page holding `entries` dependents, numbered from `first`, of
    /// `total` in all.
    fn page_of(first: usize, entries: usize, total: usize) -> String {
        let ids = first..first + entries;
        let dependencies: Vec<_> = ids
            .clone()
            .map(|id| format!(r#"{{"version_id":{id},"req":"^1.0.0","kind":"normal"}}"#))
            .collect();
        let versions: Vec<_> = ids
            .map(|id| format!(r#"{{"id":{id},"crate":"d{id}","num":"1.0.0"}}"#))
            .collect();
        format!(
            r#"{{"dependencies":[{}],"versions":[{}],"meta":{{"total":{total}}}}}"#,
            dependencies.join(","),
            versions.join(",")
        )
    }

    #[tokio::test(start_paused = true)]
    async fn reads_every_page_crates_io_counts_a_second_apart() {
        // (entries on each page, total crates.io counts, dependents read)
        let cases = [
            (vec![100, 100, 3], 203, 203),
            // No request for a page past the total.
            (vec![100, 100], 200, 200),
            // Pages smaller than asked for.
            (vec![2, 2, 1], 5, 5),
            // An empty page ends it, whatever the total says.
            (vec![100, 0], 150, 100),
        ];
        for (pages, total, want) in cases {
            let mut first = 0;
            let bodies: Vec<_> = pages
                .iter()
                .map(|&entries| {
                    let body = page_of(first, entries, total);
                    first += entries;
                    body
                })
                .collect();
            let asked = Mutex::new(Vec::new());
            let start = tokio::time::Instant::now();
            let dependents = read_pages(|number| {
                asked.lock().unwrap().push(number);
                let body = bodies[number - 1].clone();
                async move { Ok(body) }
            })
            .await
            .unwrap();
            assert_eq!(want, dependents.len(), "{pages:?}");
            let asked = asked.into_inner().unwrap();
            assert_eq!((1..=pages.len()).collect::<Vec<_>>(), asked, "{pages:?}");
            let pauses = pages.len() as u64 - 1;
            assert_eq!(Duration::from_secs(pauses), start.elapsed(), "{pages:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn counts_dev_dependencies_toward_the_total() {
        // One page holds both entries crates.io counts, one of them a dev dependency.
        let page = r#"{
            "dependencies": [
                {"version_id": 1, "req": "^1.0.0", "kind": "dev"},
                {"version_id": 2, "req": "^1.0.0", "kind": "normal"}
            ],
            "versions": [
                {"id": 1, "crate": "t", "num": "1.0.0"},
                {"id": 2, "crate": "d", "num": "1.0.0"}
            ],
            "meta": {"total": 2}
        }"#;
        let asked = Mutex::new(0);
        let dependents = read_pages(|_| {
            *asked.lock().unwrap() += 1;
            async move { Ok(page.to_string()) }
        })
        .await
        .unwrap();
        assert_eq!(1, dependents.len());
        assert_eq!(1, *asked.lock().unwrap(), "no request past the total");
    }

    #[test]
    fn api_url_comes_from_the_index_config() {
        let crates_io = r#"{"dl":"https://static.crates.io/crates","api":"https://crates.io"}"#;
        assert_eq!("https://crates.io", api_from_config(crates_io).unwrap());
        let mock = r#"{"dl":"x","api":"http://127.0.0.1:8000/"}"#;
        assert_eq!("http://127.0.0.1:8000", api_from_config(mock).unwrap());
        assert!(api_from_config(r#"{"dl":"x"}"#).is_err());
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

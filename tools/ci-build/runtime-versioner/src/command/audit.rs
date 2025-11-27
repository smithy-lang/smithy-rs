/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{
    repo::Repo,
    tag::{previous_release_tag, release_tags},
    util::utf8_path_buf,
    Audit,
};
use anyhow::{anyhow, bail, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use smithy_rs_tool_common::{
    command::sync::CommandExt, index::CratesIndex, package::PackageCategory,
    release_tag::ReleaseTag,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    process::Command,
};

pub fn audit(args: Audit) -> Result<()> {
    let repo = Repo::new(args.smithy_rs_path.as_deref())?;
    if !args.no_fetch {
        // Make sure we have the latest release tags
        fetch_smithy_rs_tags(&repo)?;
    }

    let release_tags = release_tags(&repo)?;
    let previous_release_tag =
        previous_release_tag(&repo, &release_tags, args.previous_release_tag.as_deref())?;
    if release_tags.first() != Some(&previous_release_tag) {
        tracing::warn!("there are newer releases since '{previous_release_tag}'. \
            Consider specifying a more recent release tag using the `--previous-release-tag` command-line argument or \
            the `SMITHY_RS_RUNTIME_VERSIONER_AUDIT_PREVIOUS_RELEASE_TAG` environment variable if audit fails.");
    }

    let next_crates = discover_runtime_crates(&repo.root).context("next")?;
    let previous_crates = resolve_previous_crates(&repo, previous_release_tag.as_str())?;

    let crates = augment_runtime_crates(previous_crates, next_crates, args.fake_crates_io_index)?;
    let mut errors = Vec::new();
    for rt_crate in crates {
        if let Err(err) = audit_crate(&repo, &previous_release_tag, rt_crate) {
            errors.push(err);
        }
    }
    if errors.is_empty() {
        println!("SUCCESS");
        Ok(())
    } else {
        for error in errors {
            eprintln!("{error}");
        }
        bail!("there are audit failures in the runtime crates")
    }
}

fn audit_crate(repo: &Repo, release_tag: &ReleaseTag, rt_crate: RuntimeCrate) -> Result<()> {
    if rt_crate.changed_since_release(repo, release_tag)? {
        // There is an edge case with the aws/rust-runtime crates due to the decoupled smithy-rs/SDK releases.
        // After a smithy-rs release and before a SDK release, there is a period of time where the smithy-rs
        // runtime crates are published to crates.io, but the SDK runtime crates are not.
        //
        // The way this manifests is that the crate's previous release version is now the same as the version
        // number at HEAD, and the crate is not published.
        let is_sdk_runtime_edge_case = PackageCategory::from_package_name(&rt_crate.name).is_sdk()
            && rt_crate.previous_release_version.as_ref() == Some(&rt_crate.next_release_version)
            && !rt_crate.next_version_is_published();
        if is_sdk_runtime_edge_case {
            tracing::info!("For '{}', detected that a new version of smithy-rs has been released, but that the SDK release hasn't caught up yet.", rt_crate.name);
        }

        // If this version has never been published before, then we're good.
        // (This tool doesn't check semver compatibility.)
        if !rt_crate.next_version_is_published() && !is_sdk_runtime_edge_case {
            if let Some(previous_version) = rt_crate.previous_release_version {
                tracing::info!(
                    "'{}' changed and was version bumped from {previous_version} to {}",
                    rt_crate.name,
                    rt_crate.next_release_version,
                );
            } else {
                tracing::info!(
                    "'{}' is a new crate (or wasn't independently versioned before) and will publish at {}",
                    rt_crate.name,
                    rt_crate.next_release_version,
                );
            }
            Ok(())
        } else if rt_crate.previous_release_version.as_ref() != Some(&rt_crate.next_release_version)
        {
            Err(anyhow!(
                "{crate_name} was changed and version bumped, but the new version \
                number ({version}) has already been published to crates.io. Choose a new \
                version number.",
                crate_name = rt_crate.name,
                version = rt_crate.next_release_version,
            ))
        } else {
            Err(anyhow!(
                "{crate_name} changed since {release_tag} and requires a version bump",
                crate_name = rt_crate.name
            ))
        }
    } else {
        // If it didn't change at all since last release, then we're good.
        Ok(())
    }
}

struct RuntimeCrate {
    name: String,
    path: Utf8PathBuf,
    previous_release_version: Option<String>,
    next_release_version: String,
    published_versions: Vec<String>,
}

impl RuntimeCrate {
    /// True if the runtime crate's next version exists in crates.io
    fn next_version_is_published(&self) -> bool {
        self.published_versions.contains(&self.next_release_version)
    }

    /// True if this runtime crate changed since the given release tag.
    fn changed_since_release(&self, repo: &Repo, release_tag: &ReleaseTag) -> Result<bool> {
        let status = repo
            .git([
                "diff",
                "--name-only",
                release_tag.as_str(),
                self.path.as_str(),
            ])
            .output()
            .with_context(|| format!("failed to git diff {}", self.name))?;
        let output = String::from_utf8(status.stdout)?;
        // When run during a release, this file is replaced with it's actual contents.
        // This breaks this git-based comparison and incorrectly requires a version bump.
        // Temporary fix to allow the build to succeed.
        let lines_to_ignore = &["aws-config/clippy.toml", "aws-config/Cargo.lock"];
        let changed_files = output
            .lines()
            .filter(|line| !lines_to_ignore.iter().any(|ignore| line.contains(ignore)))
            .collect::<Vec<_>>();
        Ok(!changed_files.is_empty())
    }
}

/// Loads version information from crates.io and attaches it to the passed in runtime crates.
fn augment_runtime_crates(
    previous_crates: BTreeMap<String, DiscoveredCrate>,
    next_crates: BTreeMap<String, DiscoveredCrate>,
    fake_crates_io_index: Option<Utf8PathBuf>,
) -> Result<Vec<RuntimeCrate>> {
    let index = fake_crates_io_index
        .map(CratesIndex::fake)
        .map(Ok)
        .unwrap_or_else(CratesIndex::real)?;
    let all_keys: BTreeSet<_> = previous_crates.keys().chain(next_crates.keys()).collect();
    let mut result = Vec::new();
    for key in all_keys {
        let previous_crate = previous_crates.get(key);
        if let Some(next_crate) = next_crates.get(key) {
            result.push(RuntimeCrate {
                published_versions: index.published_versions(&next_crate.name)?,
                name: next_crate.name.clone(),
                previous_release_version: previous_crate.map(|c| c.version.clone()),
                next_release_version: next_crate.version.clone(),
                path: next_crate.path.clone(),
            });
        } else {
            tracing::warn!("runtime crate '{key}' was removed and will not be published");
        }
    }
    Ok(result)
}

struct DiscoveredCrate {
    name: String,
    version: String,
    path: Utf8PathBuf,
}

/// Discovers runtime crates that are independently versioned.
/// For now, that just means the ones that don't have the special version number `0.0.0-smithy-rs-head`.
/// In the future, this can be simplified to just return all the runtime crates.
fn discover_runtime_crates(repo_root: &Utf8Path) -> Result<BTreeMap<String, DiscoveredCrate>> {
    const ROOT_PATHS: &[&str] = &["rust-runtime", "aws/rust-runtime"];
    let mut result = BTreeMap::new();
    for &root in ROOT_PATHS {
        let root = repo_root.join(root);
        for entry in fs::read_dir(&root)
            .context(root)
            .context("failed to read dir")?
        {
            let entry = entry.context("failed to read dir entry")?;
            if !entry.path().is_dir() {
                continue;
            }
            let manifest_path = entry.path().join("Cargo.toml");
            if !manifest_path.exists() {
                continue;
            }
            let manifest: toml::Value =
                toml::from_slice(&fs::read(&manifest_path).context("failed to read manifest")?)
                    .context("failed to parse manifest")?;
            let publish = manifest["package"]
                .get("publish")
                .and_then(|p| p.as_bool())
                .unwrap_or(true);
            let version = manifest["package"]["version"]
                .as_str()
                .expect("version is a string");
            if publish && version != "0.0.0-smithy-rs-head" {
                let name: String = entry.path().file_name().unwrap().to_string_lossy().into();
                result.insert(
                    name.clone(),
                    DiscoveredCrate {
                        name,
                        version: version.into(),
                        path: utf8_path_buf(entry.path()),
                    },
                );
            }
        }
    }
    Ok(result)
}

fn resolve_previous_crates(
    repo: &Repo,
    previous_release_tag: &str,
) -> Result<BTreeMap<String, DiscoveredCrate>> {
    // We checkout to a temp path so that this can be run with a dirty working tree
    // (for running in local development).
    let tempdir = tempfile::tempdir()?;
    let tempdir_path = Utf8Path::from_path(tempdir.path()).unwrap();
    let clone_path = tempdir_path.join("smithy-rs");
    fs::create_dir_all(&clone_path).context("failed to create temp smithy-rs repo")?;

    checkout_runtimes_to(repo, previous_release_tag, &clone_path)
        .context("resolve previous crates")?;
    discover_runtime_crates(&clone_path).context("resolve previous crates")
}

/// Fetches the latest tags from smithy-rs origin.
fn fetch_smithy_rs_tags(repo: &Repo) -> Result<()> {
    let output = repo
        .git(["remote", "get-url", "origin"])
        .output()
        .context("failed to verify origin git remote")?;
    let origin_url = String::from_utf8(output.stdout)
        .expect("valid utf-8")
        .trim()
        .to_string();
    if ![
        "git@github.com:smithy-lang/smithy-rs.git",
        "https://github.com/smithy-lang/smithy-rs.git",
    ]
    .iter()
    .any(|url| *url == origin_url)
    {
        bail!(
            "smithy-rs origin must be either 'git@github.com:smithy-lang/smithy-rs.git' or \
        'https://github.com/smithy-lang/smithy-rs.git' in order to get the latest release tags"
        );
    }

    repo.git(["fetch", "--tags", "origin"])
        .expect_success_output("fetch tags")?;
    Ok(())
}

fn checkout_runtimes_to(repo: &Repo, revision: &str, into: impl AsRef<Utf8Path>) -> Result<()> {
    Command::new("git")
        .arg("init")
        .current_dir(into.as_ref())
        .expect_success_output("init")?;
    let tmp_repo = Repo::new(Some(into.as_ref()))?;
    tmp_repo
        .git(["remote", "add", "origin", repo.root.as_str()])
        .expect_success_output("remote add origin")?;
    tmp_repo
        .git(["fetch", "origin", revision, "--depth", "1"])
        .expect_success_output("fetch revision")?;
    tmp_repo
        .git(["reset", "--hard", "FETCH_HEAD"])
        .expect_success_output("reset")?;
    Ok(())
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use crate::yank::yank;
use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgEnum, Parser};
use dialoguer::Confirm;
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::release_tag::ReleaseTag;
use smithy_rs_tool_common::versions_manifest::{Release, VersionsManifest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

const DEFAULT_DELAY_MILLIS: usize = 1000;

#[derive(Copy, Clone, Debug, ArgEnum, Eq, PartialEq, Ord, PartialOrd)]
pub enum CrateSet {
    /// (default) Yank all crates associated with the release.
    All,
    /// Yank all AWS SDK crates.
    AllAwsSdk,
    /// Yank generated AWS SDK crates.
    GeneratedAwsSdk,
}

#[derive(Parser, Debug)]
pub struct YankReleaseArgs {
    /// The aws-sdk-rust release tag to yank. The CLI will download the `versions.toml` file
    /// from GitHub at this tagged version to determine which crates to yank.
    #[clap(long, required_unless_present = "versions-toml")]
    github_release_tag: Option<String>,
    /// Path to a `versions.toml` file with a `[release]` section to yank.
    /// The `--github-release-tag` option is preferred to this, but this is provided as a fail safe.
    #[clap(long, required_unless_present = "github-release-tag")]
    versions_toml: Option<PathBuf>,
    #[clap(arg_enum)]
    crate_set: Option<CrateSet>,
    /// Time delay between crate yanking to avoid crates.io throttling errors.
    #[clap(long)]
    delay_millis: Option<usize>,
}

pub async fn subcommand_yank_release(
    YankReleaseArgs {
        github_release_tag,
        versions_toml,
        crate_set,
        delay_millis,
    }: &YankReleaseArgs,
) -> Result<()> {
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    // Retrieve information about the release to yank
    let release = match (github_release_tag, versions_toml) {
        (Some(release_tag), None) => acquire_release_from_tag(release_tag).await,
        (None, Some(versions_toml)) => acquire_release_from_file(versions_toml),
        _ => bail!("Only one of `--github-release-tag` or `--versions-toml` should be provided"),
    }
    .context("failed to retrieve information about the release to yank")?;

    let tag = release
        .tag
        .as_ref()
        .ok_or_else(|| {
            anyhow!("Versions manifest doesn't have a release tag. Can only yank tagged releases.")
        })?
        .clone();
    let crates = filter_crates(crate_set.unwrap_or(CrateSet::All), release);
    let _ = release;

    // Don't proceed unless the user confirms the plan
    confirm_plan(&tag, &crates)?;

    let delay_millis = Duration::from_millis(delay_millis.unwrap_or(DEFAULT_DELAY_MILLIS) as _);

    // Yank one crate at a time to try avoiding throttling errors
    for (crate_name, crate_version) in crates {
        yank(&crate_name, &crate_version).await?;

        // Keep things slow to avoid getting throttled by crates.io
        tokio::time::sleep(delay_millis).await;

        info!("Successfully yanked `{}-{}`", crate_name, crate_version);
    }

    Ok(())
}

fn filter_crates(crate_set: CrateSet, release: Release) -> BTreeMap<String, String> {
    if crate_set == CrateSet::All {
        return release.crates;
    }

    release
        .crates
        .into_iter()
        .filter(|c| {
            let category = PackageCategory::from_package_name(&c.0);
            match crate_set {
                CrateSet::All => unreachable!(),
                CrateSet::AllAwsSdk => category.is_sdk(),
                CrateSet::GeneratedAwsSdk => category == PackageCategory::AwsSdk,
            }
        })
        .collect()
}

async fn acquire_release_from_tag(tag: &str) -> Result<Release> {
    let tag = ReleaseTag::from_str(tag).context("invalid release tag")?;
    let manifest = VersionsManifest::from_github_tag(&tag)
        .await
        .context("failed to get versions.toml from GitHub")?;
    release_metadata(manifest)
}

fn acquire_release_from_file(path: &Path) -> Result<Release> {
    let parsed = VersionsManifest::from_file(path).context("failed to parse versions.toml file")?;
    release_metadata(parsed)
}

fn release_metadata(manifest: VersionsManifest) -> Result<Release> {
    if let Some(release) = manifest.release {
        Ok(release)
    } else {
        bail!("the versions.toml file didn't have a `[release]` section");
    }
}

fn confirm_plan(tag: &str, crates: &BTreeMap<String, String>) -> Result<()> {
    info!("This will yank aws-sdk-rust's `{tag}` release from crates.io.");
    info!("Crates to yank:");
    for (crate_name, crate_version) in crates {
        info!("   {}-{}", crate_name, crate_version);
    }

    if Confirm::new()
        .with_prompt(
            "Continuing will yank these crate versions from crates.io. Do you wish to continue?",
        )
        .interact()?
    {
        Ok(())
    } else {
        bail!("aborted")
    }
}

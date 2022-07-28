/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use dialoguer::Confirm;
use smithy_rs_tool_common::release_tag::ReleaseTag;
use smithy_rs_tool_common::shell::ShellOperation;
use smithy_rs_tool_common::versions_manifest::{Release, VersionsManifest};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

const MAX_CONCURRENCY: usize = 5;

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
}

pub async fn subcommand_yank_release(
    YankReleaseArgs {
        github_release_tag,
        versions_toml,
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

    // Don't proceed unless the user confirms the plan
    confirm_plan(&release)?;

    // Use a semaphore to only allow a few concurrent yanks
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
    info!(
        "Will yank {} crates in parallel where possible.",
        MAX_CONCURRENCY
    );

    let mut tasks = Vec::new();
    for (crate_name, crate_version) in release.crates {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        tasks.push(tokio::spawn(async move {
            info!("Yanking `{}-{}`...", crate_name, crate_version);
            let result = cargo::Yank::new(&crate_name, &crate_version).spawn().await;
            drop(permit);
            if result.is_ok() {
                info!("Successfully yanked `{}-{}`", crate_name, crate_version);
            }
            result
        }));
    }
    for task in tasks {
        task.await??;
    }

    Ok(())
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

fn confirm_plan(release: &Release) -> Result<()> {
    let tag = release.tag.as_ref().ok_or_else(|| {
        anyhow!("Versions manifest doesn't have a release tag. Can only yank tagged releases.")
    })?;

    info!("This will yank aws-sdk-rust's `{tag}` release from crates.io.");
    info!("Crates to yank:");
    for (crate_name, crate_version) in &release.crates {
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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use anyhow::{bail, Context, Result};
use clap::Parser;
use dialoguer::Confirm;
use regex::Regex;
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

fn validate_tag(tag: &str) -> Result<()> {
    if !Regex::new(r#"(v\d+.\d+.\d+)|(\d{4}-\d{2}-\d{2})"#)
        .unwrap()
        .is_match(tag)
    {
        bail!("invalid release tag");
    }
    Ok(())
}

async fn acquire_release_from_tag(tag: &str) -> Result<Release> {
    validate_tag(tag)?;

    let manifest_url = format!(
        "https://raw.githubusercontent.com/awslabs/aws-sdk-rust/{}/versions.toml",
        tag
    );
    info!("Downloading versions.toml from {}", manifest_url);
    let manifest_contents = reqwest::get(manifest_url)
        .await
        .context("failed to retrieve release manifest")?
        .text()
        .await
        .context("failed to retrieve release manifest content")?;
    let parsed = VersionsManifest::from_str(&manifest_contents)
        .context("failed to parse versions.toml file")?;
    release_metadata(parsed)
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
    info!(
        "This will yank aws-sdk-rust's `{}` release from crates.io.",
        release.tag
    );
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

#[cfg(test)]
mod tests {
    use super::validate_tag;

    #[test]
    fn test_validate_tag() {
        assert!(validate_tag("v0.12.0").is_ok());
        assert!(validate_tag("v0.12.1").is_ok());
        assert!(validate_tag("v1.12.1").is_ok());
        assert!(validate_tag("v10.12.11").is_ok());
        assert!(validate_tag("2022-05-24").is_ok());

        assert!(validate_tag("bad").is_err());
        assert!(validate_tag("2022-5-4").is_err());
        assert!(validate_tag("0.12.0").is_err());
        assert!(validate_tag("v0.12").is_err());
        assert!(validate_tag("url/injection").is_err());
        assert!(validate_tag("..").is_err());
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::fs::Fs;
use crate::package::{
    discover_and_validate_package_batches, Package, PackageBatch, PackageHandle, PackageStats,
};
use crate::repo::{find_git_repository_root, resolve_publish_location};
use crate::shell::ShellOperation;
use crate::CRATE_OWNER;
use crate::{cargo, SDK_REPO_NAME};
use anyhow::{bail, Result};
use crates_io_api::{AsyncClient, Error};
use dialoguer::Confirm;
use lazy_static::lazy_static;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::info;

lazy_static! {
    static ref CRATES_IO_CLIENT: AsyncClient = AsyncClient::new(
        "AWS_RUST_SDK_PUBLISHER (aws-sdk-rust@amazon.com)",
        Duration::from_secs(1)
    )
    .expect("valid client");
}

pub async fn subcommand_publish(location: &Path) -> Result<()> {
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let location = resolve_publish_location(location).await;

    info!("Discovering crates to publish...");
    let (batches, stats) = discover_and_validate_package_batches(Fs::Real, &location).await?;
    info!("Finished crate discovery.");

    // Sanity check the repository tag if publishing from `aws-sdk-rust`
    confirm_correct_tag(&batches, &location).await?;

    // Don't proceed unless the user confirms the plan
    confirm_plan(&batches, stats)?;

    // Use a semaphore to only allow a few concurrent publishes
    let max_concurrency = num_cpus::get_physical();
    let semaphore = Arc::new(Semaphore::new(max_concurrency));
    info!(
        "Will publish {} crates in parallel where possible.",
        max_concurrency
    );
    for batch in batches {
        let mut tasks = Vec::new();
        for package in batch {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            tasks.push(tokio::spawn(async move {
                // Only publish if it hasn't been published yet.
                if !is_published(&package.handle).await? {
                    info!("Publishing `{}`...", package.handle);
                    cargo::Publish::new(&package.handle, &package.crate_path)
                        .spawn()
                        .await?;
                    // Sometimes it takes a little bit of time for the new package version
                    // to become available after publish. If we proceed too quickly, then
                    // the next package publish can fail if it depends on this package.
                    wait_for_eventual_consistency(&package).await?;
                    info!("Successfully published `{}`", package.handle);
                } else {
                    info!("`{}` was already published", package.handle);
                }
                correct_owner(&package).await?;
                drop(permit);
                Ok::<_, anyhow::Error>(())
            }));
        }
        for task in tasks {
            task.await??;
        }
        info!("sleeping 30 seconds after completion of the batch");
        tokio::time::sleep(Duration::from_secs(30)).await;
    }

    Ok(())
}

async fn confirm_correct_tag(batches: &[Vec<Package>], location: &Path) -> Result<()> {
    let aws_config_version = batches
        .iter()
        .flat_map(|batch| batch.iter().find(|p| p.handle.name == "aws-config"))
        .map(|package| &package.handle.version)
        .next();
    if let Some(aws_config_version) = aws_config_version {
        let expected_tag = format!("v{}", aws_config_version);
        let repository = find_git_repository_root(SDK_REPO_NAME, location).await?;
        if expected_tag != repository.current_tag {
            bail!(
                "Current tag `{}` in the local `aws-sdk-rust` repository didn't match expected release tag `{}`",
                repository.current_tag,
                expected_tag
            );
        }
    }
    Ok(())
}

async fn is_published(handle: &PackageHandle) -> Result<bool> {
    let expected_version = handle.version.to_string();
    let crate_info = match CRATES_IO_CLIENT.get_crate(&handle.name).await {
        Ok(info) => info,
        Err(Error::NotFound(_)) => return Ok(false),
        Err(other) => return Err(other.into()),
    };
    Ok(crate_info
        .versions
        .iter()
        .any(|crate_version| crate_version.num == expected_version))
}

/// Waits for the given package to show up on crates.io
async fn wait_for_eventual_consistency(package: &Package) -> Result<()> {
    let max_wait_time = 10usize;
    for _ in 0..max_wait_time {
        if !is_published(&package.handle).await? {
            tokio::time::sleep(Duration::from_secs(1)).await;
        } else {
            return Ok(());
        }
    }
    if !is_published(&package.handle).await? {
        return Err(anyhow::Error::msg(format!(
            "package wasn't found on crates.io {} seconds after publish",
            max_wait_time
        )));
    }
    Ok(())
}

/// Corrects the crate ownership.
async fn correct_owner(package: &Package) -> Result<()> {
    let owners = cargo::GetOwners::new(&package.handle.name).spawn().await?;
    if !owners.iter().any(|owner| owner == CRATE_OWNER) {
        cargo::AddOwner::new(&package.handle.name, CRATE_OWNER)
            .spawn()
            .await?;
        info!("Corrected crate ownership of `{}`", package.handle);
    }
    Ok(())
}

fn confirm_plan(batches: &[PackageBatch], stats: PackageStats) -> Result<()> {
    let mut full_plan = Vec::new();
    for batch in batches {
        for package in batch {
            full_plan.push(format!(
                "Publish version `{}` of `{}`",
                package.handle.version, package.handle.name
            ));
        }
        full_plan.push("-- wait --".into());
    }

    info!("Publish plan:");
    for item in full_plan {
        println!("  {}", item);
    }
    info!(
        "Will publish {} crates total ({} Smithy runtime, {} AWS runtime, {} AWS SDK).",
        stats.total(),
        stats.smithy_runtime_crates,
        stats.aws_runtime_crates,
        stats.aws_sdk_crates
    );

    if Confirm::new()
        .with_prompt("Continuing will publish to crates.io. Do you wish to continue?")
        .interact()?
    {
        Ok(())
    } else {
        Err(anyhow::Error::msg("aborted"))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::package::PackageHandle;

    #[ignore]
    #[tokio::test]
    async fn crate_published_works() {
        let handle = PackageHandle::new("aws-smithy-http", "0.27.0-alpha.1".parse().unwrap());
        assert_eq!(is_published(&handle).await.expect("failed"), true);
        // we will never publish this version
        let handle = PackageHandle::new("aws-smithy-http", "0.21.0-alpha.1".parse().unwrap());
        assert_eq!(is_published(&handle).await.expect("failed"), false);
    }
}

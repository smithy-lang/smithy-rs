/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use crate::package::{
    discover_and_validate_package_batches, Package, PackageBatch, PackageHandle, PackageStats,
};
use crate::retry::{run_with_retry, BoxError, ErrorClass};
use crate::SDK_REPO_CRATE_PATH;
use crate::{cargo, SDK_REPO_NAME};
use anyhow::{bail, Context, Result};
use clap::Parser;
use crates_io_api::{AsyncClient, Error};
use dialoguer::Confirm;
use lazy_static::lazy_static;
use smithy_rs_tool_common::git;
use smithy_rs_tool_common::shell::ShellOperation;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

#[derive(Parser, Debug)]
pub struct PublishArgs {
    /// Path containing the crates to publish. Crates will be discovered recursively
    #[clap(long)]
    location: PathBuf,

    /// Don't prompt for confirmation before publishing
    #[clap(short('y'))]
    skip_confirmation: bool,
}

pub async fn subcommand_publish(
    PublishArgs {
        location,
        skip_confirmation,
    }: &PublishArgs,
) -> Result<()> {
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let location = resolve_publish_location(location);

    info!("Discovering crates to publish...");
    let (batches, stats) = discover_and_validate_package_batches(Fs::Real, &location).await?;
    info!("Finished crate discovery.");

    // Don't proceed unless the user confirms the plan
    confirm_plan(&batches, stats, *skip_confirmation)?;

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
                    publish(&package.handle, &package.crate_path).await?;

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

/// Given a `location`, this function looks for the `aws-sdk-rust` git repository. If found,
/// it resolves the `sdk/` directory. Otherwise, it returns the original `location`.
pub fn resolve_publish_location(location: &Path) -> PathBuf {
    match git::find_git_repository_root(SDK_REPO_NAME, location) {
        // If the given path was the `aws-sdk-rust` repo root, then resolve the `sdk/` directory to publish from
        Ok(sdk_repo) => sdk_repo.join(SDK_REPO_CRATE_PATH),
        // Otherwise, publish from the given path (likely the smithy-rs runtime bundle)
        Err(_) => location.into(),
    }
}

async fn publish(handle: &PackageHandle, crate_path: &Path) -> Result<()> {
    info!("Publishing `{}`...", handle);
    run_with_retry(
        &format!("Publishing `{}`", handle),
        3,
        Duration::from_secs(5),
        || async {
            cargo::Publish::new(handle.clone(), &crate_path)
                .spawn()
                .await?;
            Result::<_, BoxError>::Ok(())
        },
        |_err| ErrorClass::Retry,
    )
    .await?;
    Ok(())
}

async fn is_published(handle: &PackageHandle) -> Result<bool> {
    run_with_retry(
        &format!("Checking if `{}` is already published", handle.name),
        3,
        Duration::from_secs(5),
        || async {
            let expected_version = (&handle.version).to_string();
            let crate_info = match CRATES_IO_CLIENT.get_crate(&handle.name).await {
                Ok(info) => info,
                Err(Error::NotFound(_)) => return Ok(false),
                Err(other) => return Err(other),
            };
            Ok(crate_info
                .versions
                .iter()
                .any(|crate_version| crate_version.num == expected_version))
        },
        |err| match err {
            Error::Http(_) => ErrorClass::Retry,
            _ => ErrorClass::NoRetry,
        },
    )
    .await
    .context("is_published")
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
    run_with_retry(
        &format!("Correcting ownership of `{}`", package.handle.name),
        3,
        Duration::from_secs(5),
        || async {
            let actual_owners: HashSet<String> = cargo::GetOwners::new(&package.handle.name).spawn().await?.into_iter().collect();
            let expected_owners = package.expected_owners();

            let owners_to_be_added = expected_owners.difference(&actual_owners);
            let incorrect_owners = actual_owners.difference(&expected_owners);

            let mut added_individual = false;
            for crate_owner in owners_to_be_added {
                cargo::AddOwner::new(&package.handle.name, crate_owner)
                    .spawn()
                    .await?;
                info!("Added `{}` as owner of `{}`", crate_owner, package.handle);
                // Teams in crates.io start with `github:` while individuals are just the GitHub user name
                added_individual |= !crate_owner.starts_with("github:");
            }
            for incorrect_owner in incorrect_owners {
                // Adding an individual owner requires accepting an invite, so don't attempt to remove
                // anyone if an owner was added, as removing the last individual owner may break.
                // The next publish run will remove the incorrect owner.
                if !added_individual {
                    cargo::RemoveOwner::new(&package.handle.name, incorrect_owner)
                        .spawn()
                        .await
                        .context(format!("remove incorrect owner `{}` from crate `{}`", incorrect_owner, package.handle))?;
                    info!(
                        "Removed incorrect owner `{}` from crate `{}`",
                        incorrect_owner, package.handle
                    );
                } else {
                    info!("Skipping removal of incorrect owner `{}` from crate `{}` due to new owners", incorrect_owner, package.handle);
                }
            }
            Result::<_, BoxError>::Ok(())
        },
        |_err| ErrorClass::Retry,
    )
    .await
    .context("correct_owner")
}

fn confirm_plan(
    batches: &[PackageBatch],
    stats: PackageStats,
    skip_confirmation: bool,
) -> Result<()> {
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

    if skip_confirmation
        || Confirm::new()
            .with_prompt("Continuing will publish to crates.io. Do you wish to continue?")
            .interact()?
    {
        Ok(())
    } else {
        bail!("aborted")
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

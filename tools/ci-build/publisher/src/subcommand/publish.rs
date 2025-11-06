/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use crate::package::{discover_and_validate_package_batches, PackageBatch, PackageStats};
use crate::publish::publish;
use crate::{cargo, SDK_REPO_CRATE_PATH, SDK_REPO_NAME};
use anyhow::{bail, Context, Result};
use clap::Parser;
use dialoguer::Confirm;
use smithy_rs_tool_common::package::{Package, PackageHandle};
use smithy_rs_tool_common::retry::{run_with_retry, BoxError, ErrorClass};
use smithy_rs_tool_common::shell::ShellOperation;
use smithy_rs_tool_common::{git, index::CratesIndex};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{collections::HashSet, sync::Arc};
use tracing::info;

const DEFAULT_DELAY_MILLIS: usize = 1000;

#[derive(Parser, Debug)]
pub struct PublishArgs {
    /// Path containing the crates to publish. Crates will be discovered recursively
    #[clap(long)]
    location: PathBuf,

    /// Don't prompt for confirmation before publishing
    #[clap(short('y'))]
    skip_confirmation: bool,

    /// Time delay between crate publishes to avoid crates.io throttling errors.
    #[clap(long)]
    delay_millis: Option<usize>,
}

pub async fn subcommand_publish(
    PublishArgs {
        location,
        skip_confirmation,
        delay_millis,
    }: &PublishArgs,
) -> Result<()> {
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let location = resolve_publish_location(location);
    let delay_millis = Duration::from_millis(delay_millis.unwrap_or(DEFAULT_DELAY_MILLIS) as _);

    info!("Discovering crates to publish...");
    let (batches, stats) = discover_and_validate_package_batches(Fs::Real, &location).await?;
    info!("Finished crate discovery.");

    // Don't proceed unless the user confirms the plan
    confirm_plan(&batches, stats, *skip_confirmation)?;

    let index = Arc::new(CratesIndex::real()?);
    for batch in &batches {
        let mut any_published = false;
        for package in batch {
            // Only publish if it hasn't been published yet.
            if !is_published(index.clone(), &package.handle).await? {
                publish(&package.handle, &package.crate_path).await?;

                // Keep things slow to avoid getting throttled by crates.io
                tokio::time::sleep(delay_millis).await;

                // Sometimes it takes a little bit of time for the new package version
                // to become available after publish. If we proceed too quickly, then
                // the next package publish can fail if it depends on this package.
                wait_for_eventual_consistency(index.clone(), package).await?;
                info!("Successfully published `{}`", &package.handle);
                any_published = true;
            } else {
                info!("`{}` was already published", &package.handle);
            }
        }
        if any_published {
            info!("Sleeping 30 seconds after completion of the batch");
            tokio::time::sleep(Duration::from_secs(30)).await;
        } else {
            info!("No packages in the batch needed publishing. Proceeding with the next batch immediately.")
        }
    }

    for batch in &batches {
        for package in batch {
            correct_owner(&package.handle).await?;
        }
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

async fn is_published(index: Arc<CratesIndex>, handle: &PackageHandle) -> Result<bool> {
    let name = handle.name.clone();
    let version = handle.expect_version().clone();
    tokio::task::spawn_blocking(move || {
        smithy_rs_tool_common::index::is_published(index.as_ref(), &name, &version)
    })
    .await?
}

/// Waits for the given package to show up on crates.io
async fn wait_for_eventual_consistency(index: Arc<CratesIndex>, package: &Package) -> Result<()> {
    // This timeout value is quite long (it started at 10 seconds), but is currently increased to handle
    // recent slowdowns we have seen with crates.io populating the sparse index. You can find some discussion
    // here: https://rust-lang.zulipchat.com/#narrow/channel/318791-t-crates-io/topic/sparse.20index.20updates.20sometimes.20being.20rate.20limited/with/529157932
    // and a tracking issue at https://github.com/rust-lang/crates.io/issues/11588
    let max_wait_time = 180usize;
    for _ in 0..max_wait_time {
        if !is_published(index.clone(), &package.handle).await? {
            tokio::time::sleep(Duration::from_secs(1)).await;
        } else {
            return Ok(());
        }
    }
    if !is_published(index.clone(), &package.handle).await? {
        return Err(anyhow::Error::msg(format!(
            "package wasn't found on crates.io {max_wait_time} seconds after publish"
        )));
    }
    Ok(())
}

/// Corrects the crate ownership.
pub async fn correct_owner(handle: &PackageHandle) -> Result<()> {
    // https://github.com/orgs/awslabs/teams/smithy-rs-server
    const SMITHY_RS_SERVER_OWNER: &str = "github:awslabs:smithy-rs-server";
    // https://github.com/orgs/awslabs/teams/rust-sdk-owners
    const RUST_SDK_OWNER: &str = "github:awslabs:rust-sdk-owners";

    run_with_retry(
        &format!("Correcting ownership of `{}`", handle.name),
        3,
        Duration::from_secs(5),
        || async {
            let actual_owners: HashSet<String> = cargo::GetOwners::new(&handle.name).spawn().await?.into_iter().collect();
            let expected_owners = handle.expected_owners().iter().map(|s| s.to_string()).collect::<HashSet<_>>();

            let owners_to_be_added = expected_owners.difference(&actual_owners);
            let owners_to_be_removed = actual_owners.difference(&expected_owners);

            let mut added_individual = false;
            for crate_owner in owners_to_be_added {
                cargo::AddOwner::new(&handle.name, crate_owner)
                    .spawn()
                    .await?;
                info!("Added `{}` as owner of `{}`", crate_owner, handle);
                // Teams in crates.io start with `github:` while individuals are just the GitHub user name
                added_individual |= !crate_owner.starts_with("github:");
            }
            for crate_owner in owners_to_be_removed {
                // Trying to remove them will result in an error due to a bug in crates.io
                // Upstream tracking issue: https://github.com/rust-lang/crates.io/issues/2736
                if crate_owner == SMITHY_RS_SERVER_OWNER || crate_owner == RUST_SDK_OWNER {
                    continue;
                }
                // Adding an individual owner requires accepting an invite, so don't attempt to remove
                // anyone if an owner was added, as removing the last individual owner may break.
                // The next publish run will remove the incorrect owner.
                if !added_individual {
                    cargo::RemoveOwner::new(&handle.name, crate_owner)
                        .spawn()
                        .await
                        .with_context(|| format!("remove incorrect owner `{crate_owner}` from crate `{handle}`"))?;
                    info!(
                        "Removed incorrect owner `{}` from crate `{}`",
                        crate_owner, handle
                    );
                } else {
                    info!("Skipping removal of incorrect owner `{}` from crate `{}` due to new owners", crate_owner, handle);
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
                package.handle.expect_version(),
                package.handle.name
            ));
        }
        full_plan.push("-- wait --".into());
    }

    info!("Publish plan:");
    for item in full_plan {
        println!("  {item}");
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

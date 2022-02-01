/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::cargo;
use crate::fs::Fs;
use crate::package::{discover_and_validate_package_batches, Package, PackageHandle, Publish};
use crate::repo::resolve_publish_location;
use anyhow::{bail, Result};
use dialoguer::Confirm;
use semver::Version;
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::shell::ShellOperation;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

const MAX_CONCURRENCY: usize = 5;

pub async fn subcommand_yank_category(
    category: &str,
    version: Version,
    location: &Path,
) -> Result<()> {
    let category = match category {
        "aws-runtime" => PackageCategory::AwsRuntime,
        "aws-sdk" => PackageCategory::AwsSdk,
        "smithy-runtime" => PackageCategory::SmithyRuntime,
        _ => {
            return Err(anyhow::Error::msg(format!(
                "unrecognized package category: {}",
                category
            )));
        }
    };

    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let location = resolve_publish_location(location).await;

    info!("Discovering crates to yank...");
    let (batches, _) = discover_and_validate_package_batches(Fs::Real, &location).await?;
    let packages: Vec<Package> = batches
        .into_iter()
        .flatten()
        .filter(|p| p.publish == Publish::Allowed && p.category == category)
        .map(|p| {
            if p.handle.version != version {
                bail!(
                    "Version to yank, `{}`, does not match locally checked out version of `{}` (`{}`) in {:?}",
                    version, p.handle.name, p.handle.version, p.crate_path
                );
            }
            Ok(Package::new(
                // Replace the version with the version given on the CLI
                PackageHandle::new(p.handle.name, version.clone()),
                p.manifest_path,
                p.local_dependencies,
                Publish::Allowed,
            ))
        })
        .collect::<Result<Vec<Package>>>()?;
    info!("Finished crate discovery.");

    // Don't proceed unless the user confirms the plan
    confirm_plan(&packages)?;

    // Use a semaphore to only allow a few concurrent yanks
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
    info!(
        "Will yank {} crates in parallel where possible.",
        MAX_CONCURRENCY
    );

    let mut tasks = Vec::new();
    for package in packages {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        tasks.push(tokio::spawn(async move {
            info!("Yanking `{}`...", package.handle);
            let result = cargo::Yank::new(package.handle.clone(), &package.crate_path)
                .spawn()
                .await;
            drop(permit);
            info!("Successfully yanked `{}`", package.handle);
            result
        }));
    }
    for task in tasks {
        task.await??;
    }

    Ok(())
}

fn confirm_plan(packages: &[Package]) -> Result<()> {
    info!("Yank plan:");
    for package in packages {
        println!(
            "Yank version `{}` of `{}`",
            package.handle.version, package.handle.name
        );
    }

    if Confirm::new()
        .with_prompt("Continuing will yank crate versions from crates.io. Do you wish to continue?")
        .interact()?
    {
        Ok(())
    } else {
        Err(anyhow::Error::msg("aborted"))
    }
}

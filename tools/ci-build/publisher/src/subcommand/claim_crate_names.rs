/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::fs::Fs;
use crate::package::{discover_packages, PackageHandle, Publish};
use crate::publish::{has_been_published_on_crates_io, publish};
use crate::subcommand::publish::correct_owner;
use crate::{cargo, SDK_REPO_NAME};
use clap::Parser;
use dialoguer::Confirm;
use semver::Version;
use smithy_rs_tool_common::git;
use smithy_rs_tool_common::package::PackageCategory;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;

#[derive(Parser, Debug)]
pub struct ClaimCrateNamesArgs {
    /// Don't prompt for confirmation before publishing
    #[clap(short('y'))]
    skip_confirmation: bool,
}

pub async fn subcommand_claim_crate_names(args: &ClaimCrateNamesArgs) -> anyhow::Result<()> {
    let ClaimCrateNamesArgs { skip_confirmation } = args;
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let smithy_rs_repository_root =
        git::find_git_repository_root(SDK_REPO_NAME, std::env::current_dir()?)?;
    let packages = discover_publishable_crate_names(&smithy_rs_repository_root).await?;
    let unpublished_package_names = {
        let mut s = HashSet::new();
        for package_name in packages {
            if !has_been_published_on_crates_io(&package_name).await? {
                s.insert(package_name);
            }
        }
        s
    };

    confirm_user_intent(&unpublished_package_names, *skip_confirmation)?;

    if unpublished_package_names.is_empty() {
        info!("All publishable packages already exist on crates.io - nothing to do.");
        return Ok(());
    }

    for name in unpublished_package_names {
        claim_crate_name(&name).await?;
    }

    Ok(())
}

async fn claim_crate_name(name: &str) -> anyhow::Result<()> {
    let temporary_directory = tempfile::tempdir()?;
    let crate_dir_path = temporary_directory.path();
    create_dummy_lib_crate(Fs::Real, name, crate_dir_path.to_path_buf()).await?;

    let category = PackageCategory::from_package_name(name);
    let package_handle = PackageHandle::new(name, Version::new(0, 0, 1));
    publish(&package_handle, crate_dir_path).await?;
    // Keep things slow to avoid getting throttled by crates.io
    tokio::time::sleep(Duration::from_secs(2)).await;
    info!("Successfully published `{}`", package_handle);
    correct_owner(&package_handle, &category).await?;
    Ok(())
}

/// Return the list of publishable crate names in the `smithy-rs` git repository.
async fn discover_publishable_crate_names(repository_root: &Path) -> anyhow::Result<Vec<String>> {
    async fn _discover_publishable_crate_names(
        fs: Fs,
        path: PathBuf,
    ) -> anyhow::Result<HashSet<String>> {
        let packages = discover_packages(fs, path).await?;
        let mut publishable_package_names = HashSet::new();
        for package in packages {
            if let Publish::Allowed = package.publish {
                publishable_package_names.insert(package.handle.name);
            }
        }
        Ok(publishable_package_names)
    }

    let packages = {
        let mut p = vec![];
        info!("Discovering publishable crates...");
        p.extend(
            _discover_publishable_crate_names(Fs::Real, repository_root.join("rust-runtime"))
                .await?,
        );
        p.extend(
            _discover_publishable_crate_names(
                Fs::Real,
                repository_root.join("aws").join("rust-runtime"),
            )
            .await?,
        );
        info!("Finished crate discovery.");
        p
    };
    Ok(packages)
}

async fn create_dummy_lib_crate(
    fs: Fs,
    package_name: &str,
    directory_path: PathBuf,
) -> anyhow::Result<()> {
    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.0.1"
edition = "2021"
description = "Placeholder ahead of the next smithy-rs release"
license = "Apache-2.0"
repository = "https://github.com/awslabs/smithy-rs""#,
        package_name
    );
    fs.write_file(directory_path.join("Cargo.toml"), cargo_toml.as_bytes())
        .await?;
    let src_dir_path = directory_path.join("src");
    fs.create_dir_all(&src_dir_path).await?;
    fs.write_file(src_dir_path.join("lib.rs"), &[]).await?;
    Ok(())
}

fn confirm_user_intent(
    crate_names: &HashSet<String>,
    skip_confirmation: bool,
) -> anyhow::Result<()> {
    use std::fmt::Write;

    let prompt = {
        let mut s = String::new();
        writeln!(
            &mut s,
            "The following new crate names will be claimed on crates.io:"
        )?;
        for c in crate_names {
            writeln!(&mut s, "- {c}")?;
        }
        writeln!(&mut s)?;
        s
    };
    if !(skip_confirmation || Confirm::new().with_prompt(&prompt).interact()?) {
        anyhow::bail!("Aborting")
    }
    Ok(())
}

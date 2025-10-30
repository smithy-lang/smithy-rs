/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::publish::is_published;
use crate::publish::publish;
use crate::subcommand::publish::correct_owner;
use crate::{cargo, SDK_REPO_NAME};
use crate::{fs::Fs, package::discover_manifests};
use anyhow::{Context, Result};
use cargo_toml::Manifest;
use clap::Parser;
use dialoguer::Confirm;
use semver::Version;
use smithy_rs_tool_common::package::PackageHandle;
use smithy_rs_tool_common::{git, index::CratesIndex};
use std::time::Duration;
use std::{collections::HashSet, fs};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::info;

#[derive(Parser, Debug)]
pub struct ClaimCrateNamesArgs {
    /// Don't prompt for confirmation before publishing
    #[clap(short('y'))]
    skip_confirmation: bool,
}

pub async fn subcommand_claim_crate_names(args: &ClaimCrateNamesArgs) -> Result<()> {
    let ClaimCrateNamesArgs { skip_confirmation } = args;
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let smithy_rs_repository_root =
        git::find_git_repository_root(SDK_REPO_NAME, std::env::current_dir()?)?;
    let index = Arc::new(CratesIndex::real()?);
    let packages = discover_publishable_crate_names(&smithy_rs_repository_root).await?;
    let unpublished_package_names = {
        let mut s = HashSet::new();
        for package_name in packages {
            if !is_published(index.clone(), &package_name).await? {
                s.insert(package_name);
            }
        }
        s
    };

    if unpublished_package_names.is_empty() {
        info!("All publishable packages already exist on crates.io - nothing to do.");
        return Ok(());
    }

    confirm_user_intent(&unpublished_package_names, *skip_confirmation)?;
    for name in unpublished_package_names {
        claim_crate_name(&name).await?;
    }

    Ok(())
}

async fn claim_crate_name(name: &str) -> Result<()> {
    let temporary_directory = tempfile::tempdir()?;
    let crate_dir_path = temporary_directory.path();
    create_dummy_lib_crate(Fs::Real, name, crate_dir_path.to_path_buf()).await?;

    let package_handle = PackageHandle::new(name, Some(Version::new(0, 0, 1)));
    publish(&package_handle, crate_dir_path).await?;

    // Keep things slow to avoid getting throttled by crates.io
    tokio::time::sleep(Duration::from_secs(2)).await;
    correct_owner(&package_handle).await?;

    info!("Successfully published `{}`", package_handle);
    Ok(())
}

async fn load_publishable_crate_names(path: &Path) -> Result<HashSet<String>> {
    let manifest_paths = discover_manifests(path).await?;
    let mut result = HashSet::new();
    for manifest_path in &manifest_paths {
        let content =
            fs::read(manifest_path).with_context(|| format!("failed to read {path:?}"))?;
        let manifest = Manifest::from_slice(&content)
            .with_context(|| format!("failed to load crate manifest for {path:?}"))?;
        if let Some(package) = manifest.package {
            let crate_name = package.name();
            if matches!(package.publish(), cargo_toml::Publish::Flag(true)) {
                result.insert(crate_name.to_string());
            }
        }
    }
    Ok(result)
}

/// Return the list of publishable crate names in the `smithy-rs` git repository.
async fn discover_publishable_crate_names(repository_root: &Path) -> Result<Vec<String>> {
    let packages = {
        let mut p = vec![];
        info!("Discovering publishable crates...");
        p.extend(load_publishable_crate_names(&repository_root.join("rust-runtime")).await?);
        p.extend(
            load_publishable_crate_names(&repository_root.join("aws").join("rust-runtime")).await?,
        );
        info!("Finished crate discovery.");
        p
    };
    Ok(packages)
}

async fn create_dummy_lib_crate(fs: Fs, package_name: &str, directory_path: PathBuf) -> Result<()> {
    let cargo_toml = format!(
        r#"[package]
name = "{package_name}"
version = "0.0.1"
edition = "2021"
description = "Placeholder ahead of the next smithy-rs release"
license = "Apache-2.0"
repository = "https://github.com/smithy-lang/smithy-rs""#
    );
    fs.write_file(directory_path.join("Cargo.toml"), cargo_toml.as_bytes())
        .await?;
    let src_dir_path = directory_path.join("src");
    fs.create_dir_all(&src_dir_path).await?;
    fs.write_file(src_dir_path.join("lib.rs"), &[]).await?;
    Ok(())
}

fn confirm_user_intent(crate_names: &HashSet<String>, skip_confirmation: bool) -> Result<()> {
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

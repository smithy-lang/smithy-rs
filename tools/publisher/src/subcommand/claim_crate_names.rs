use crate::fs::Fs;
use crate::package::{discover_package_manifests, Error};
use crate::publish::has_been_published_on_crates_io;
use crate::{cargo, SDK_REPO_NAME};
use anyhow::Context;
use cargo_toml::Manifest;
use clap::Parser;
use smithy_rs_tool_common::git;
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
pub struct ClaimCrateNamesArgs {
    /// Path containing the crates whose names should be claimed on crates.io.
    /// Crates will be discovered recursively
    #[clap(long)]
    location: PathBuf,

    /// Don't prompt for confirmation before publishing
    #[clap(short('y'))]
    skip_confirmation: bool,
}

pub async fn subcommand_claim_crate_names(args: &ClaimCrateNamesArgs) -> anyhow::Result<()> {
    let ClaimCrateNamesArgs {
        location,
        skip_confirmation,
    } = args;
    // Make sure cargo exists
    cargo::confirm_installed_on_path()?;

    let repository_root = git::find_git_repository_root(SDK_REPO_NAME, location)?;
    let mut packages = vec![];
    info!("Discovering publishable crates...");
    packages.extend(
        discover_publishable_crate_names(Fs::Real, repository_root.join("rust-runtime")).await?,
    );
    packages.extend(
        discover_publishable_crate_names(
            Fs::Real,
            repository_root.join("aws").join("rust-runtime"),
        )
        .await?,
    );
    info!("Finished crate discovery.");

    let mut unpublished_packages = HashSet::new();
    for package_name in packages {
        if !has_been_published_on_crates_io(&package_name).await? {
            unpublished_packages.insert(package_name);
        }
    }

    if unpublished_packages.is_empty() {
        info!("All publishable packages already exist on crates.io - nothing to do.");
        return Ok(());
    }
    dbg!(unpublished_packages);
    Ok(())
}

async fn discover_publishable_crate_names(
    fs: Fs,
    path: PathBuf,
) -> anyhow::Result<HashSet<String>> {
    let manifest_paths = discover_package_manifests(path).await?;
    let mut publishable_package_names = HashSet::new();
    for manifest_path in manifest_paths {
        let contents: Vec<u8> = fs.read_file(&manifest_path).await?;
        let manifest = Manifest::from_slice(&contents)
            .with_context(|| format!("failed to load package manifest for {:?}", manifest_path))?;
        let package = manifest
            .package
            .ok_or_else(move || Error::InvalidManifest(manifest_path))
            .context("crate manifest doesn't have a `[package]` section")?;
        let name = package.name;
        if let cargo_toml::Publish::Flag(true) = package.publish {
            publishable_package_names.insert(name);
        }
    }
    Ok(publishable_package_names)
}

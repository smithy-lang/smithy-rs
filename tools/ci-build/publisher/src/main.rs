/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use clap::Parser;
use publisher::subcommand::claim_crate_names::{subcommand_claim_crate_names, ClaimCrateNamesArgs};
use publisher::subcommand::fix_manifests::subcommand_fix_manifests;
use publisher::subcommand::fix_manifests::FixManifestsArgs;
use publisher::subcommand::generate_version_manifest::{
    subcommand_generate_version_manifest, GenerateVersionManifestArgs,
};
use publisher::subcommand::hydrate_readme::{subcommand_hydrate_readme, HydrateReadmeArgs};
use publisher::subcommand::publish::subcommand_publish;
use publisher::subcommand::publish::PublishArgs;
use publisher::subcommand::tag_versions_manifest::subcommand_tag_versions_manifest;
use publisher::subcommand::tag_versions_manifest::TagVersionsManifestArgs;
use publisher::subcommand::upgrade_runtime_crates_version::subcommand_upgrade_runtime_crates_version;
use publisher::subcommand::upgrade_runtime_crates_version::UpgradeRuntimeCratesVersionArgs;
use publisher::subcommand::yank_release::{subcommand_yank_release, YankReleaseArgs};
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Parser, Debug)]
#[clap(author, version, about)]
enum Args {
    /// Fixes path dependencies in manifests to also have version numbers
    FixManifests(FixManifestsArgs),
    /// Upgrade the version of the runtime crates used by the code generator (via `gradle.properties`).
    ///
    /// The command will fail if you try to perform a downgrade - e.g. change the version from
    /// `0.53.1` to `0.52.0` or `0.53.0`.
    UpgradeRuntimeCratesVersion(UpgradeRuntimeCratesVersionArgs),
    /// Publishes crates to crates.io
    Publish(PublishArgs),
    /// Publishes an empty library crate to crates.io when a new runtime crate is introduced.
    ///
    /// It must be invoked from the `smithy-rs` repository.
    ClaimCrateNames(ClaimCrateNamesArgs),
    /// Yanks an entire SDK release. For individual packages, use `cargo yank` instead.
    /// Only one of the `--github-release-tag` or `--versions-toml` options are required.
    YankRelease(YankReleaseArgs),
    /// Hydrates the SDK README template file
    HydrateReadme(HydrateReadmeArgs),
    /// Generates a version manifest file for a generated SDK
    GenerateVersionManifest(GenerateVersionManifestArgs),
    /// Adds a release tag to an existing version manifest
    TagVersionsManifest(TagVersionsManifestArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "error,publisher=info".to_owned()),
        )
        .init();

    match Args::parse() {
        Args::ClaimCrateNames(args) => subcommand_claim_crate_names(&args).await?,
        Args::UpgradeRuntimeCratesVersion(args) => {
            subcommand_upgrade_runtime_crates_version(&args).await?
        }
        Args::Publish(args) => subcommand_publish(&args).await?,
        Args::FixManifests(args) => subcommand_fix_manifests(&args).await?,
        Args::YankRelease(args) => subcommand_yank_release(&args).await?,
        Args::HydrateReadme(args) => subcommand_hydrate_readme(&args)?,
        Args::GenerateVersionManifest(args) => subcommand_generate_version_manifest(&args).await?,
        Args::TagVersionsManifest(args) => subcommand_tag_versions_manifest(&args)?,
    }

    Ok(())
}

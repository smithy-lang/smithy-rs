/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::subcommand::fix_manifests::subcommand_fix_manifests;
use crate::subcommand::publish::subcommand_publish;
use crate::subcommand::yank_release::{subcommand_yank_release, YankReleaseArgs};
use anyhow::Result;
use clap::Parser;
use subcommand::fix_manifests::FixManifestsArgs;
use subcommand::generate_version_manifest::{
    subcommand_generate_version_manifest, GenerateVersionManifestArgs,
};
use subcommand::hydrate_readme::{subcommand_hydrate_readme, HydrateReadmeArgs};
use subcommand::publish::PublishArgs;

mod cargo;
mod fs;
mod package;
mod retry;
mod sort;
mod subcommand;

pub const SDK_REPO_CRATE_PATH: &str = "sdk";
pub const SDK_REPO_NAME: &str = "aws-sdk-rust";
pub const SMITHYRS_REPO_NAME: &str = "smithy-rs";

// Crate ownership for SDK crates. Crates.io requires that at least one owner
// is an individual rather than a team, so we use the automation user for that.
pub const CRATE_OWNERS: &[&str] = &[
    // https://github.com/orgs/awslabs/teams/rust-sdk-owners
    "github:awslabs:rust-sdk-owners",
    // https://github.com/aws-sdk-rust-ci
    "aws-sdk-rust-ci",
];

#[derive(Parser, Debug)]
#[clap(author, version, about)]
enum Args {
    /// Fixes path dependencies in manifests to also have version numbers
    FixManifests(FixManifestsArgs),
    /// Publishes crates to crates.io
    Publish(PublishArgs),
    /// Yanks an entire SDK release. For individual packages, use `cargo yank` instead.
    /// Only one of the `--github-release-tag` or `--versions-toml` options are required.
    YankRelease(YankReleaseArgs),
    /// Hydrates the SDK README template file
    HydrateReadme(HydrateReadmeArgs),
    /// Generates a version manifest file for a generated SDK
    GenerateVersionManifest(GenerateVersionManifestArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "error,publisher=info".to_owned()),
        )
        .init();

    match Args::parse() {
        Args::Publish(args) => subcommand_publish(&args).await?,
        Args::FixManifests(args) => subcommand_fix_manifests(&args).await?,
        Args::YankRelease(args) => subcommand_yank_release(&args).await?,
        Args::HydrateReadme(args) => subcommand_hydrate_readme(&args).await?,
        Args::GenerateVersionManifest(args) => subcommand_generate_version_manifest(&args).await?,
    }
    Ok(())
}

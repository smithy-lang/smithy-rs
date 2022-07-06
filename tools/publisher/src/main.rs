/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use clap::Parser;
use publisher::subcommand::fix_manifests::subcommand_fix_manifests;
use publisher::subcommand::fix_manifests::FixManifestsArgs;
use publisher::subcommand::generate_version_manifest::{
    subcommand_generate_version_manifest, GenerateVersionManifestArgs,
};
use publisher::subcommand::hydrate_readme::subcommand_hydrate_readme_v1;
use publisher::subcommand::hydrate_readme::{
    subcommand_hydrate_readme, HydrateReadmeArgs, HydrateReadmeArgsV1,
};
use publisher::subcommand::publish::subcommand_publish;
use publisher::subcommand::publish::PublishArgs;
use publisher::subcommand::yank_release::{subcommand_yank_release, YankReleaseArgs};

// TODO(https://github.com/awslabs/smithy-rs/issues/1531): Remove V1 args
#[derive(Parser, Debug)]
#[clap(author, version, about)]
enum ArgsV1 {
    /// Fixes path dependencies in manifests to also have version numbers
    FixManifests(FixManifestsArgs),
    /// Publishes crates to crates.io
    Publish(PublishArgs),
    /// Yanks an entire SDK release. For individual packages, use `cargo yank` instead.
    /// Only one of the `--github-release-tag` or `--versions-toml` options are required.
    YankRelease(YankReleaseArgs),
    /// Hydrates the SDK README template file
    HydrateReadme(HydrateReadmeArgsV1),
    /// Generates a version manifest file for a generated SDK
    GenerateVersionManifest(GenerateVersionManifestArgs),
}

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

    if let Ok(args) = Args::try_parse() {
        match args {
            Args::Publish(args) => subcommand_publish(&args).await?,
            Args::FixManifests(args) => subcommand_fix_manifests(&args).await?,
            Args::YankRelease(args) => subcommand_yank_release(&args).await?,
            Args::HydrateReadme(args) => subcommand_hydrate_readme(&args)?,
            Args::GenerateVersionManifest(args) => {
                subcommand_generate_version_manifest(&args).await?
            }
        }
    } else {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1531): Remove V1 args
        println!("Failed to match new arg format. Trying to parse the old arg format.");
        let working_dir = std::env::current_dir()?;
        match ArgsV1::parse() {
            ArgsV1::Publish(args) => subcommand_publish(&args).await?,
            ArgsV1::FixManifests(args) => subcommand_fix_manifests(&args).await?,
            ArgsV1::YankRelease(args) => subcommand_yank_release(&args).await?,
            ArgsV1::HydrateReadme(args) => subcommand_hydrate_readme_v1(&args, &working_dir)?,
            ArgsV1::GenerateVersionManifest(args) => {
                subcommand_generate_version_manifest(&args).await?
            }
        }
    }

    Ok(())
}

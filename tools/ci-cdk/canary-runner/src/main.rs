/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use tracing_subscriber::{filter::EnvFilter, prelude::*};

mod build_bundle;
mod generate_matrix;
mod run;

#[derive(Debug, Parser, Eq, PartialEq)]
#[clap(version, about)]
pub(crate) enum Args {
    /// Builds the canary Lambda bundle
    #[clap(alias = "build-bundle")]
    BuildBundle(build_bundle::BuildBundleArgs),

    /// Generates a GitHub Actions test matrix for the canary
    #[clap(alias = "generate-matrix")]
    GenerateMatrix(generate_matrix::GenerateMatrixArgs),

    /// Builds, uploads, and invokes the canary as a Lambda
    #[clap(alias = "run")]
    Run(run::RunArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("warn,canary_runner=info"))
        .unwrap();
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let opt = Args::parse();
    match opt {
        Args::BuildBundle(subopt) => build_bundle::build_bundle(subopt).await.map(|_| ()),
        Args::GenerateMatrix(subopt) => generate_matrix::generate_matrix(subopt).await,
        Args::Run(subopt) => run::run(subopt).await,
    }
}

// Tests for individual subcommands can be found in their respective modules

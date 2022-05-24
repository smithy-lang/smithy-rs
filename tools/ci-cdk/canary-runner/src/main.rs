/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use tracing_subscriber::{filter::EnvFilter, prelude::*};

mod generate_matrix;
mod run;

#[derive(Debug, Parser)]
#[clap(version, about)]
enum Opt {
    /// Generates a GitHub Actions test matrix for the canary
    #[clap(alias = "generate-matrix")]
    GenerateMatrix(generate_matrix::GenerateMatrixOpt),

    /// Builds, uploads, and invokes the canary as a Lambda
    #[clap(alias = "run")]
    Run(run::RunOpt),
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

    let opt = Opt::parse();
    match opt {
        Opt::GenerateMatrix(subopt) => generate_matrix::generate_matrix(subopt).await,
        Opt::Run(subopt) => run::run(subopt).await,
    }
}

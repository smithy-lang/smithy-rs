/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

mod audit;

#[derive(clap::Args, Clone)]
pub struct AuditArgs {
    /// Path to smithy-rs. Defaults to current working directory.
    #[arg(long)]
    smithy_rs_path: Option<PathBuf>,
}

#[derive(clap::Parser, Clone)]
#[clap(author, version, about)]
enum Command {
    Audit(AuditArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let command = Command::parse();
    match command {
        Command::Audit(args) => audit::audit(args),
    }
}

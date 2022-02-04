/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::error::ErrorPrinter;
use crate::visitor::Visitor;
use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::{OwoColorize, Stream};
use rustdoc_types::FORMAT_VERSION;
use smithy_rs_tool_common::macros::here;
use smithy_rs_tool_common::shell::ShellOperation;
use std::fs;
use std::path::PathBuf;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod cargo;
mod config;
mod context;
mod error;
mod visitor;

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Path to the crate to examine
    #[clap(long)]
    crate_path: PathBuf,
    /// Expected `target/` directory for that crate
    #[clap(long)]
    target_path: PathBuf,
    /// Path to config toml to read
    #[clap(long)]
    config: Option<PathBuf>,
    /// Enable verbose output for debugging
    #[clap(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.verbose {
        let filter_layer = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("debug"))
            .unwrap();
        let fmt_layer = tracing_subscriber::fmt::layer()
            .without_time()
            .with_ansi(true)
            .with_level(true)
            .with_target(false)
            .pretty();
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .init();
    }

    let config = if let Some(config_path) = &args.config {
        let contents = fs::read_to_string(config_path).context("failed to read config file")?;
        toml::from_str(&contents).context("failed to parse config file")?
    } else {
        Default::default()
    };

    println!(
        "This build understands rustdoc format version {}",
        FORMAT_VERSION
    );
    println!("Running rustdoc to produce json doc output...");
    let package = cargo::CargoRustDocJson::new(&args.crate_path, args.target_path)
        .run()
        .context(here!())?;

    println!("Examining all public types...");
    let errors = Visitor::new(config, package)?.visit_all()?;
    let mut error_printer = ErrorPrinter::new(&args.crate_path);
    for error in &errors {
        println!("{}", error);
        if let Some(location) = error.location() {
            error_printer
                .pretty_print_error_context(location, error.subtext())
                .context("failed to output error context")?;
        }
    }
    if !errors.is_empty() {
        println!(
            "{} {} emitted",
            errors.len(),
            "errors".if_supports_color(Stream::Stdout, |text| text.red())
        );
    }

    Ok(())
}

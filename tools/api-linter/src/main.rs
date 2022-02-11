/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::error::ErrorPrinter;
use crate::visitor::Visitor;
use anyhow::{anyhow, bail};
use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::{OwoColorize, Stream};
use rustdoc_types::FORMAT_VERSION;
use smithy_rs_tool_common::macros::here;
use smithy_rs_tool_common::shell::ShellOperation;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod cargo;
mod config;
mod context;
mod error;
mod visitor;

#[derive(Debug)]
enum OutputFormat {
    Errors,
    MarkdownTable,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Errors => "errors",
            Self::MarkdownTable => "markdown-table",
        })
    }
}

impl FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "errors" => Ok(OutputFormat::Errors),
            "markdown-table" => Ok(OutputFormat::MarkdownTable),
            _ => Err(anyhow!("invalid output format: {}", s)),
        }
    }
}

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
    /// Nightly version of Rustdoc to use. By default, `+nightly` will be used.
    /// This argument can be used to pin to specific nightly version (i.e., `+nightly-2022-02-08`).
    #[clap(long)]
    nightly_version: Option<String>,
    /// Format to output results in
    #[clap(long, default_value_t = OutputFormat::Errors)]
    output_format: OutputFormat,
}

impl Args {
    fn validate(&self) -> Result<()> {
        if let Some(version) = &self.nightly_version {
            if !version.starts_with("+nightly") {
                bail!("Nightly version must start with `+nightly`");
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    args.validate()?;
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

    eprintln!(
        "This build understands rustdoc format version {}",
        FORMAT_VERSION
    );
    eprintln!("Running rustdoc to produce json doc output...");
    let package =
        cargo::CargoRustDocJson::new(&args.crate_path, args.target_path, args.nightly_version)
            .run()
            .context(here!())?;

    eprintln!("Examining all public types...");
    let errors = Visitor::new(config, package)?.visit_all()?;
    match args.output_format {
        OutputFormat::Errors => {
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
        }
        OutputFormat::MarkdownTable => {
            println!("| Crate | Type | Used In |");
            println!("| ---   | ---   | ---     |");
            let mut rows = Vec::new();
            for error in &errors {
                let type_name = error.type_name();
                let crate_name = &type_name[0..type_name.find("::").unwrap_or(type_name.len())];
                let location = error.location().unwrap();
                rows.push(format!(
                    "| {} | {} | {}:{}:{} |",
                    crate_name,
                    type_name,
                    location.filename.to_string_lossy(),
                    location.begin.0,
                    location.begin.1
                ));
            }
            rows.sort();
            rows.into_iter().for_each(|row| println!("{}", row));
        }
    }

    Ok(())
}

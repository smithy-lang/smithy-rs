/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{anyhow, bail};
use anyhow::{Context, Result};
use cargo_check_external_types::cargo::CargoRustDocJson;
use cargo_check_external_types::error::ErrorPrinter;
use cargo_check_external_types::here;
use cargo_check_external_types::visitor::Visitor;
use cargo_metadata::{CargoOpt, Metadata};
use clap::Parser;
use owo_colors::{OwoColorize, Stream};
use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

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
            _ => Err(anyhow!(
                "invalid output format: {}. Expected `errors` or `markdown-table`.",
                s
            )),
        }
    }
}

#[derive(clap::Args, Debug)]
struct CheckExternalTypesArgs {
    /// Enables all crate features
    #[clap(long)]
    all_features: bool,
    /// Disables default features
    #[clap(long)]
    no_default_features: bool,
    /// Comma delimited list of features to enable in the crate
    #[clap(long, use_value_delimiter = true)]
    features: Option<Vec<String>>,
    /// Path to the Cargo manifest
    manifest_path: Option<PathBuf>,

    /// Path to config toml to read
    #[clap(long)]
    config: Option<PathBuf>,
    /// Enable verbose output for debugging
    #[clap(short, long)]
    verbose: bool,
    /// Format to output results in
    #[clap(long, default_value_t = OutputFormat::Errors)]
    output_format: OutputFormat,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, bin_name = "cargo")]
enum Args {
    CheckExternalTypes(CheckExternalTypesArgs),
}

enum Error {
    ValidationErrors,
    Failure(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Failure(err)
    }
}

fn main() {
    process::exit(match run_main() {
        Ok(_) => 0,
        Err(Error::ValidationErrors) => 1,
        Err(Error::Failure(err)) => {
            println!("{:#}", dbg!(err));
            2
        }
    })
}

fn run_main() -> Result<(), Error> {
    let Args::CheckExternalTypes(args) = Args::parse();
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

    let mut cargo_metadata_cmd = cargo_metadata::MetadataCommand::new();
    if args.all_features {
        cargo_metadata_cmd.features(CargoOpt::AllFeatures);
    }
    if args.no_default_features {
        cargo_metadata_cmd.features(CargoOpt::NoDefaultFeatures);
    }
    if let Some(features) = args.features {
        cargo_metadata_cmd.features(CargoOpt::SomeFeatures(features));
    }
    let crate_path = if let Some(manifest_path) = args.manifest_path {
        cargo_metadata_cmd.manifest_path(&manifest_path);
        manifest_path
            .canonicalize()
            .context(here!())?
            .parent()
            .expect("parent path")
            .to_path_buf()
    } else {
        std::env::current_dir()
            .context(here!())?
            .canonicalize()
            .context(here!())?
    };
    let cargo_metadata = cargo_metadata_cmd.exec().context(here!())?;
    let cargo_features = resolve_features(&cargo_metadata)?;

    eprintln!("Running rustdoc to produce json doc output...");
    let package = CargoRustDocJson::new(
        &*cargo_metadata
            .root_package()
            .as_ref()
            .map(|package| Cow::Borrowed(package.name.as_str()))
            .unwrap_or_else(|| crate_path.file_name().expect("file name").to_string_lossy()),
        &crate_path,
        &cargo_metadata.target_directory,
        cargo_features,
    )
    .run()
    .context(here!())?;

    eprintln!("Examining all public types...");
    let errors = Visitor::new(config, package)?.visit_all()?;
    match args.output_format {
        OutputFormat::Errors => {
            let mut error_printer = ErrorPrinter::new(&cargo_metadata.workspace_root);
            for error in &errors {
                println!("{}", error);
                if let Some(location) = error.location() {
                    error_printer.pretty_print_error_context(location, error.subtext())
                }
            }
            if !errors.is_empty() {
                println!(
                    "{} {} emitted",
                    errors.len(),
                    "errors".if_supports_color(Stream::Stdout, |text| text.red())
                );
                return Err(Error::ValidationErrors);
            }
        }
        OutputFormat::MarkdownTable => {
            println!("| Crate | Type | Used In |");
            println!("| ---   | ---  | ---     |");
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

fn resolve_features(metadata: &Metadata) -> Result<Vec<String>> {
    let root_package = metadata
        .root_package()
        .ok_or_else(|| anyhow!("No root package found"))?;
    if let Some(resolve) = &metadata.resolve {
        let root_node = resolve
            .nodes
            .iter()
            .find(|&n| n.id == root_package.id)
            .ok_or_else(|| anyhow!("Failed to find node for root package"))?;
        Ok(root_node.features.clone())
    } else {
        bail!("Cargo metadata didn't have resolved nodes");
    }
}

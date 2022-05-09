/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Context, Result};
use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;
use smithy_rs_tool_common::macros::here;
use smithy_rs_tool_common::shell::{handle_failure, ShellOperation};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize)]
struct CrateFormatVersion {
    format_version: u32,
}

/// Runs the `cargo rustdoc` command required to produce Rustdoc's JSON output with a nightly compiler.
pub struct CargoRustDocJson {
    /// Path of the crate to examine
    crate_path: PathBuf,
    /// Expected `target/` directory where the output will be
    target_path: PathBuf,
    /// Features to enable
    features: Vec<String>,
}

impl CargoRustDocJson {
    pub fn new(
        crate_path: impl Into<PathBuf>,
        target_path: impl Into<PathBuf>,
        features: Vec<String>,
    ) -> Self {
        CargoRustDocJson {
            crate_path: crate_path.into(),
            target_path: target_path.into(),
            features,
        }
    }
}

impl ShellOperation for CargoRustDocJson {
    type Output = Crate;

    fn run(&self) -> Result<Crate> {
        let cargo = std::env::var("CARGO")
            .ok()
            .unwrap_or_else(|| "cargo".to_string());
        let crate_path = self.crate_path.canonicalize().context(here!())?;

        let mut command = Command::new(&cargo);
        command.current_dir(&self.crate_path).arg("rustdoc");
        if !self.features.is_empty() {
            command.arg("--no-default-features").arg("--features");
            command.arg(&self.features.join(","));
        }
        command
            .arg("--")
            .arg("--document-private-items")
            .arg("-Z")
            .arg("unstable-options")
            .arg("--output-format")
            .arg("json");
        let output = command
            .output()
            .context(here!("failed to run nightly rustdoc"))?;
        handle_failure("rustdoc", &output)?;

        let crate_name = crate_path.file_name().expect("file name").to_string_lossy();
        let output_file_name = self
            .target_path
            .canonicalize()
            .context(here!())?
            .join(format!("doc/{}.json", crate_name.replace('-', "_")));

        let json = fs::read_to_string(output_file_name).context(here!())?;
        let format_version: CrateFormatVersion = serde_json::from_str(&json)
            .context("Failed to find `format_version` in rustdoc JSON output.")
            .context(here!())?;
        if format_version.format_version != FORMAT_VERSION {
            bail!(
                "The version of rustdoc being used produces JSON format version {0}, but \
                this tool requires format version {1}. This can happen if the locally \
                installed version of rustdoc doesn't match the rustdoc JSON types from \
                the `rustdoc-types` crate.\n\n\
                If this occurs with the latest Rust nightly and the latest version of this \
                tool, then this is a bug, and the tool needs to be upgraded to the latest \
                format version.\n\n\
                Otherwise, you'll need to determine a Rust nightly version that matches \
                this tool's supported format version (or vice versa).",
                format_version.format_version,
                FORMAT_VERSION
            );
        }
        let package: Crate = serde_json::from_str(&json)
            .context("Failed to parse rustdoc output.")
            .context(here!())?;
        Ok(package)
    }
}

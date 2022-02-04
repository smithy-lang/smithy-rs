/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::{Context, Result};
use rustdoc_types::Crate;
use smithy_rs_tool_common::macros::here;
use smithy_rs_tool_common::shell::{handle_failure, ShellOperation};
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

pub struct CargoRustDocJson {
    program: &'static str,
    /// Path of the crate to examine
    crate_path: PathBuf,
    /// Expected `target/` directory where the output will be
    target_path: PathBuf,
}

impl CargoRustDocJson {
    pub fn new(crate_path: impl Into<PathBuf>, target_path: impl Into<PathBuf>) -> Self {
        CargoRustDocJson {
            program: "cargo",
            crate_path: crate_path.into(),
            target_path: target_path.into(),
        }
    }
}

impl ShellOperation for CargoRustDocJson {
    type Output = Crate;

    fn run(&self) -> Result<Crate> {
        let crate_path = self.crate_path.canonicalize().context(here!())?;

        let mut command = Command::new(self.program);
        command
            .current_dir(&self.crate_path)
            .arg("+nightly")
            .arg("rustdoc")
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

        let file = fs::File::open(output_file_name).context(here!())?;
        let reader = BufReader::new(file);
        let package: Crate = serde_json::from_reader(reader)
            .context(
                "Failed to parse rustdoc output. This can happen if the locally installed \
                version of rustdoc doesn't match the rustdoc JSON types from the `rustdoc-types` \
                crate. Try updating your nightly compiler as well as that crate to resolve.",
            )
            .context(here!())?;
        Ok(package)
    }
}

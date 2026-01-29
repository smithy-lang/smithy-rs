/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use smithy_rs_tool_common::shell::{capture_error, output_text, ShellOperation};
use std::process::Command;
use tracing::info;

/// Yanks a package version from crates.io
pub struct Yank {
    program: &'static str,
    crate_name: String,
    crate_version: String,
}

impl Yank {
    pub fn new(crate_name: impl Into<String>, crate_version: impl Into<String>) -> Yank {
        Yank {
            program: "cargo",
            crate_name: crate_name.into(),
            crate_version: crate_version.into(),
        }
    }
}

impl ShellOperation for Yank {
    type Output = ();

    fn run(&self) -> Result<()> {
        let mut command = Command::new(self.program);
        command
            .arg("yank")
            .arg("--vers")
            .arg(&self.crate_version)
            .arg(&self.crate_name);
        let output = command.output()?;
        if !output.status.success() {
            let (_, stderr) = output_text(&output);
            let no_such_version = format!(
                "error: crate `{}` does not have a version `{}`",
                self.crate_name, self.crate_version
            );
            if stderr.contains(&no_such_version) {
                info!(
                    "{} never had a version {}.",
                    self.crate_name, self.crate_version
                );
            } else {
                return Err(capture_error("cargo yank", &output));
            }
        }
        Ok(())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn yank_succeeds() {
        Yank {
            program: "./fake_cargo/cargo_success",
            crate_name: "aws-sdk-dynamodb".into(),
            crate_version: "0.0.22-alpha".into(),
        }
        .spawn()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn yank_fails() {
        let result = Yank {
            program: "./fake_cargo/cargo_fails",
            crate_name: "something".into(),
            crate_version: "0.0.22-alpha".into(),
        }
        .spawn()
        .await;
        assert!(result.is_err(), "expected error, got {result:?}");
        assert_eq!(
            "Failed to cargo yank:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }

    #[tokio::test]
    async fn yank_no_such_version() {
        Yank {
            program: "./fake_cargo/cargo_yank_not_found",
            crate_name: "aws-sigv4".into(),
            crate_version: "0.0.0".into(),
        }
        .spawn()
        .await
        .unwrap();
    }
}

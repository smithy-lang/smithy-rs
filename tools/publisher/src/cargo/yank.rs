/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::package::PackageHandle;
use anyhow::Result;
use smithy_rs_tool_common::shell::{capture_error, output_text, ShellOperation};
use std::path::PathBuf;
use std::process::Command;
use tracing::info;

/// Yanks a package version from crates.io
pub struct Yank {
    program: &'static str,
    package_handle: PackageHandle,
    package_path: PathBuf,
}

impl Yank {
    pub fn new(package_handle: PackageHandle, package_path: impl Into<PathBuf>) -> Yank {
        Yank {
            program: "cargo",
            package_handle,
            package_path: package_path.into(),
        }
    }
}

impl ShellOperation for Yank {
    type Output = ();

    fn run(&self) -> Result<()> {
        let mut command = Command::new(self.program);
        command
            .current_dir(&self.package_path)
            .arg("yank")
            .arg("--vers")
            .arg(format!("{}", self.package_handle.version))
            .arg(&self.package_handle.name);
        let output = command.output()?;
        if !output.status.success() {
            let (_, stderr) = output_text(&output);
            let no_such_version = format!(
                "error: crate `{}` does not have a version `{}`",
                self.package_handle.name, self.package_handle.version
            );
            if stderr.contains(&no_such_version) {
                info!(
                    "{} never had a version {}.",
                    self.package_handle.name, self.package_handle.version
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
    use semver::Version;
    use std::env;

    #[tokio::test]
    async fn yank_succeeds() {
        Yank {
            program: "./fake_cargo/cargo_success",
            package_handle: PackageHandle::new(
                "aws-sdk-dynamodb",
                Version::parse("0.0.22-alpha").unwrap(),
            ),
            package_path: env::current_dir().unwrap().into(),
        }
        .spawn()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn yank_fails() {
        let result = Yank {
            program: "./fake_cargo/cargo_fails",
            package_handle: PackageHandle::new(
                "something",
                Version::parse("0.0.22-alpha").unwrap(),
            ),
            package_path: env::current_dir().unwrap().into(),
        }
        .spawn()
        .await;
        assert!(result.is_err(), "expected error, got {:?}", result);
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
            package_handle: PackageHandle::new("aws-sigv4", Version::parse("0.0.0").unwrap()),
            package_path: env::current_dir().unwrap().into(),
        }
        .spawn()
        .await
        .unwrap();
    }
}

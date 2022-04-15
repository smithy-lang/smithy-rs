/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::git::CommitHash;
use anyhow::Result;
use smithy_rs_tool_common::shell::handle_failure;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg_attr(test, mockall::automock)]
pub trait Gradle {
    /// Runs the `aws:sdk:clean` target
    fn aws_sdk_clean(&self) -> Result<()>;

    /// Runs `aws:sdk:assemble` target with property `aws.fullsdk=true` set
    fn aws_sdk_assemble(&self, examples_revision: &CommitHash) -> Result<()>;
}

pub struct GradleCLI {
    path: PathBuf,
    binary_name: String,
}

impl GradleCLI {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.into(),
            binary_name: "./gradlew".into(),
        }
    }

    #[cfg(test)]
    pub fn with_binary(path: &Path, name: &str) -> Self {
        Self {
            path: path.into(),
            binary_name: name.into(),
        }
    }
}

impl Gradle for GradleCLI {
    fn aws_sdk_clean(&self) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("aws:sdk:clean");
        command.current_dir(&self.path);

        let output = command.output()?;
        handle_failure("aws_sdk_clean", &output)?;
        Ok(())
    }

    fn aws_sdk_assemble(&self, examples_revision: &CommitHash) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("-Paws.fullsdk=true");
        command.arg(format!("-Paws.sdk.examples.revision={}", examples_revision));
        command.arg("aws:sdk:assemble");
        command.current_dir(&self.path);

        let output = command.output()?;
        handle_failure("aws_sdk_assemble", &output)?;
        Ok(())
    }
}

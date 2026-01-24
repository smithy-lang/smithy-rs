/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use smithy_rs_tool_common::shell::{handle_failure, ShellOperation};
use std::process::Command;

pub struct RemoveOwner {
    program: &'static str,
    package_name: String,
    owner: String,
}

impl RemoveOwner {
    pub fn new(package_name: impl Into<String>, owner: impl Into<String>) -> RemoveOwner {
        RemoveOwner {
            program: "cargo",
            package_name: package_name.into(),
            owner: owner.into(),
        }
    }
}

impl ShellOperation for RemoveOwner {
    type Output = ();

    fn run(&self) -> Result<()> {
        let mut command = Command::new(self.program);
        command
            .arg("owner")
            .arg("--remove")
            .arg(&self.owner)
            .arg(&self.package_name);
        let output = command.output()?;
        handle_failure("remove owner", &output)?;
        Ok(())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    use smithy_rs_tool_common::shell::ShellOperation;

    #[tokio::test]
    async fn remove_owner_success() {
        RemoveOwner {
            program: "./fake_cargo/cargo_success",
            package_name: "aws-sdk-s3".into(),
            owner: "incorrect_owner".into(),
        }
        .spawn()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn remove_owner_failed() {
        let result = RemoveOwner {
            program: "./fake_cargo/cargo_fails",
            package_name: "aws-sdk-s3".into(),
            owner: "incorrect_owner".into(),
        }
        .spawn()
        .await;

        assert!(result.is_err(), "expected error, got {result:?}");
        assert_eq!(
            "Failed to remove owner:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

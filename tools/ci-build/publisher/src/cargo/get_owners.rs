/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use regex_lite::Regex;
use smithy_rs_tool_common::shell::{handle_failure, output_text, ShellOperation};
use std::process::Command;
use std::sync::LazyLock;

static LINE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([\w\d\-_:]+)\s+\([\w\d\s\-_]+\)$").unwrap());

pub struct GetOwners {
    program: &'static str,
    package_name: String,
}

impl GetOwners {
    pub fn new(package_name: impl Into<String>) -> GetOwners {
        GetOwners {
            program: "cargo",
            package_name: package_name.into(),
        }
    }
}

impl ShellOperation for GetOwners {
    type Output = Vec<String>;

    fn run(&self) -> Result<Vec<String>> {
        let mut command = Command::new(self.program);
        command.arg("owner").arg("--list").arg(&self.package_name);
        let output = command.output()?;
        handle_failure("get crate owners", &output)?;

        let mut result = Vec::new();
        let (stdout, _) = output_text(&output);
        for line in stdout.lines() {
            if let Some(captures) = LINE_REGEX.captures(line) {
                let user_id = captures.get(1).unwrap().as_str();
                result.push(user_id.to_string());
            } else {
                return Err(anyhow::Error::msg(format!(
                    "unrecognized line in `cargo owner` output: {line}"
                )));
            }
        }
        Ok(result)
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_owners_success() {
        let owners = GetOwners {
            program: "./fake_cargo/cargo_owner_list",
            package_name: "aws-sdk-s3".into(),
        }
        .spawn()
        .await
        .unwrap();
        assert_eq!(
            vec![
                "rcoh".to_string(),
                "github:awslabs:rust-sdk-owners".to_string()
            ],
            owners
        );
    }

    #[tokio::test]
    async fn get_owners_failed() {
        let result = GetOwners {
            program: "./fake_cargo/cargo_fails",
            package_name: "aws-sdk-s3".into(),
        }
        .spawn()
        .await;

        assert!(result.is_err(), "expected error, got {result:?}");
        assert_eq!(
            "Failed to get crate owners:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

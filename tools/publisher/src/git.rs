/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::shell::{handle_failure, output_text, ShellOperation};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;

pub struct GetCurrentTag<'a> {
    program: &'static str,
    path: &'a Path,
}

impl<'a> GetCurrentTag<'a> {
    pub fn new(path: &'a Path) -> GetCurrentTag<'a> {
        GetCurrentTag {
            program: "git",
            path,
        }
    }
}

#[async_trait]
impl<'a> ShellOperation for GetCurrentTag<'a> {
    type Output = String;

    async fn spawn(&self) -> Result<String> {
        let mut command = Command::new(self.program);
        command.arg("describe");
        command.arg("--tags");
        command.current_dir(self.path);
        let output = tokio::task::spawn_blocking(move || command.output()).await??;
        handle_failure("get current tag", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_current_tag_success() {
        let tag = GetCurrentTag {
            program: "./git_describe_tags",
            path: "./fake_git".as_ref(),
        }
        .spawn()
        .await
        .unwrap();
        assert_eq!("some-tag", tag);
    }

    #[tokio::test]
    async fn get_current_tag_failure() {
        let result = GetCurrentTag {
            program: "./git_fails",
            path: "./fake_git".as_ref(),
        }
        .spawn()
        .await;

        assert!(result.is_err(), "expected error, got {:?}", result);
        assert_eq!(
            "Failed to get current tag:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

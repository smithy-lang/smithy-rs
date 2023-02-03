/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shell::{handle_failure, output_text, ShellOperation};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct GetCurrentTag {
    program: &'static str,
    path: PathBuf,
}

impl GetCurrentTag {
    pub fn new(path: impl Into<PathBuf>) -> GetCurrentTag {
        GetCurrentTag {
            program: "git",
            path: path.into(),
        }
    }
}

impl ShellOperation for GetCurrentTag {
    type Output = String;

    fn run(&self) -> Result<String> {
        let mut command = Command::new(self.program);
        command.arg("describe");
        command.arg("--tags");
        command.current_dir(&self.path);

        let output = command.output()?;
        handle_failure("get current tag", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn get_current_tag_success() {
        let tag = GetCurrentTag {
            program: "./git_describe_tags",
            path: "./fake_git".into(),
        }
        .run()
        .unwrap();
        assert_eq!("some-tag", tag);
    }

    #[cfg(feature = "async-shell")]
    #[tokio::test]
    async fn get_current_tag_success_async() {
        let tag = GetCurrentTag {
            program: "./git_describe_tags",
            path: "./fake_git".into(),
        }
        .spawn()
        .await
        .unwrap();
        assert_eq!("some-tag", tag);
    }

    #[test]
    fn get_current_tag_failure() {
        let result = GetCurrentTag {
            program: "./git_fails",
            path: "./fake_git".into(),
        }
        .run();

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

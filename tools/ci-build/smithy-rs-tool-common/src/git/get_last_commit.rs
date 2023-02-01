/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shell::{handle_failure, output_text, ShellOperation};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct GetLastCommit {
    program: &'static str,
    repo_path: PathBuf,
}

impl GetLastCommit {
    pub fn new(repo_path: impl Into<PathBuf>) -> GetLastCommit {
        GetLastCommit {
            program: "git",
            repo_path: repo_path.into(),
        }
    }
}

impl ShellOperation for GetLastCommit {
    type Output = String;

    fn run(&self) -> Result<String> {
        let mut command = Command::new(self.program);
        command.arg("rev-parse");
        command.arg("HEAD");
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("get last commit", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn get_last_commit_success() {
        let last_commit = GetLastCommit {
            program: "./git_revparse_head",
            repo_path: "./fake_git".into(),
        }
        .run()
        .unwrap();
        assert_eq!("commithash", last_commit);
    }

    #[test]
    fn get_last_commit_failure() {
        let result = GetLastCommit {
            program: "./git_fails",
            repo_path: "./fake_git".into(),
        }
        .run();

        assert!(result.is_err(), "expected error, got {:?}", result);
        assert_eq!(
            "Failed to get last commit:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

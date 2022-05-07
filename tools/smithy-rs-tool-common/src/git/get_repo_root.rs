/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shell::{handle_failure, output_text, ShellOperation};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct GetRepoRoot {
    program: &'static str,
    start_path: PathBuf,
}

impl GetRepoRoot {
    pub fn new(start_path: impl Into<PathBuf>) -> GetRepoRoot {
        GetRepoRoot {
            program: "git",
            start_path: start_path.into(),
        }
    }
}

impl ShellOperation for GetRepoRoot {
    type Output = PathBuf;

    fn run(&self) -> Result<PathBuf> {
        let mut command = Command::new(self.program);
        command.arg("rev-parse");
        command.arg("--show-toplevel");
        command.current_dir(&self.start_path);

        let output = command.output()?;
        handle_failure("determine git repo root", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn get_repo_root_success() {
        let last_commit = GetRepoRoot {
            program: "./git_revparse_show_toplevel",
            start_path: "./fake_git".into(),
        }
        .run()
        .unwrap();
        assert_eq!(PathBuf::from("/git/repo/root/path"), last_commit);
    }

    #[test]
    fn get_repo_root_failure() {
        let result = GetRepoRoot {
            program: "./git_fails",
            start_path: "./fake_git".into(),
        }
        .run();

        assert!(result.is_err(), "expected error, got {:?}", result);
        assert_eq!(
            "Failed to determine git repo root:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

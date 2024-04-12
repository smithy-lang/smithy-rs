/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::shell::{handle_failure, ShellOperation};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct Reset {
    program: &'static str,
    path: PathBuf,
    args: Vec<String>,
}

impl Reset {
    pub fn new(path: impl Into<PathBuf>, args: &[&str]) -> Self {
        Reset {
            program: "git",
            path: path.into(),
            args: args.iter().map(|&s| s.to_string()).collect(),
        }
    }
}

impl ShellOperation for Reset {
    type Output = ();

    fn run(&self) -> Result<()> {
        let mut command = Command::new(self.program);
        command.arg("reset");
        for arg in &self.args {
            command.arg(arg);
        }
        command.current_dir(&self.path);

        let output = command.output()?;
        handle_failure("git reset", &output)?;
        Ok(())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn reset_success() {
        Reset {
            program: "./git_reset",
            path: "./fake_git".into(),
            args: vec!["--hard".to_string(), "some-commit-hash".to_string()],
        }
        .run()
        .unwrap();
    }

    #[test]
    fn reset_failure() {
        let result = Reset {
            program: "./git_fails",
            path: "./fake_git".into(),
            args: vec!["--hard".to_string(), "some-commit-hash".to_string()],
        }
        .run();

        assert!(result.is_err(), "expected error, got {:?}", result);
        assert_eq!(
            "Failed to git reset:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

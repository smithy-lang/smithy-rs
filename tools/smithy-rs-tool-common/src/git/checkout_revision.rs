/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::shell::{handle_failure, ShellOperation};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct CheckoutRevision {
    program: &'static str,
    path: PathBuf,
    revision: String,
}

impl CheckoutRevision {
    pub fn new(path: impl Into<PathBuf>, revision: impl Into<String>) -> Self {
        CheckoutRevision {
            program: "git",
            path: path.into(),
            revision: revision.into(),
        }
    }
}

impl ShellOperation for CheckoutRevision {
    type Output = ();

    fn run(&self) -> Result<()> {
        let mut command = Command::new(self.program);
        command.arg("checkout");
        command.arg(&self.revision);
        command.current_dir(&self.path);

        let output = command.output()?;
        handle_failure("checkout revision", &output)?;
        Ok(())
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn checkout_revision_success() {
        CheckoutRevision {
            program: "./git_checkout_revision",
            path: "./fake_git".into(),
            revision: "test-revision".into(),
        }
        .run()
        .unwrap();
    }

    #[test]
    fn checkout_revision_failure() {
        let result = CheckoutRevision {
            program: "./git_fails",
            path: "./fake_git".into(),
            revision: "test-revision".into(),
        }
        .run();

        assert!(result.is_err(), "expected error, got {:?}", result);
        assert_eq!(
            "Failed to checkout revision:\n\
            Status: 1\n\
            Stdout: some stdout failure message\n\n\
            Stderr: some stderr failure message\n\n",
            format!("{}", result.err().unwrap())
        );
    }
}

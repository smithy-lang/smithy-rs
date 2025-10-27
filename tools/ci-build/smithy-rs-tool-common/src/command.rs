/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities to simplify working with [`std::command::Command`].

/// Synchronous command extension functions.
pub mod sync {
    use anyhow::{bail, Context, Result};
    use std::process::Command;

    /// Extension trait to make `Command` nicer to work with.
    pub trait CommandExt {
        /// Expects the command to exit with a successful status, and returns the command's stdout.
        ///
        /// If the command fails, the `context` is used to help describe the problem.
        /// It will always be formatted with "failed to {context}".
        fn expect_success_output(&mut self, context: &str) -> Result<String>;

        /// Expects the command to exit with one of the given statuses.
        ///
        /// If the command fails, the `context` is used to help describe the problem.
        /// It will always be formatted with "failed to {context}".
        fn expect_status_one_of(
            &mut self,
            context: &str,
            statuses: impl IntoIterator<Item = i32>,
        ) -> Result<i32>;
    }

    impl CommandExt for Command {
        fn expect_success_output(&mut self, context: &str) -> Result<String> {
            let output = self
                .output()
                .with_context(|| format!("failed to invoke {self:?}"))?;
            let stdout = String::from_utf8(output.stdout)
                .with_context(|| format!("command: {self:?}"))
                .context("output had invalid utf-8")?;
            if !output.status.success() {
                bail!(
                    "failed to {context}\n\ncommand: {:?}\n\nstdout:\n{}\n\nstderr:\n{}\n",
                    self,
                    stdout,
                    String::from_utf8_lossy(&output.stderr),
                );
            }
            Ok(stdout)
        }

        fn expect_status_one_of(
            &mut self,
            context: &str,
            statuses: impl IntoIterator<Item = i32>,
        ) -> Result<i32> {
            let output = self
                .output()
                .with_context(|| format!("failed to invoke {self:?}"))?;
            let expected: Vec<_> = statuses.into_iter().collect();
            let actual = output.status.code().unwrap();
            if expected.contains(&actual) {
                Ok(actual)
            } else {
                bail!(
                    "failed to {context}\n\n\
                    expected exit status to be one of {expected:?}, but got {actual}\n\n\
                    command: {:?}\n\nstdout:\n{}\n\nstderr:\n{}\n",
                    self,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                )
            }
        }
    }
}

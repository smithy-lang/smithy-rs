/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;
use std::time::Duration;

pub(crate) struct NewArgs {}

fn step<T>(message: &'static str, step: impl FnOnce() -> Result<T>) -> Result<T> {
    let spinner = ProgressBar::new_spinner()
        .with_message(message)
        .with_style(ProgressStyle::with_template("{spinner} {msg} {elapsed}").unwrap());
    spinner.enable_steady_tick(Duration::from_millis(100));
    let result = step();
    let check = match &result {
        Ok(_) => "✅",
        Err(_) => "❌",
    };
    spinner.set_style(ProgressStyle::with_template("{msg} {elapsed}").unwrap());
    spinner.finish_with_message(format!("{check} {message}"));
    result
}

pub(crate) fn subcommand_new(args: &NewArgs) -> Result<()> {
    let (gh_pr_number, description) = step("pulling information from the GitHub CLI", || {
        Command::new("gh").args(["--json", "number", "body"]);
        Ok((1, 2))
    })?;
    Ok(())
}

fn run(command: &mut Command) -> anyhow::Result<()> {
    let status = command.output()?;
    if !status.status.success() {
        bail!(
            "command `{:?}` failed:\n{}{}",
            command,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

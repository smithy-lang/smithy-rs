/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use regex_lite::Regex;
use serde::Deserialize;
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Parser, Eq, PartialEq)]
pub struct NewArgs {
    mode: Mode,
}

#[derive(Debug, Eq, PartialEq)]
enum Mode {
    SmithyRs,
    Sdk,
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "smithy-rs" => Ok(Mode::SmithyRs),
            "sdk" => Ok(Mode::Sdk),
            _ => bail!(format!("invalid mode {s}")),
        }
    }
}

fn step<T>(message: &'static str, step: impl FnOnce() -> Result<T>) -> Result<T> {
    let spinner = ProgressBar::new_spinner()
        .with_message(message)
        .with_style(ProgressStyle::with_template("{spinner} {msg} {elapsed}").unwrap());
    spinner.enable_steady_tick(Duration::from_millis(100));
    let result = step();
    match &result {
        Ok(_) => spinner.finish_and_clear(),
        Err(_) => spinner.finish_with_message(format!("❌ {message}")),
    };
    drop(spinner);
    result
}

#[derive(Deserialize)]
struct PrMetadata {
    number: usize,
    body: String,
    author: Author,
}
#[derive(Deserialize)]
struct Author {
    login: String,
}

pub fn subcommand_new(args: &NewArgs) -> Result<()> {
    let pr = step("pulling information from the GitHub CLI", || {
        let json = run(Command::new("gh").args(["pr", "view", "--json", "number,body,author"]))
            .context("Failed to run the GitHub CLI")?;
        Ok(serde_json::from_str::<PrMetadata>(&json)?)
    })?;
    let number = pr.number;
    let author = pr.author.login;
    let mode = match args.mode {
        Mode::Sdk => "aws-sdk-rust",
        Mode::SmithyRs => "smithy-rs",
    };
    let linked_issues =
        Regex::new(r"https://github.com/[a-zA-Z-]+/([a-zA-Z-]+)/(pull|issues)/([0-9]+)").unwrap();
    let mut issues = linked_issues
        .captures_iter(&pr.body)
        .map(|captures| {
            let repo = captures.get(1).unwrap().as_str();
            let number = captures.get(3).unwrap().as_str();
            format!("\"{repo}#{number}\"")
        })
        .collect::<Vec<_>>();
    eprintln!("{}", &pr.body);

    let shortcode_issues = Regex::new("(smithy-rs|aws-sdk-rust)#[0-9]+").unwrap();
    for m in shortcode_issues.find_iter(&pr.body) {
        issues.push(format!("\"{}\"", m.as_str()));
    }
    issues.push(format!("\"smithy-rs#{number}\""));
    let issues = issues.join(", ");
    let new_entry = format!(
        r#"
[[{mode}]]
message = "Fill me in!"
references = [{issues}]
meta = {{ "breaking" = false, "bug" = "todo", "tada" = "todo" }}
author = "{author}"
    "#
    );
    println!("{}", new_entry.trim());
    Ok(())
}

fn run(command: &mut Command) -> anyhow::Result<String> {
    let status = command.output()?;
    if !status.status.success() {
        bail!(
            "command `{:?}` failed:\n{}{}",
            command,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(String::from_utf8(status.stdout)?)
}

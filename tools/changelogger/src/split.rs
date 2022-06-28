/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use clap::Parser;
use smithy_rs_tool_common::changelog::Changelog;
use smithy_rs_tool_common::git::{find_git_repository_root, Git, GitCLI};
use std::path::{Path, PathBuf};
use std::{env, fs};

const INTERMEDIATE_SOURCE_HEADER: &str =
    "# This is an intermediate file that will be replaced after automation is complete.\n\
     # It will be used to generate a changelog entry for smithy-rs.\n\
     # Do not commit the contents of this file!\n";

const DEST_HEADER: &str =
    "# This file will be used by automation when cutting a release of the SDK\n\
     # to include code generator change log entries into the release notes.\n\
     # This is an auto-generated file. Do not edit.\n";

#[derive(Parser, Debug, Eq, PartialEq)]
pub struct SplitArgs {
    /// The source path to split changelog entries from
    #[clap(long, action)]
    pub source: PathBuf,
    /// The destination to place changelog entries
    #[clap(long, action)]
    pub destination: PathBuf,

    // Git revision to use in `since_commit` fields; for testing only
    #[clap(skip)]
    pub since_commit: Option<String>,
    // Location of the smithy-rs repository; for testing only
    #[clap(skip)]
    pub smithy_rs_location: Option<PathBuf>,
}

pub fn subcommand_split(args: &SplitArgs) -> Result<()> {
    let changelog = Changelog::load_from_file(&args.source).map_err(|errs| {
        anyhow::Error::msg(format!(
            "cannot split changelogs with changelog errors: {:#?}",
            errs
        ))
    })?;

    let (source_log, dest_log) = (
        smithy_rs_entries(changelog.clone()),
        sdk_entries(args, changelog).context("failed to filter SDK entries")?,
    );
    write_entries(&args.source, INTERMEDIATE_SOURCE_HEADER, &source_log)
        .context("failed to write source")?;
    write_entries(&args.destination, DEST_HEADER, &dest_log)
        .context("failed to write destination")?;
    Ok(())
}

fn sdk_entries(args: &SplitArgs, mut changelog: Changelog) -> Result<Changelog> {
    changelog.smithy_rs.clear();

    let current_dir = env::current_dir()?;
    let repo_root = find_git_repository_root(
        "smithy-rs",
        args.smithy_rs_location
            .as_deref()
            .unwrap_or_else(|| current_dir.as_path()),
    )
    .context("failed to find smithy-rs root")?;
    let last_commit = GitCLI::new(&repo_root)?
        .get_head_revision()
        .context("failed to get current revision of smithy-rs")?;
    let last_commit = args
        .since_commit
        .as_deref()
        .unwrap_or_else(|| last_commit.as_ref());
    for entry in changelog.aws_sdk_rust.iter_mut() {
        entry.since_commit = Some(last_commit.to_string());
    }
    Ok(changelog)
}

fn smithy_rs_entries(mut changelog: Changelog) -> Changelog {
    changelog.aws_sdk_rust.clear();
    changelog.sdk_models.clear();
    changelog
}

fn write_entries(into_path: &Path, header: &str, changelog: &Changelog) -> Result<()> {
    let json_changelog = changelog.to_json_string()?;
    fs::write(into_path, format!("{header}\n{json_changelog}"))?;
    Ok(())
}

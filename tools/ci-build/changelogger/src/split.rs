/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use clap::Parser;
use smithy_rs_tool_common::changelog::{Changelog, ChangelogLoader};
use smithy_rs_tool_common::git::{find_git_repository_root, Git, GitCLI};
use std::path::{Path, PathBuf};
use std::{env, fs, mem};

// Value chosen arbitrarily. It is large enough that we're unlikely to lose
// SDK changelog entries, but small enough that the SDK changelog file
// doesn't get too long.
const MAX_ENTRY_AGE: usize = 5;

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
    let loader = ChangelogLoader::default();
    let combined_changelog = if args.source.is_dir() {
        loader.load_from_dir(&args.source)
    } else {
        loader.load_from_file(&args.source)
    }
    .map_err(|errs| {
        anyhow::Error::msg(format!(
            "cannot split changelogs with changelog errors: {errs:#?}"
        ))
    })?;
    let current_sdk_changelog = if args.destination.exists() {
        loader.load_from_file(&args.destination).map_err(|errs| {
            anyhow::Error::msg(format!(
                "failed to load existing SDK changelog entries: {errs:#?}"
            ))
        })?
    } else {
        Changelog::new()
    };

    let new_sdk_entries =
        sdk_entries(args, combined_changelog).context("failed to filter SDK entries")?;
    let sdk_changelog = merge_sdk_entries(current_sdk_changelog, new_sdk_entries);

    write_entries(&args.destination, DEST_HEADER, &sdk_changelog)
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
            .unwrap_or(current_dir.as_path()),
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

fn merge_sdk_entries(old_changelog: Changelog, new_changelog: Changelog) -> Changelog {
    let mut merged = old_changelog;
    merged.merge(new_changelog);

    for entry in &mut merged.aws_sdk_rust {
        *entry.age.get_or_insert(0) += 1;
    }

    let mut to_filter = Vec::new();
    mem::swap(&mut merged.aws_sdk_rust, &mut to_filter);
    merged.aws_sdk_rust = to_filter
        .into_iter()
        .filter(|entry| entry.age.expect("set above") <= MAX_ENTRY_AGE)
        .collect();

    merged
}

fn write_entries(into_path: &Path, header: &str, changelog: &Changelog) -> Result<()> {
    let json_changelog = changelog.to_json_string()?;
    fs::write(into_path, format!("{header}\n{json_changelog}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{merge_sdk_entries, MAX_ENTRY_AGE};
    use smithy_rs_tool_common::changelog::{Changelog, HandAuthoredEntry};

    #[test]
    fn test_merge_sdk_entries() {
        let mut old_entries = Changelog::new();
        old_entries.aws_sdk_rust.push(HandAuthoredEntry {
            message: "old-1".into(),
            age: None,
            ..Default::default()
        });
        old_entries.aws_sdk_rust.push(HandAuthoredEntry {
            message: "old-2".into(),
            age: Some(MAX_ENTRY_AGE),
            ..Default::default()
        });
        old_entries.aws_sdk_rust.push(HandAuthoredEntry {
            message: "old-3".into(),
            age: Some(1),
            ..Default::default()
        });

        let mut new_entries = Changelog::new();
        new_entries.aws_sdk_rust.push(HandAuthoredEntry {
            message: "new-1".into(),
            ..Default::default()
        });
        new_entries.aws_sdk_rust.push(HandAuthoredEntry {
            message: "new-2".into(),
            ..Default::default()
        });

        let combined = merge_sdk_entries(old_entries, new_entries);
        assert_eq!("old-1", combined.aws_sdk_rust[0].message);
        assert_eq!(Some(1), combined.aws_sdk_rust[0].age);
        assert_eq!("old-3", combined.aws_sdk_rust[1].message);
        assert_eq!(Some(2), combined.aws_sdk_rust[1].age);
        assert_eq!("new-1", combined.aws_sdk_rust[2].message);
        assert_eq!(Some(1), combined.aws_sdk_rust[2].age);
        assert_eq!("new-2", combined.aws_sdk_rust[3].message);
        assert_eq!(Some(1), combined.aws_sdk_rust[3].age);
    }
}

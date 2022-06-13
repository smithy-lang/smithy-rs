/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use clap::Parser;
use smithy_rs_tool_common::changelog::Changelog;
use std::fs;
use std::path::{Path, PathBuf};

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
}

pub fn subcommand_split(args: &SplitArgs) -> Result<()> {
    let changelog = Changelog::load_from_file(&args.source).map_err(|errs| {
        anyhow::Error::msg(format!(
            "cannot split changelogs with changelog errors: {:#?}",
            errs
        ))
    })?;

    let (source_log, dest_log) = (smithy_rs_entries(changelog.clone()), sdk_entries(changelog));
    write_entries(&args.source, INTERMEDIATE_SOURCE_HEADER, &source_log)?;
    write_entries(&args.destination, DEST_HEADER, &dest_log)?;
    Ok(())
}

fn sdk_entries(mut changelog: Changelog) -> Changelog {
    changelog.smithy_rs.clear();
    changelog
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

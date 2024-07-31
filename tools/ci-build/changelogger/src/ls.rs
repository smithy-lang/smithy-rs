/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::entry::{ChangeSet, ChangelogEntries};
use crate::render::render;
use anyhow::Context;
use clap::Parser;
use smithy_rs_tool_common::changelog::{ChangelogLoader, ValidationSet};
use smithy_rs_tool_common::git::find_git_repository_root;
use smithy_rs_tool_common::here;
use smithy_rs_tool_common::versions_manifest::CrateVersionMetadataMap;

#[derive(Parser, Debug, Eq, PartialEq)]
pub struct LsArgs {
    /// Which set of changes to preview
    #[clap(long, short, action)]
    pub change_set: ChangeSet,
}

pub fn subcommand_ls(args: LsArgs) -> anyhow::Result<()> {
    let mut dot_changelog = find_git_repository_root("smithy-rs", ".").context(here!())?;
    dot_changelog.push(".changelog");

    let loader = ChangelogLoader::default();
    let changelog = loader.load_from_dir(dot_changelog.clone())?;

    changelog.validate(ValidationSet::Render).map_err(|errs| {
        anyhow::Error::msg(format!(
            "failed to load {dot_changelog:?}: {errors}",
            errors = errs.join("\n")
        ))
    })?;

    let entries = ChangelogEntries::from(changelog);

    let (release_header, release_notes) = render(
        match args.change_set {
            ChangeSet::SmithyRs => &entries.smithy_rs,
            ChangeSet::AwsSdk => &entries.aws_sdk_rust,
        },
        CrateVersionMetadataMap::new(),
        &format!("\nNext changelog preview for {}", args.change_set),
    );

    println!("{release_header}{release_notes}");

    Ok(())
}

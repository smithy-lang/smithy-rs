/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use changelogger::init::subcommand_init;
use changelogger::new_entry::subcommand_new_entry;
use changelogger::preview_next::subcommand_preview_next;
use changelogger::render::subcommand_render;
use changelogger::split::subcommand_split;
use clap::Parser;

#[derive(Parser, Debug, Eq, PartialEq)]
#[clap(name = "changelogger", author, version, about)]
pub enum Args {
    /// Print to stdout the empty "next" CHANGELOG template
    Init(changelogger::init::InitArgs),
    /// Create a new changelog entry Markdown file in the `smithy-rs/.changelog` directory
    NewEntry(changelogger::new_entry::NewEntryArgs),
    /// Render a preview of changelog entries since the last release
    PreviewNext(changelogger::preview_next::PreviewNextArgs),
    /// Render a TOML/JSON changelog into GitHub-flavored Markdown
    Render(changelogger::render::RenderArgs),
    /// Split SDK changelog entries into a separate file
    Split(changelogger::split::SplitArgs),
}

fn main() -> Result<()> {
    match Args::parse() {
        Args::Init(init) => subcommand_init(&init),
        Args::NewEntry(new_entry) => subcommand_new_entry(new_entry),
        Args::PreviewNext(preview_next) => subcommand_preview_next(preview_next),
        Args::Render(render) => subcommand_render(&render),
        Args::Split(split) => subcommand_split(&split),
    }
}

#[cfg(test)]
mod tests {
    use super::Args;
    use changelogger::entry::ChangeSet;
    use changelogger::new_entry::NewEntryArgs;
    use changelogger::preview_next::PreviewNextArgs;
    use changelogger::render::RenderArgs;
    use changelogger::split::SplitArgs;
    use clap::Parser;
    use smithy_rs_tool_common::changelog::{Reference, Target};
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn args_parsing() {
        assert_eq!(
            Args::Split(SplitArgs {
                source: PathBuf::from("fromplace"),
                destination: PathBuf::from("someplace"),
                since_commit: None,
                smithy_rs_location: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "split",
                "--source",
                "fromplace",
                "--destination",
                "someplace"
            ])
            .unwrap()
        );

        assert_eq!(
            Args::Render(RenderArgs {
                change_set: ChangeSet::SmithyRs,
                independent_versioning: false,
                source: vec![PathBuf::from("fromplace")],
                source_to_truncate: PathBuf::from("fromplace"),
                changelog_output: PathBuf::from("some-changelog"),
                release_manifest_output: Some(PathBuf::from("some-manifest")),
                current_release_versions_manifest: None,
                previous_release_versions_manifest: None,
                date_override: None,
                smithy_rs_location: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "render",
                "--change-set",
                "smithy-rs",
                "--source",
                "fromplace",
                "--source-to-truncate",
                "fromplace",
                "--changelog-output",
                "some-changelog",
                "--release-manifest-output",
                "some-manifest"
            ])
            .unwrap()
        );

        assert_eq!(
            Args::Render(RenderArgs {
                change_set: ChangeSet::AwsSdk,
                independent_versioning: true,
                source: vec![
                    PathBuf::from("fromplace"),
                    PathBuf::from("fromanotherplace")
                ],
                source_to_truncate: PathBuf::from("fromplace"),
                changelog_output: PathBuf::from("some-changelog"),
                release_manifest_output: None,
                current_release_versions_manifest: None,
                previous_release_versions_manifest: None,
                date_override: None,
                smithy_rs_location: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "render",
                "--change-set",
                "aws-sdk",
                "--independent-versioning",
                "--source",
                "fromplace",
                "--source",
                "fromanotherplace",
                "--source-to-truncate",
                "fromplace",
                "--changelog-output",
                "some-changelog",
            ])
            .unwrap()
        );

        assert_eq!(
            Args::Render(RenderArgs {
                change_set: ChangeSet::AwsSdk,
                independent_versioning: true,
                source: vec![PathBuf::from("fromplace")],
                source_to_truncate: PathBuf::from("fromplace"),
                changelog_output: PathBuf::from("some-changelog"),
                release_manifest_output: None,
                current_release_versions_manifest: None,
                previous_release_versions_manifest: Some(PathBuf::from("path/to/versions.toml")),
                date_override: None,
                smithy_rs_location: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "render",
                "--change-set",
                "aws-sdk",
                "--independent-versioning",
                "--source",
                "fromplace",
                "--source-to-truncate",
                "fromplace",
                "--changelog-output",
                "some-changelog",
                "--previous-release-versions-manifest",
                "path/to/versions.toml"
            ])
            .unwrap()
        );

        assert_eq!(
            Args::Render(RenderArgs {
                change_set: ChangeSet::AwsSdk,
                independent_versioning: true,
                source: vec![PathBuf::from("fromplace")],
                source_to_truncate: PathBuf::from("fromplace"),
                changelog_output: PathBuf::from("some-changelog"),
                release_manifest_output: None,
                current_release_versions_manifest: Some(PathBuf::from(
                    "path/to/current/versions.toml"
                )),
                previous_release_versions_manifest: Some(PathBuf::from(
                    "path/to/previous/versions.toml"
                )),
                date_override: None,
                smithy_rs_location: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "render",
                "--change-set",
                "aws-sdk",
                "--independent-versioning",
                "--source",
                "fromplace",
                "--source-to-truncate",
                "fromplace",
                "--changelog-output",
                "some-changelog",
                "--current-release-versions-manifest",
                "path/to/current/versions.toml",
                "--previous-release-versions-manifest",
                "path/to/previous/versions.toml"
            ])
            .unwrap()
        );

        assert_eq!(
            Args::NewEntry(NewEntryArgs {
                applies_to: Some(vec![Target::Client, Target::AwsSdk]),
                authors: Some(vec!["external-contrib".to_owned(), "ysaito1001".to_owned()]),
                references: Some(vec![
                    Reference::from_str("smithy-rs#1234").unwrap(),
                    Reference::from_str("aws-sdk-rust#5678").unwrap()
                ]),
                breaking: false,
                new_feature: true,
                bug_fix: false,
                message: Some("Implement a long-awaited feature for S3".to_owned()),
                basename: None,
            }),
            Args::try_parse_from([
                "./changelogger",
                "new-entry",
                "--applies-to",
                "client",
                "--applies-to",
                "aws-sdk-rust",
                "--author",
                "external-contrib",
                "--author",
                "ysaito1001",
                "--ref",
                "smithy-rs#1234",
                "--ref",
                "aws-sdk-rust#5678",
                "--new-feature",
                "--message",
                "Implement a long-awaited feature for S3",
            ])
            .unwrap()
        );

        assert_eq!(
            Args::PreviewNext(PreviewNextArgs {
                change_set: ChangeSet::SmithyRs
            }),
            Args::try_parse_from([
                "./changelogger",
                "preview-next",
                "--change-set",
                "smithy-rs",
            ])
            .unwrap()
        );

        assert_eq!(
            Args::PreviewNext(PreviewNextArgs {
                change_set: ChangeSet::AwsSdk
            }),
            Args::try_parse_from(["./changelogger", "preview-next", "--change-set", "aws-sdk",])
                .unwrap()
        );
    }
}

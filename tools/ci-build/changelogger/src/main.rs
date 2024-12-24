/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use changelogger::ls::subcommand_ls;
use changelogger::new::subcommand_new;
use changelogger::render::subcommand_render;
use changelogger::split::subcommand_split;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Eq, PartialEq)]
#[clap(name = "changelogger", author, version, about)]
pub struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Eq, PartialEq)]
enum Command {
    /// Create a new changelog entry Markdown file in the `smithy-rs/.changelog` directory
    #[clap(visible_alias("n"))]
    New(changelogger::new::NewArgs),
    /// Render a preview of changelog entries since the last release
    #[clap(visible_alias("l"))]
    Ls(changelogger::ls::LsArgs),
    /// Render a TOML/JSON changelog into GitHub-flavored Markdown
    Render(changelogger::render::RenderArgs),
    /// Split SDK changelog entries into a separate file
    Split(changelogger::split::SplitArgs),
}

fn main() -> Result<()> {
    use Command::*;
    match Args::parse().command {
        New(new) => subcommand_new(new),
        Ls(ls) => subcommand_ls(ls),
        Render(render) => subcommand_render(&render),
        Split(split) => subcommand_split(&split),
    }
}

#[cfg(test)]
mod tests {
    use super::{Args, Command};
    use changelogger::entry::ChangeSet;
    use changelogger::ls::LsArgs;
    use changelogger::new::NewArgs;
    use changelogger::render::RenderArgs;
    use changelogger::split::SplitArgs;
    use clap::Parser;
    use smithy_rs_tool_common::changelog::{Reference, Target};
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn args_parsing() {
        assert_eq!(
            Args {
                command: Command::Split(SplitArgs {
                    source: PathBuf::from("fromplace"),
                    destination: PathBuf::from("someplace"),
                    since_commit: None,
                    smithy_rs_location: None,
                })
            },
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
            Args {
                command: Command::Render(RenderArgs {
                    change_set: ChangeSet::SmithyRs,
                    independent_versioning: false,
                    source: vec![PathBuf::from("fromplace")],
                    changelog_output: PathBuf::from("some-changelog"),
                    source_to_truncate: Some(PathBuf::from("fromplace")),
                    release_manifest_output: Some(PathBuf::from("some-manifest")),
                    current_release_versions_manifest: None,
                    previous_release_versions_manifest: None,
                    date_override: None,
                    smithy_rs_location: None,
                    aws_sdk_rust_location: None,
                })
            },
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
            Args {
                command: Command::Render(RenderArgs {
                    change_set: ChangeSet::AwsSdk,
                    independent_versioning: true,
                    source: vec![
                        PathBuf::from("fromplace"),
                        PathBuf::from("fromanotherplace")
                    ],
                    changelog_output: PathBuf::from("some-changelog"),
                    source_to_truncate: Some(PathBuf::from("fromplace")),
                    release_manifest_output: None,
                    current_release_versions_manifest: None,
                    previous_release_versions_manifest: None,
                    date_override: None,
                    smithy_rs_location: None,
                    aws_sdk_rust_location: Some(PathBuf::from("aws-sdk-rust-location")),
                })
            },
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
                "--aws-sdk-rust-location",
                "aws-sdk-rust-location",
            ])
            .unwrap()
        );

        assert_eq!(
            Args {
                command: Command::Render(RenderArgs {
                    change_set: ChangeSet::AwsSdk,
                    independent_versioning: true,
                    source: vec![PathBuf::from("fromplace")],
                    changelog_output: PathBuf::from("some-changelog"),
                    source_to_truncate: Some(PathBuf::from("fromplace")),
                    release_manifest_output: None,
                    current_release_versions_manifest: None,
                    previous_release_versions_manifest: Some(PathBuf::from(
                        "path/to/versions.toml"
                    )),
                    date_override: None,
                    smithy_rs_location: None,
                    aws_sdk_rust_location: Some(PathBuf::from("aws-sdk-rust-location")),
                })
            },
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
                "path/to/versions.toml",
                "--aws-sdk-rust-location",
                "aws-sdk-rust-location",
            ])
            .unwrap()
        );

        assert_eq!(
            Args {
                command: Command::Render(RenderArgs {
                    change_set: ChangeSet::AwsSdk,
                    independent_versioning: true,
                    source: vec![PathBuf::from("fromplace")],
                    changelog_output: PathBuf::from("some-changelog"),
                    source_to_truncate: Some(PathBuf::from("fromplace")),
                    release_manifest_output: None,
                    current_release_versions_manifest: Some(PathBuf::from(
                        "path/to/current/versions.toml"
                    )),
                    previous_release_versions_manifest: Some(PathBuf::from(
                        "path/to/previous/versions.toml"
                    )),
                    date_override: None,
                    smithy_rs_location: None,
                    aws_sdk_rust_location: Some(PathBuf::from("aws-sdk-rust-location")),
                })
            },
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
                "path/to/previous/versions.toml",
                "--aws-sdk-rust-location",
                "aws-sdk-rust-location",
            ])
            .unwrap()
        );

        assert_eq!(
            Args {
                command: Command::New(NewArgs {
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
                })
            },
            Args::try_parse_from([
                "./changelogger",
                "new",
                "--applies-to",
                "client",
                "--applies-to",
                "aws-sdk-rust",
                "--authors",
                "external-contrib",
                "--authors",
                "ysaito1001",
                "--references",
                "smithy-rs#1234",
                "--references",
                "aws-sdk-rust#5678",
                "--new-feature",
                "--message",
                "Implement a long-awaited feature for S3",
            ])
            .unwrap()
        );

        assert_eq!(
            Args {
                command: Command::Ls(LsArgs {
                    change_set: ChangeSet::SmithyRs
                })
            },
            Args::try_parse_from(["./changelogger", "ls", "--change-set", "smithy-rs",]).unwrap()
        );

        assert_eq!(
            Args {
                command: Command::Ls(LsArgs {
                    change_set: ChangeSet::AwsSdk
                })
            },
            Args::try_parse_from(["./changelogger", "ls", "--change-set", "aws-sdk",]).unwrap()
        );
    }
}

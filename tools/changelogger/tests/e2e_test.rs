/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use changelogger::entry::ChangeSet;
use changelogger::render::{subcommand_render, RenderArgs, EXAMPLE_ENTRY, USE_UPDATE_CHANGELOGS};
use changelogger::split::{subcommand_split, SplitArgs};
use smithy_rs_tool_common::git::{CommitHash, Git, GitCLI};
use smithy_rs_tool_common::shell::handle_failure;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use time::OffsetDateTime;

const SOURCE_TOML: &'static str = r#"
    [[aws-sdk-rust]]
    message = "Some change"
    references = ["aws-sdk-rust#123", "smithy-rs#456"]
    meta = { "breaking" = false, "tada" = false, "bug" = true }
    since-commit = "REPLACE_SINCE_COMMIT_1"
    author = "test-dev"

    [[aws-sdk-rust]]
    message = "Some other change"
    references = ["aws-sdk-rust#234", "smithy-rs#567"]
    meta = { "breaking" = false, "tada" = false, "bug" = true }
    since-commit = "REPLACE_SINCE_COMMIT_2"
    author = "test-dev"

    [[smithy-rs]]
    message = "Another change"
    references = ["smithy-rs#1234"]
    meta = { "breaking" = false, "tada" = false, "bug" = false }
    author = "another-dev"
    "#;

const SDK_MODEL_SOURCE_TOML: &'static str = r#"
    [[aws-sdk-model]]
    module = "aws-sdk-ec2"
    version = "0.12.0"
    kind = "Feature"
    message = "Some API change"
"#;

fn create_fake_repo_root(
    path: &Path,
    smithy_rs_version: &str,
    aws_sdk_version: &str,
) -> (CommitHash, CommitHash) {
    handle_failure(
        "git-init",
        &Command::new("git")
            .arg("init")
            .arg(".")
            .current_dir(path)
            .output()
            .unwrap(),
    )
    .unwrap();
    fs::write(
        path.join("gradle.properties"),
        format!(
            r#"
            smithy.rs.runtime.crate.version={}
            aws.sdk.version={}
            "#,
            smithy_rs_version, aws_sdk_version
        ),
    )
    .unwrap();

    // Simulate some release commits
    let git = GitCLI::new(path).unwrap();
    let mut release_commits = Vec::new();
    for i in 0..2 {
        fs::write(path.join("random-file"), format!("{}", i * 10)).unwrap();
        git.stage(path).unwrap();
        git.commit(
            "test-dev",
            "test-dev@example.com",
            &format!("prepare release {}", i),
        )
        .unwrap();
        fs::write(path.join("random-file"), format!("{}", i * 10 + 1)).unwrap();
        git.stage(path).unwrap();
        git.commit(
            "test-dev",
            "test-dev@example.com",
            &format!("finish release {}", i),
        )
        .unwrap();
        release_commits.push(git.get_head_revision().unwrap());
    }
    (release_commits.remove(0), release_commits.remove(0))
}

#[test]
fn split_aws_sdk_test() {
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.toml");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(&source_path, SOURCE_TOML).unwrap();
    fs::write(&dest_path, "overwrite-me").unwrap();

    subcommand_split(&SplitArgs {
        source: source_path.clone(),
        destination: dest_path.clone(),
        since_commit: Some("test-commit-hash".into()),
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let source = fs::read_to_string(&source_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();

    pretty_assertions::assert_str_eq!(
        r#"# This is an intermediate file that will be replaced after automation is complete.
# It will be used to generate a changelog entry for smithy-rs.
# Do not commit the contents of this file!

{
  "smithy-rs": [
    {
      "message": "Another change",
      "meta": {
        "bug": false,
        "breaking": false,
        "tada": false
      },
      "author": "another-dev",
      "references": [
        "smithy-rs#1234"
      ],
      "since-commit": null
    }
  ],
  "aws-sdk-rust": [],
  "aws-sdk-model": []
}"#,
        source
    );
    pretty_assertions::assert_str_eq!(
        r#"# This file will be used by automation when cutting a release of the SDK
# to include code generator change log entries into the release notes.
# This is an auto-generated file. Do not edit.

{
  "smithy-rs": [],
  "aws-sdk-rust": [
    {
      "message": "Some change",
      "meta": {
        "bug": true,
        "breaking": false,
        "tada": false
      },
      "author": "test-dev",
      "references": [
        "aws-sdk-rust#123",
        "smithy-rs#456"
      ],
      "since-commit": "test-commit-hash"
    },
    {
      "message": "Some other change",
      "meta": {
        "bug": true,
        "breaking": false,
        "tada": false
      },
      "author": "test-dev",
      "references": [
        "aws-sdk-rust#234",
        "smithy-rs#567"
      ],
      "since-commit": "test-commit-hash"
    }
  ],
  "aws-sdk-model": []
}"#,
        dest
    );
}

#[test]
fn render_smithy_rs_test() {
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(&source_path, SOURCE_TOML).unwrap();
    fs::write(
        &dest_path,
        format!(
            "{}\nv0.41.0 (Some date in the past)\n=========\n\nOld entry contents\n",
            USE_UPDATE_CHANGELOGS
        ),
    )
    .unwrap();
    fs::write(&release_manifest_path, "overwrite-me").unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::SmithyRs,
        independent_versioning: false,
        source: vec![source_path.clone()],
        source_to_truncate: source_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let source = fs::read_to_string(&source_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY.trim(), source);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
v0.42.0 (January 1st, 1970)
===========================
**New this release:**
- ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change

**Contributors**
Thank you for your contributions! ❤
- @another-dev ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234))

v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
    pretty_assertions::assert_str_eq!(
        r#"{
  "tagName": "v0.42.0",
  "name": "v0.42.0 (January 1st, 1970)",
  "body": "**New this release:**\n- ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change\n\n**Contributors**\nThank you for your contributions! ❤\n- @another-dev ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234))\n",
  "prerelease": true
}"#,
        release_manifest
    );
}

#[test]
fn render_aws_sdk_test() {
    let tmp_dir = TempDir::new().unwrap();
    let source1_path = tmp_dir.path().join("source1.toml");
    let source2_path = tmp_dir.path().join("source2.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("aws-sdk-rust-release-manifest.json");
    let versions_manifest_path = tmp_dir.path().join("versions.toml");

    let (release_1_commit, release_2_commit) =
        create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(
        &source1_path,
        SOURCE_TOML
            .replace("REPLACE_SINCE_COMMIT_1", release_1_commit.as_ref())
            .replace("REPLACE_SINCE_COMMIT_2", release_2_commit.as_ref()),
    )
    .unwrap();
    fs::write(&source2_path, SDK_MODEL_SOURCE_TOML).unwrap();
    fs::write(
        &dest_path,
        format!(
            "{}\nv0.41.0 (Some date in the past)\n=========\n\nOld entry contents\n",
            USE_UPDATE_CHANGELOGS
        ),
    )
    .unwrap();
    fs::write(&release_manifest_path, "overwrite-me").unwrap();
    fs::write(
        &versions_manifest_path,
        format!(
            "smithy_rs_revision = '{release_1_commit}'
             aws_doc_sdk_examples_revision = 'not-relevant'
             [crates]",
        ),
    )
    .unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::AwsSdk,
        independent_versioning: false,
        source: vec![source1_path.clone(), source2_path.clone()],
        source_to_truncate: source1_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        previous_release_versions_manifest: Some(versions_manifest_path),
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let source1 = fs::read_to_string(&source1_path).unwrap();
    let source2 = fs::read_to_string(&source2_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY.trim(), source1);
    pretty_assertions::assert_str_eq!(SDK_MODEL_SOURCE_TOML, source2);

    // It should only have one of the SDK changelog entries since
    // the other should be filtered out by the `since_commit` attribute
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
v0.12.0 (January 1st, 1970)
===========================
**New this release:**
- 🐛 ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567), @test-dev) Some other change

**Service Features:**
- `aws-sdk-ec2` (0.12.0): Some API change

**Contributors**
Thank you for your contributions! ❤
- @test-dev ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567))

v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
    pretty_assertions::assert_str_eq!(
        r#"{
  "tagName": "v0.12.0",
  "name": "v0.12.0 (January 1st, 1970)",
  "body": "**New this release:**\n- 🐛 ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567), @test-dev) Some other change\n\n**Service Features:**\n- `aws-sdk-ec2` (0.12.0): Some API change\n\n**Contributors**\nThank you for your contributions! ❤\n- @test-dev ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567))\n",
  "prerelease": true
}"#,
        release_manifest
    );
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use changelogger::entry::ChangeSet;
use changelogger::render::{subcommand_render, RenderArgs, EXAMPLE_ENTRY, USE_UPDATE_CHANGELOGS};
use changelogger::split::{subcommand_split, SplitArgs};
use smithy_rs_tool_common::changelog::{Changelog, HandAuthoredEntry};
use smithy_rs_tool_common::git::{CommitHash, Git, GitCLI};
use smithy_rs_tool_common::shell::handle_failure;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use time::OffsetDateTime;

const SOURCE_TOML: &str = r#"
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

const SDK_MODEL_SOURCE_TOML: &str = r#"
    [[aws-sdk-model]]
    module = "aws-sdk-ec2"
    version = "0.12.0"
    kind = "Feature"
    message = "Some API change"
"#;

const VERSIONS_TOML: &str = r#"
    smithy_rs_revision = '41ca31b85b4ba8c0ad680fe62a230266cc52cc44'
    aws_doc_sdk_examples_revision = '97a177aab8c3d2fef97416cb66e4b4d0da840138'

    [manual_interventions]
    crates_to_remove = []
    [crates.aws-config]
    category = 'AwsRuntime'
    version = '0.54.1'
    source_hash = 'e93380cfbd05e68d39801cbf0113737ede552a5eceb28f4c34b090048d539df9'

    [crates.aws-sdk-accessanalyzer]
    category = 'AwsSdk'
    version = '0.24.0'
    source_hash = 'a7728756b41b33d02f68a5865d3456802b7bc3949ec089790bc4e726c0de8539'
    model_hash = '71f1f130504ebd55396c3166d9441513f97e49b281a5dd420fd7e2429860b41b'

    [crates.aws-smithy-async]
    category = 'SmithyRuntime'
    version = '0.54.1'
    source_hash = '8ced52afc783cbb0df47ee8b55260b98e9febdc95edd796ed14c43db5199b0a9'

    [release]
    tag = 'release-2023-01-26'

    [release.crates]
    aws-config = "0.54.1"
    aws-sdk-accessanalyzer = '0.24.0'
    aws-smithy-async = '0.54.1'
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
fn split_aws_sdk() {
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.toml");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(&source_path, SOURCE_TOML).unwrap();

    let mut original_dest_changelog = Changelog::new();
    original_dest_changelog
        .aws_sdk_rust
        .push(HandAuthoredEntry {
            message: "old-existing-entry-1".into(),
            // this entry should get filtered out since it's too old
            age: Some(5),
            ..Default::default()
        });
    original_dest_changelog
        .aws_sdk_rust
        .push(HandAuthoredEntry {
            message: "old-existing-entry-2".into(),
            age: Some(2),
            since_commit: Some("commit-old-existing-entry-2".into()),
            ..Default::default()
        });
    fs::write(
        &dest_path,
        original_dest_changelog.to_json_string().unwrap(),
    )
    .unwrap();

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
        "tada": false,
        "target": "all"
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
      "message": "old-existing-entry-2",
      "meta": {
        "bug": false,
        "breaking": false,
        "tada": false
      },
      "author": "",
      "references": [],
      "since-commit": "commit-old-existing-entry-2",
      "age": 3
    },
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
      "since-commit": "test-commit-hash",
      "age": 1
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
      "since-commit": "test-commit-hash",
      "age": 1
    }
  ],
  "aws-sdk-model": []
}"#,
        dest
    );
}

#[test]
fn render_smithy_rs() {
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
        independent_versioning: true,
        source: vec![source_path.clone()],
        source_to_truncate: source_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
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
January 1st, 1970
=================
**New this release:**
- (all, [smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change

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
  "tagName": "release-1970-01-01",
  "name": "January 1st, 1970",
  "body": "**New this release:**\n- (all, [smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change\n\n**Contributors**\nThank you for your contributions! ❤\n- @another-dev ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234))\n\n",
  "prerelease": true
}"#,
        release_manifest
    );
}

#[test]
fn render_aws_sdk() {
    let tmp_dir = TempDir::new().unwrap();
    let source1_path = tmp_dir.path().join("source1.toml");
    let source2_path = tmp_dir.path().join("source2.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("aws-sdk-rust-release-manifest.json");
    let previous_versions_manifest_path = tmp_dir.path().join("versions.toml");

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
        &previous_versions_manifest_path,
        format!(
            "smithy_rs_revision = '{release_1_commit}'
             aws_doc_sdk_examples_revision = 'not-relevant'
             [crates]",
        ),
    )
    .unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::AwsSdk,
        independent_versioning: true,
        source: vec![source1_path.clone(), source2_path.clone()],
        source_to_truncate: source1_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: Some(previous_versions_manifest_path),
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
January 1st, 1970
=================
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
  "tagName": "release-1970-01-01",
  "name": "January 1st, 1970",
  "body": "**New this release:**\n- 🐛 ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567), @test-dev) Some other change\n\n**Service Features:**\n- `aws-sdk-ec2` (0.12.0): Some API change\n\n**Contributors**\nThank you for your contributions! ❤\n- @test-dev ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/awslabs/smithy-rs/issues/567))\n\n",
  "prerelease": true
}"#,
        release_manifest
    );
}

/// entries with target set to each of the possible ones, and one entry with no target
/// set, which should result in the default
#[test]
fn render_smithy_entries() {
    const NEXT_CHANGELOG: &str = r#"
# Example changelog entries
# [[aws-sdk-rust]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
#
# [[smithy-rs]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
[[aws-sdk-rust]]
message = "Some change"
references = ["aws-sdk-rust#123", "smithy-rs#456"]
meta = { "breaking" = false, "tada" = false, "bug" = true }
since-commit = "REPLACE_SINCE_COMMIT_1"
author = "test-dev"

[[smithy-rs]]
message = "First change - server"
references = ["smithy-rs#1"]
meta = { "breaking" = false, "tada" = false, "bug" = false, target = "server" }
author = "server-dev"

[[smithy-rs]]
message = "Second change - should be all"
references = ["smithy-rs#2"]
meta = { "breaking" = false, "tada" = false, "bug" = false, target = "all" }
author = "another-dev"

[[smithy-rs]]
message = "Third change - empty"
references = ["smithy-rs#3"]
meta = { "breaking" = true, "tada" = false, "bug" = false }
author = "rcoh"

[[smithy-rs]]
message = "Fourth change - client"
references = ["smithy-rs#4"]
meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "client" }
author = "rcoh"
"#;
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(&source_path, NEXT_CHANGELOG).unwrap();
    fs::write(
        &dest_path,
        format!(
            "{}\nv0.41.0 (Some date in the past)\n=========\n\nOld entry contents\n",
            USE_UPDATE_CHANGELOGS
        ),
    )
    .unwrap();
    fs::write(release_manifest_path, "overwrite-me").unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::SmithyRs,
        independent_versioning: true,
        source: vec![source_path.clone()],
        source_to_truncate: source_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let source = fs::read_to_string(&source_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();

    // source file should be empty
    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY.trim(), source);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**Breaking Changes:**
- ⚠ (all, [smithy-rs#3](https://github.com/awslabs/smithy-rs/issues/3)) Third change - empty

**New this release:**
- (server, [smithy-rs#1](https://github.com/awslabs/smithy-rs/issues/1), @server-dev) First change - server
- (all, [smithy-rs#2](https://github.com/awslabs/smithy-rs/issues/2), @another-dev) Second change - should be all
- (client, [smithy-rs#4](https://github.com/awslabs/smithy-rs/issues/4)) Fourth change - client

**Contributors**
Thank you for your contributions! ❤
- @another-dev ([smithy-rs#2](https://github.com/awslabs/smithy-rs/issues/2))
- @server-dev ([smithy-rs#1](https://github.com/awslabs/smithy-rs/issues/1))


v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
}

/// aws_sdk_rust should not be allowed to have target entries
#[test]
fn aws_sdk_cannot_have_target() {
    const NEXT_CHANGELOG: &str = r#"
# Example changelog entries
# [[aws-sdk-rust]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
#
# [[smithy-rs]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
[[aws-sdk-rust]]
message = "Some change"
references = ["aws-sdk-rust#123", "smithy-rs#456"]
meta = { "breaking" = false, "tada" = false, "bug" = true, "target" = "client" }
since-commit = "REPLACE_SINCE_COMMIT_1"
author = "test-dev"

[[smithy-rs]]
message = "First change - server"
references = ["smithy-rs#1"]
meta = { "breaking" = false, "tada" = false, "bug" = false, target = "server" }
author = "server-dev"

[[smithy-rs]]
message = "Second change - should be all"
references = ["smithy-rs#2"]
meta = { "breaking" = false, "tada" = false, "bug" = false, target = "all" }
author = "another-dev"

[[smithy-rs]]
message = "Third change - empty"
references = ["smithy-rs#3"]
meta = { "breaking" = true, "tada" = false, "bug" = false }
author = "rcoh"

[[smithy-rs]]
message = "Fourth change - client"
references = ["smithy-rs#4"]
meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "client" }
author = "rcoh"
"#;
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    fs::write(&source_path, NEXT_CHANGELOG).unwrap();
    fs::write(
        &dest_path,
        format!(
            "{}\nv0.41.0 (Some date in the past)\n=========\n\nOld entry contents\n",
            USE_UPDATE_CHANGELOGS
        ),
    )
    .unwrap();
    fs::write(release_manifest_path, "overwrite-me").unwrap();

    let result = subcommand_render(&RenderArgs {
        change_set: ChangeSet::SmithyRs,
        independent_versioning: true,
        source: vec![source_path.clone()],
        source_to_truncate: source_path,
        changelog_output: dest_path,
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
    });

    if let Err(e) = result {
        let index = e
            .to_string()
            .find("aws-sdk-rust changelog entry cannot have an affected target");
        assert!(index.is_some());
    } else {
        panic!("This should have been error that aws-sdk-rust has a target entry");
    }
}

#[test]
fn render_crate_versions() {
    let tmp_dir = TempDir::new().unwrap();
    let source_path = tmp_dir.path().join("source.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");
    let current_versions_manifest_path = tmp_dir.path().join("versions.toml");

    create_fake_repo_root(tmp_dir.path(), "0.54.1", "0.24.0");

    fs::write(&source_path, SOURCE_TOML).unwrap();
    fs::write(
        &dest_path,
        format!(
            "{}\nv0.54.0 (Some date in the past)\n=========\n\nOld entry contents\n",
            USE_UPDATE_CHANGELOGS
        ),
    )
    .unwrap();
    fs::write(&release_manifest_path, "overwrite-me").unwrap();
    fs::write(&current_versions_manifest_path, VERSIONS_TOML).unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::SmithyRs,
        independent_versioning: true,
        source: vec![source_path.clone()],
        source_to_truncate: source_path.clone(),
        changelog_output: dest_path.clone(),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: Some(current_versions_manifest_path),
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let source = fs::read_to_string(&source_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    // source file should be empty
    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY.trim(), source);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**New this release:**
- (all, [smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change

**Contributors**
Thank you for your contributions! ❤
- @another-dev ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234))

**Crate Versions**
<details>
<summary>Click to expand to view crate versions...</summary>

|Crate|Version|
|-|-|
|aws-config|0.54.1|
|aws-sdk-accessanalyzer|0.24.0|
|aws-smithy-async|0.54.1|
</details>


v0.54.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
    pretty_assertions::assert_str_eq!(
        r#"{
  "tagName": "release-1970-01-01",
  "name": "January 1st, 1970",
  "body": "**New this release:**\n- (all, [smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234), @another-dev) Another change\n\n**Contributors**\nThank you for your contributions! ❤\n- @another-dev ([smithy-rs#1234](https://github.com/awslabs/smithy-rs/issues/1234))\n\n**Crate Versions**\n<details>\n<summary>Click to expand to view crate versions...</summary>\n\n|Crate|Version|\n|-|-|\n|aws-config|0.54.1|\n|aws-sdk-accessanalyzer|0.24.0|\n|aws-smithy-async|0.54.1|\n</details>\n\n",
  "prerelease": true
}"#,
        release_manifest
    );
}

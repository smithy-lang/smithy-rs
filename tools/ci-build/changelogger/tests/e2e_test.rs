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
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};

const SOURCE_MARKDOWN1: &str = r#"---
applies_to: ["aws-sdk-rust"]
authors: ["test-dev"]
references: ["aws-sdk-rust#123", "smithy-rs#456"]
breaking: false
new_feature: false
bug_fix: true
---
Some change
"#;

const SOURCE_MARKDOWN2: &str = r#"---
applies_to: ["aws-sdk-rust"]
authors: ["test-dev"]
references: ["aws-sdk-rust#234", "smithy-rs#567"]
breaking: false
new_feature: false
bug_fix: true
---
Some other change
"#;

const SOURCE_MARKDOWN3: &str = r#"---
applies_to: ["client", "server"]
authors: ["another-dev"]
references: ["smithy-rs#1234"]
breaking: false
new_feature: false
bug_fix: false
---
Another change
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

    handle_failure(
        "git-tag",
        &Command::new("git")
            .arg("tag")
            .arg("release-1970-01-01")
            .current_dir(path)
            .output()
            .unwrap(),
    )
    .unwrap();

    (release_commits.remove(0), release_commits.remove(0))
}

fn create_changelog_entry_markdown_files(
    markdown_files: &[&str],
    changelog_dir: &Path,
) -> Vec<PathBuf> {
    let mut source_paths = Vec::with_capacity(markdown_files.len());
    markdown_files.iter().enumerate().for_each(|(i, md)| {
        let source_path = changelog_dir.join(format!("source{i}.md"));
        fs::write(&source_path, md.trim()).unwrap();
        source_paths.push(source_path);
    });
    source_paths
}

#[test]
fn split_aws_sdk() {
    let tmp_dir = TempDir::new().unwrap();
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let dest_path = tmp_dir.path().join("dest.toml");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");
    create_changelog_entry_markdown_files(
        // Exclude `SOURCE_MARKDOWN2` to make string comparison verification at the end of this
        // function deterministic across platforms. If `SOURCE_MARKDOWN2` were included, `dest_path`
        // might list `SOURCE_MARKDOWN1` and `SOURCE_MARKDOWN2` in an non-deterministic order due to
        // the use of `std::fs::read_dir` in `ChangelogLoader::load_from_dir`, making the test
        // brittle.
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN3],
        &dot_changelog_path,
    );

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
        source: dot_changelog_path,
        destination: dest_path.clone(),
        since_commit: Some("test-commit-hash".into()),
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();

    let dest = fs::read_to_string(&dest_path).unwrap();

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
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    create_changelog_entry_markdown_files(
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN2, SOURCE_MARKDOWN3],
        &dot_changelog_path,
    );
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
        source: vec![dot_changelog_path.clone()],
        changelog_output: dest_path.clone(),
        source_to_truncate: Some(dot_changelog_path.clone()),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
        aws_sdk_rust_location: None,
    })
    .unwrap();

    let dot_example = fs::read_to_string(dot_changelog_path.join(".example")).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY, dot_example);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**New this release:**
- (all, [smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234), @another-dev) Another change

**Contributors**
Thank you for your contributions! ❤
- @another-dev ([smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234))


v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
    pretty_assertions::assert_str_eq!(
        r#"{
  "tagName": "release-1970-01-01.2",
  "name": "January 1st, 1970",
  "body": "**New this release:**\n- (all, [smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234), @another-dev) Another change\n\n**Contributors**\nThank you for your contributions! ❤\n- @another-dev ([smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234))\n\n",
  "prerelease": false
}"#,
        release_manifest
    );
}

#[test]
fn render_aws_sdk() {
    let tmp_dir = TempDir::new().unwrap();
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let model_source_path = tmp_dir.path().join("model_source.toml");
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("aws-sdk-rust-release-manifest.json");
    let previous_versions_manifest_path = tmp_dir.path().join("versions.toml");

    let (last_release_commit, _) = create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");
    let source_paths = create_changelog_entry_markdown_files(
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN3],
        &dot_changelog_path,
    );

    // Replicate the state by running the `split` subcommand where `SOURCE_MARKDOWN1` is saved in
    // `dest_path` as an old release SDK changelog entry. That's done by `last_release_commit` will
    // be the value of `since-commit` for SOURCE_MARKDOWN1
    subcommand_split(&SplitArgs {
        source: dot_changelog_path.clone(),
        destination: dest_path.clone(),
        since_commit: Some(last_release_commit.as_ref().to_owned()),
        smithy_rs_location: Some(tmp_dir.path().into()),
    })
    .unwrap();
    // After split, make sure to remove `SOURCE_MARKDOWN1` from the changelog directory otherwise
    // it'd be rendered when the subcommand `render` runs below.
    fs::remove_file(&source_paths[0]).unwrap();

    // Now that `SOURCE_MARKDOWN1` has been saved in `dest_path` as an old SDK entry, create a new
    // SDK changelog entry in the changelog directory.
    fs::write(
        &dot_changelog_path.join(format!("123456.md")),
        SOURCE_MARKDOWN2.trim(),
    )
    .unwrap();

    fs::write(&model_source_path, SDK_MODEL_SOURCE_TOML).unwrap();
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
            "smithy_rs_revision = '{last_release_commit}'
                 [crates]",
        ),
    )
    .unwrap();

    subcommand_render(&RenderArgs {
        change_set: ChangeSet::AwsSdk,
        independent_versioning: true,
        source: vec![dot_changelog_path.clone(), model_source_path.clone()],
        changelog_output: dest_path.clone(),
        source_to_truncate: Some(dot_changelog_path.clone()),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH + Duration::days(1)),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: Some(previous_versions_manifest_path),
        smithy_rs_location: Some(tmp_dir.path().into()),
        aws_sdk_rust_location: None,
    })
    .unwrap();

    let dot_example = fs::read_to_string(dot_changelog_path.join(".example")).unwrap();
    let model_source = fs::read_to_string(&model_source_path).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY, dot_example);
    pretty_assertions::assert_str_eq!(SDK_MODEL_SOURCE_TOML, model_source);

    // It should only have one of the SDK changelog entries since
    // the other should be filtered out by the `since_commit` attribute
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 2nd, 1970
=================
**New this release:**
- :bug: ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/smithy-lang/smithy-rs/issues/567), @test-dev) Some other change

**Service Features:**
- `aws-sdk-ec2` (0.12.0): Some API change

**Contributors**
Thank you for your contributions! ❤
- @test-dev ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/smithy-lang/smithy-rs/issues/567))


v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
    pretty_assertions::assert_str_eq!(
        r#"{
  "tagName": "release-1970-01-02",
  "name": "January 2nd, 1970",
  "body": "**New this release:**\n- :bug: ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/smithy-lang/smithy-rs/issues/567), @test-dev) Some other change\n\n**Service Features:**\n- `aws-sdk-ec2` (0.12.0): Some API change\n\n**Contributors**\nThank you for your contributions! ❤\n- @test-dev ([aws-sdk-rust#234](https://github.com/awslabs/aws-sdk-rust/issues/234), [smithy-rs#567](https://github.com/smithy-lang/smithy-rs/issues/567))\n\n",
  "prerelease": false
}"#,
        release_manifest
    );
}

#[test]
fn render_server_smithy_entry() {
    const SERVER_ONLY_MARKDOWN: &str = r#"---
applies_to: ["server"]
authors: ["server-dev"]
references: ["smithy-rs#1"]
breaking: false
new_feature: false
bug_fix: false
---
Change from server
"#;
    let tmp_dir = TempDir::new().unwrap();
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    create_changelog_entry_markdown_files(
        // Not include other `smithy-rs` targeted entries other than `SERVER_ONLY_MARKDOWN` to make
        // string comparison verification deterministic
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN2, SERVER_ONLY_MARKDOWN],
        &dot_changelog_path,
    );
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
        source: vec![dot_changelog_path.clone()],
        changelog_output: dest_path.clone(),
        source_to_truncate: Some(dot_changelog_path.clone()),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
        aws_sdk_rust_location: None,
    })
    .unwrap();

    let dot_example = fs::read_to_string(dot_changelog_path.join(".example")).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();

    // source file should be empty
    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY, &dot_example);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**New this release:**
- (server, [smithy-rs#1](https://github.com/smithy-lang/smithy-rs/issues/1), @server-dev) Change from server

**Contributors**
Thank you for your contributions! ❤
- @server-dev ([smithy-rs#1](https://github.com/smithy-lang/smithy-rs/issues/1))


v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
}

#[test]
fn render_client_smithy_entry() {
    const CLIENT_ONLY_MARKDOWN: &str = r#"---
applies_to: ["client"]
authors: ["client-dev"]
references: ["smithy-rs#4"]
breaking: false
new_feature: false
bug_fix: false
---
Change from client
"#;
    let tmp_dir = TempDir::new().unwrap();
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");

    create_fake_repo_root(tmp_dir.path(), "0.42.0", "0.12.0");

    create_changelog_entry_markdown_files(
        // Not include other `smithy-rs` targeted entries other than `CLIENT_ONLY_MARKDOWN` to make
        // string comparison verification deterministic
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN2, CLIENT_ONLY_MARKDOWN],
        &dot_changelog_path,
    );
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
        source: vec![dot_changelog_path.clone()],
        changelog_output: dest_path.clone(),
        source_to_truncate: Some(dot_changelog_path.clone()),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: None,
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
        aws_sdk_rust_location: None,
    })
    .unwrap();

    let dot_example = fs::read_to_string(dot_changelog_path.join(".example")).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();

    // source file should be empty
    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY, &dot_example);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**New this release:**
- (client, [smithy-rs#4](https://github.com/smithy-lang/smithy-rs/issues/4), @client-dev) Change from client

**Contributors**
Thank you for your contributions! ❤
- @client-dev ([smithy-rs#4](https://github.com/smithy-lang/smithy-rs/issues/4))


v0.41.0 (Some date in the past)
=========

Old entry contents
"#,
        dest
    );
}

#[test]
fn render_crate_versions() {
    let tmp_dir = TempDir::new().unwrap();
    let dot_changelog = TempDir::new_in(tmp_dir.path()).unwrap();
    let dot_changelog_path = dot_changelog.path().to_owned();
    let dest_path = tmp_dir.path().join("dest.md");
    let release_manifest_path = tmp_dir.path().join("smithy-rs-release-manifest.json");
    let current_versions_manifest_path = tmp_dir.path().join("versions.toml");

    create_fake_repo_root(tmp_dir.path(), "0.54.1", "0.24.0");

    create_changelog_entry_markdown_files(
        &[SOURCE_MARKDOWN1, SOURCE_MARKDOWN2, SOURCE_MARKDOWN3],
        &dot_changelog_path,
    );
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
        source: vec![dot_changelog_path.clone()],
        changelog_output: dest_path.clone(),
        source_to_truncate: Some(dot_changelog_path.clone()),
        release_manifest_output: Some(tmp_dir.path().into()),
        date_override: Some(OffsetDateTime::UNIX_EPOCH),
        current_release_versions_manifest: Some(current_versions_manifest_path),
        previous_release_versions_manifest: None,
        smithy_rs_location: Some(tmp_dir.path().into()),
        aws_sdk_rust_location: None,
    })
    .unwrap();

    let dot_example = fs::read_to_string(dot_changelog_path.join(".example")).unwrap();
    let dest = fs::read_to_string(&dest_path).unwrap();
    let release_manifest = fs::read_to_string(&release_manifest_path).unwrap();

    pretty_assertions::assert_str_eq!(EXAMPLE_ENTRY, dot_example);
    pretty_assertions::assert_str_eq!(
        r#"<!-- Do not manually edit this file. Use the `changelogger` tool. -->
January 1st, 1970
=================
**New this release:**
- (all, [smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234), @another-dev) Another change

**Contributors**
Thank you for your contributions! ❤
- @another-dev ([smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234))

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
  "tagName": "release-1970-01-01.2",
  "name": "January 1st, 1970",
  "body": "**New this release:**\n- (all, [smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234), @another-dev) Another change\n\n**Contributors**\nThank you for your contributions! ❤\n- @another-dev ([smithy-rs#1234](https://github.com/smithy-lang/smithy-rs/issues/1234))\n\n**Crate Versions**\n<details>\n<summary>Click to expand to view crate versions...</summary>\n\n|Crate|Version|\n|-|-|\n|aws-config|0.54.1|\n|aws-sdk-accessanalyzer|0.24.0|\n|aws-smithy-async|0.54.1|\n</details>\n\n",
  "prerelease": false
}"#,
        release_manifest
    );
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use once_cell::sync::Lazy;
use regex::Regex;
use sdk_sync::init_tracing;
use sdk_sync::sync::gen::CodeGenSettings;
use sdk_sync::sync::{Sync, BOT_EMAIL, BOT_NAME};
use smithy_rs_tool_common::git::{Git, GitCLI};
use smithy_rs_tool_common::shell::handle_failure;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

static INIT_TRACING: Lazy<bool> = Lazy::new(|| {
    init_tracing();
    true
});

#[track_caller]
fn assert_file_exists<P: AsRef<Path>>(path: P) {
    if !path.as_ref().exists() {
        panic!("Path {:?} doesn't exist when it should", path.as_ref());
    }
}

#[track_caller]
fn assert_no_file<P: AsRef<Path>>(path: P) {
    if path.as_ref().exists() {
        panic!("Path {:?} exists when it shouldn't", path.as_ref());
    }
}

#[track_caller]
fn assert_file_contents<P: AsRef<Path>>(path: P, contents: &str) {
    let actual = fs::read_to_string(path.as_ref()).unwrap();
    pretty_assertions::assert_str_eq!(contents, actual);
}

#[test]
fn test_without_model_changes() {
    assert!(*INIT_TRACING);

    // Create a temp directory with a bunch of fake repos
    let tmp_dir = TempDir::new().unwrap();
    let create_test_workspace_path = std::env::current_dir()
        .expect("current_dir")
        .join("tests/create-test-workspace")
        .canonicalize()
        .expect("canonicalize");
    let mut command = Command::new(create_test_workspace_path);
    command.current_dir(&tmp_dir);
    let output = command.output().expect("run create-test-workspace");
    handle_failure("create-test-workspace", &output).expect("successfully create workspace");

    // Get initial commit hashes
    let aws_sdk_rust = GitCLI::new(&tmp_dir.as_ref().join("aws-sdk-rust")).unwrap();
    let smithy_rs = GitCLI::new(&tmp_dir.as_ref().join("smithy-rs")).unwrap();
    let sdk_start_revision = aws_sdk_rust.get_head_revision().unwrap();
    let smithy_rs_start_revision = smithy_rs.get_head_revision().unwrap();

    // Assert pre-conditions
    assert_file_exists(aws_sdk_rust.path().join(".git"));
    assert_file_exists(aws_sdk_rust.path().join(".handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/endpoints.json"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/fake_content"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("some_handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("versions.toml"));
    assert_file_exists(aws_sdk_rust.path().join("examples/s3/fake_content"));
    assert_no_file(aws_sdk_rust.path().join("examples/Cargo.toml"));

    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/s3.json"),
        "Ancient S3 model\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/fake_content"),
        "Some S3 client code\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("examples/s3/fake_content"),
        "Some S3 example\n",
    );

    // Run the sync
    let sync = Sync::new(
        &tmp_dir.path().join("aws-doc-sdk-examples"),
        &tmp_dir.path().join("aws-sdk-rust"),
        &tmp_dir.path().join("smithy-rs"),
        CodeGenSettings {
            aws_models_path: Some(tmp_dir.path().join("aws-sdk-rust").join("aws-models")),
            ..Default::default()
        },
    )
    .expect("create sync success");
    sync.sync().expect("sync success");

    // Get the new SDK commits and verify them
    let sdk_new_revisions = aws_sdk_rust
        .rev_list("HEAD", sdk_start_revision.as_ref(), None)
        .unwrap();
    let sdk_commits: Vec<_> = sdk_new_revisions
        .into_iter()
        .map(|commit_hash| aws_sdk_rust.show(commit_hash.as_ref()).unwrap())
        .collect();

    assert_eq!(2, sdk_commits.len());

    assert_eq!(BOT_NAME, sdk_commits[0].author_name);
    assert_eq!(BOT_EMAIL, sdk_commits[0].author_email);
    assert_eq!(
        "[examples] Sync SDK examples from `awsdocs/aws-doc-sdk-examples`",
        sdk_commits[0].message_subject
    );
    assert!(
        {
            let re = Regex::new(r"Includes commit\(s\):\s+[a-z0-9]{40}\s+Co-authored-by: Test Dev <testdev@example.com>").unwrap();
            re.is_match(&sdk_commits[0].message_body)
        },
        "commit body didn't match regex"
    );

    assert_eq!("Another Dev", sdk_commits[1].author_name);
    assert_eq!("anotherdev@example.com", sdk_commits[1].author_email);
    assert_eq!(
        "[smithy-rs] Update S3 to do more",
        sdk_commits[1].message_subject
    );
    assert_eq!("", sdk_commits[1].message_body);

    // Verify SDK files
    assert_file_exists(aws_sdk_rust.path().join(".git"));
    assert_file_exists(aws_sdk_rust.path().join(".handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/endpoints.json"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("some_handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("versions.toml"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/fake_content"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("examples/s3/fake_content"));
    assert_no_file(aws_sdk_rust.path().join("examples/Cargo.toml"));

    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/s3.json"),
        "Ancient S3 model\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/fake_content"),
        "Some updated S3 client code\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("examples/s3/fake_content"),
        "Some modified S3 example\n",
    );

    // Verify smithy-rs had no changes
    assert_eq!(
        smithy_rs_start_revision,
        smithy_rs.get_head_revision().unwrap()
    );
}

#[test]
fn test_with_model_changes() {
    assert!(*INIT_TRACING);

    // Create a temp directory with a bunch of fake repos
    let tmp_dir = TempDir::new().unwrap();
    let create_test_workspace_path = std::env::current_dir()
        .expect("current_dir")
        .join("tests/create-test-workspace")
        .canonicalize()
        .expect("canonicalize");
    let mut command = Command::new(create_test_workspace_path);
    command.arg("--with-model-changes");
    command.current_dir(&tmp_dir);
    let output = command.output().expect("run create-test-workspace");
    handle_failure("create-test-workspace", &output).expect("successfully create workspace");

    // Get initial commit hashes
    let aws_sdk_rust = GitCLI::new(&tmp_dir.as_ref().join("aws-sdk-rust")).unwrap();
    let smithy_rs = GitCLI::new(&tmp_dir.as_ref().join("smithy-rs")).unwrap();
    let sdk_start_revision = aws_sdk_rust.get_head_revision().unwrap();
    let smithy_rs_start_revision = smithy_rs.get_head_revision().unwrap();

    // Assert pre-conditions
    assert_file_exists(aws_sdk_rust.path().join(".git"));
    assert_file_exists(aws_sdk_rust.path().join(".handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/endpoints.json"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/fake_content"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("some_handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("versions.toml"));
    assert_file_exists(aws_sdk_rust.path().join("examples/s3/fake_content"));
    assert_no_file(aws_sdk_rust.path().join("examples/Cargo.toml"));

    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/s3.json"),
        "Ancient S3 model\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/fake_content"),
        "Some S3 client code\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("examples/s3/fake_content"),
        "Some S3 example\n",
    );

    // Run the sync
    let sync = Sync::new(
        &tmp_dir.path().join("aws-doc-sdk-examples"),
        &tmp_dir.path().join("aws-sdk-rust"),
        &tmp_dir.path().join("smithy-rs"),
        CodeGenSettings {
            aws_models_path: Some(tmp_dir.path().join("aws-sdk-rust").join("aws-models")),
            ..Default::default()
        },
    )
    .expect("create sync success");
    sync.sync().expect("sync success");

    // Get the new SDK commits and verify them
    let sdk_new_revisions = aws_sdk_rust
        .rev_list("HEAD", sdk_start_revision.as_ref(), None)
        .unwrap();
    let sdk_commits: Vec<_> = sdk_new_revisions
        .into_iter()
        .map(|commit_hash| aws_sdk_rust.show(commit_hash.as_ref()).unwrap())
        .collect();

    assert_eq!(3, sdk_commits.len(), "commits: {:#?}", sdk_commits);

    assert_eq!(BOT_NAME, sdk_commits[0].author_name);
    assert_eq!(BOT_EMAIL, sdk_commits[0].author_email);
    assert_eq!(
        "[examples] Sync SDK examples from `awsdocs/aws-doc-sdk-examples`",
        sdk_commits[0].message_subject
    );
    assert!(
        {
            let re = Regex::new(r"Includes commit\(s\):\s+[a-z0-9]{40}\s+Co-authored-by: Test Dev <testdev@example.com>").unwrap();
            re.is_match(&sdk_commits[0].message_body)
        },
        "commit body didn't match regex"
    );

    assert_eq!(BOT_NAME, sdk_commits[1].author_name);
    assert_eq!(BOT_EMAIL, sdk_commits[1].author_email);
    assert_eq!("Update SDK models", sdk_commits[1].message_subject);
    assert_eq!("", sdk_commits[1].message_body);

    assert_eq!("Another Dev", sdk_commits[2].author_name);
    assert_eq!("anotherdev@example.com", sdk_commits[2].author_email);
    assert_eq!(
        "[smithy-rs] Update S3 to do more",
        sdk_commits[2].message_subject
    );
    assert_eq!("", sdk_commits[2].message_body);

    // Verify SDK files
    assert_file_exists(aws_sdk_rust.path().join(".git"));
    assert_file_exists(aws_sdk_rust.path().join(".handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/endpoints.json"));
    assert_file_exists(aws_sdk_rust.path().join("aws-models/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("some_handwritten"));
    assert_file_exists(aws_sdk_rust.path().join("versions.toml"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/fake_content"));
    assert_file_exists(aws_sdk_rust.path().join("sdk/s3/s3.json"));
    assert_file_exists(aws_sdk_rust.path().join("examples/s3/fake_content"));
    assert_no_file(aws_sdk_rust.path().join("examples/Cargo.toml"));

    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/s3.json"),
        "Updated S3 model\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("sdk/verify-endpoints.json"),
        "Updated endpoints.json\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("sdk/s3/fake_content"),
        "Some updated S3 client code\n",
    );
    assert_file_contents(
        aws_sdk_rust.path().join("examples/s3/fake_content"),
        "Some modified S3 example\n",
    );

    // Verify smithy-rs had no changes
    assert_eq!(
        smithy_rs_start_revision,
        smithy_rs.get_head_revision().unwrap()
    );
}

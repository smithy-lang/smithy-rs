/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{env::current_dir, path::PathBuf, process::Command, str::FromStr};

/// Tests whether de-serialization feature for number is properly feature gated
#[test]
fn feature_gate_test_for_number_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Number");
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for number is properly feature gated
#[test]
fn feature_gate_test_for_number_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Number");
    ser_test(&cargo_project_path);
}

/// Tests whether de-serialization feature for document is properly feature gated
#[test]
fn feature_gate_test_for_document_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Document");
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for document is properly feature gated
#[test]
fn feature_gate_test_for_document_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Document");
    ser_test(&cargo_project_path);
}

/// Tests whether de-serialization feature for datetime is properly feature gated
#[test]
fn feature_gate_test_for_datetime_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("DateTime");
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for datetime is properly feature gated
#[test]
fn feature_gate_test_for_datetime_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("DateTime");
    ser_test(&cargo_project_path);
}

/// Tests whether de-serialization feature for blob is properly feature gated
#[test]
fn feature_gate_test_for_blob_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Blob");
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for blob is properly feature gated
#[test]
fn feature_gate_test_for_blob_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Blob");
    ser_test(&cargo_project_path);
}

/// tests whether serialization features are proplery feature
fn ser_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features to compile a code that requries serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize to compile a code that requries serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize to compile a code that requries serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check to compile a code that requries serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize to compile a code that requries serialization feature is needed.
    // it is expected to fail to compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-deserialize to compile a code that requries serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);
}

fn de_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features to compile a code that requries de-serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize to compile a code that requries de-serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize to compile a code that requries de-serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check to compile a code that requries de-serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize to compile a code that requries de-serialization feature is needed.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize to compile a code that requries de-serialization feature is needed.
    // it is expected to fail to compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);
}

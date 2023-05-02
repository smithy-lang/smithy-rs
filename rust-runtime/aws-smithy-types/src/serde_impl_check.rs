/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{env::current_dir, path::PathBuf, process::Command};

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


// create directory and files for testing
fn create_cargo_dir(datatype: &str, enable_ser: bool, enable_deser: bool) -> PathBuf {
    let deser = include_str!("../test_data/template/ser.rs");
    let ser = include_str!("../test_data/template/deser.rs");
    
    let cargo = include_str!("../test_data/template/Cargo.toml").replace(
        r#"aws-smithy-types = { path = "./" }"#,
        &format!(
            "aws-smithy-types = {{ path = {:#?} }}",
            current_dir().unwrap().to_str().unwrap().to_string()
        ),
    );

    let base_path = PathBuf::from_str("/tmp/smithy-rust-test")
        .unwrap()
        .join(datatype);
    let src_path = base_path.join("src");
    std::fs::create_dir_all(&base_path).unwrap();
    std::fs::create_dir_all(&src_path).unwrap();
    base_path
}


fn ser_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to fail to compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-deserialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);
}

fn de_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"]);
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-serialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to successfully compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is expected to fail to compile.
    let cmd = Command::new("bash")
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable");
    let is_success = cmd.spawn().unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);
}

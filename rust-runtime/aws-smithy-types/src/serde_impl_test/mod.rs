/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{env::current_dir, path::PathBuf, process::Command, str::FromStr};
mod blob;
mod date_time;
mod document;
mod number;

#[derive(Debug)]
pub(crate) enum Target {
    Ser,
    De,
}

// create directory and files for testing
pub(crate) fn create_cargo_dir(datatype: &str, target: Target) -> PathBuf {
    let base_path = PathBuf::from_str("/tmp/smithy-rust-test/")
        .unwrap()
        .join(format!("{target:#?}"))
        .join(datatype);
    let src_path = base_path.join("src");

    // create temp directory
    std::fs::create_dir_all(&base_path).unwrap();
    std::fs::create_dir_all(&src_path).unwrap();

    // write cargo
    {
        let cargo = include_str!("../../test_data/template/Cargo.toml").replace(
            r#"aws-smithy-types = { path = "./" }"#,
            &format!(
                "aws-smithy-types = {{ path = {:#?} }}",
                current_dir().unwrap().to_str().unwrap().to_string()
            ),
        );
        std::fs::write(&base_path.join("Cargo.toml"), cargo).unwrap();
    };

    // write main.rs
    let ser = include_str!("../../test_data/template/ser");
    let deser = include_str!("../../test_data/template/deser");
    let place_holder = "$PLACE_HOLDER$";
    let contents = match target {
        Target::De => deser.replace(place_holder, datatype),
        Target::Ser => {
            let s = match datatype {
                "Blob" => "Blob::new([1, 2,3])",
                "Number" => "Number::PosInt(1)",
                "Document" => "Document::from(0)",
                "DateTime" => "DateTime::from_secs(0)",
                _ => panic!(),
            };
            ser.replace(place_holder, s)
        }
    };
    std::fs::write(&src_path.join("main.rs"), contents).unwrap();

    base_path
}

pub(crate) fn ser_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-serialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-deserialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-serialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is  expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);

    // runs cargo check --features serde-deserialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);
}

pub(crate) fn de_test(cargo_project_path: &PathBuf) {
    // runs cargo check --all-features without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --all-features"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-serialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-deserialize without "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-serialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is not expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-serialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, false);

    // runs cargo check --features serde-deserialize with "--cfg aws_sdk_unstable" enabled.
    // the code that it compiles require de-serialization feature.
    // it is  expected to compile.
    let mut cmd = Command::new("bash");

    let child = cmd
        .current_dir(&cargo_project_path)
        .args(["-c", "cargo check --features serde-deserialize"])
        .env("RUSTFLAGS", "--cfg aws_sdk_unstable")
        .spawn();

    let is_success = child.unwrap().wait().unwrap().success();
    assert_eq!(is_success, true);
}

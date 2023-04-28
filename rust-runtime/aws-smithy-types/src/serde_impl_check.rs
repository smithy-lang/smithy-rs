/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{env::current_dir, path::PathBuf, process::Command, str::FromStr};

/// ensures serde features is not enabled when features are not enabled
/// Checks whether features are properly gated behind aws_sdk_unstable
fn base(dt: &str) {
    // data type
    let data_type = format!("aws_smithy_types::{dt}");
    // commands 2 run
    let array = [
        ("cargo build --all-features", [true; 2], false),
        ("cargo build --features serde-serialize", [true; 2], false),
        ("cargo build --features serde-deserialize", [true; 2], false),
        // checks if features are properly gated behind serde-serialize/deserialize
        ("cargo build", [true; 2], true),
        (
            "cargo build --features serde-serialize",
            [false, true],
            true,
        ),
        (
            "cargo build --features serde-deserialize",
            [true, false],
            true,
        ),
    ];

    // const
    let replace_data_type = "ReplaceDataType";

    // templates
    let deser = include_str!("../test_data/template/ser.rs");
    let ser = include_str!("../test_data/template/deser.rs");
    let cargo = include_str!("../test_data/template/Cargo.toml").replace(
        r#"aws-smithy-types = { path = "./" }"#,
        &format!(
            "aws-smithy-types = {{ path = {:#?} }}",
            current_dir().unwrap().to_str().unwrap().to_string()
        ),
    );

    // paths

    let base_path = PathBuf::from_str("/tmp/smithy-rust-test").unwrap().join(dt);
    let cmdpath = base_path.join("cmd.sh");
    let src_path = base_path.join("src");
    let main_path = src_path.join("main.rs");

    for (cmd_txt, [check_deser, check_ser], env) in array {
        std::fs::create_dir_all(&base_path).unwrap();
        std::fs::create_dir_all(&src_path).unwrap();

        std::fs::write(&cmdpath, cmd_txt).unwrap();
        let func = || {
            let mut cmd = Command::new("bash");
            cmd.current_dir(&base_path);
            cmd.arg(&cmdpath.to_str().unwrap().to_string());
            if env {
                cmd.env("RUSTFLAGS", "--cfg aws_sdk_unstable");
            }

            let check = cmd.spawn().unwrap().wait_with_output().unwrap();

            assert!(
                !check.status.success(),
                "{:#?}",
                (cmd, cmd_txt, check_ser, check_deser, env, dt)
            );
        };

        std::fs::write(&base_path.join("Cargo.toml"), &cargo).unwrap();

        if check_ser {
            std::fs::write(&main_path, ser.replace(replace_data_type, &data_type)).unwrap();
            func();
        }
        if check_deser {
            std::fs::write(&main_path, deser.replace(replace_data_type, &data_type)).unwrap();
            func();
        }
    }
}

#[test]
fn number() {
    base("Number");
}

#[test]
fn blob() {
    base("Blob");
}

#[test]
fn document() {
    base("Document");
}

#[test]
fn date_time() {
    base("DateTime");
}

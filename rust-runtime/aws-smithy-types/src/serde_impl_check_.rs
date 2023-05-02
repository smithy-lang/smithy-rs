/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{env::current_dir, path::PathBuf, process::Command, str::FromStr};

const PLACE_HOLDER: &str = "$PLACE_HOLDER$";
struct CargoCmd {
    cargo_command: &'static str,
    is_ser_compilable: bool,
    is_deser_compilable: bool,
    enable_aws_sdk_unstable: bool,
}

impl CargoCmd {
    const ALL_FEATURE: &str = "cargo check --all-features";
    const SERDE_SERIALIZE: &str = "cargo check --features serde-serialize";
    const SERDE_DESERIALIZE: &str = "cargo check --features serde-serialize";
}

#[test]
fn print() {
    let array = [
        ("cargo check --all-features", [true; 2], false),
        ("cargo check --features serde-serialize", [true; 2], false),
        ("cargo check --features serde-deserialize", [true; 2], false),
        // checks if features are properly gated behind serde-serialize/deserialize
        ("cargo check", [true; 2], true),
        (
            "cargo check --features serde-serialize",
            [false, true],
            true,
        ),
        (
            "cargo check --features serde-deserialize",
            [true, false],
            true,
        ),
    ];

    let mut ser_stack = "".to_string();
    let mut de_stack = "".to_string();
    for i in array {
        let [ser, de] = i.1;
        let func = |ser, check2| {
            let func = |b| if b { "not" } else { "" };
            let msg = if check2 {
                "de-serialization feature"
            } else {
                "serialization feature"
            };
            let env = if i.2 {
                r#".env("RUSTFLAGS", "--cfg aws_sdk_unstable")"#
            } else {
                ""
            };
            let s = format! { r#"
// runs {cmd} {env} "--cfg aws_sdk_unstable" enabled.
// the code that it compiles require {msg}.
// it is {ser} expected to compile.
let mut cmd = Command::new("bash");

let child = cmd.current_dir(&cargo_project_path)
    .args(["-c", {cmd:#?}, "&>", "/dev/null"])
    {env2}
    .spawn();

let is_success = child.unwrap().wait().unwrap().success();
assert_eq!(is_success, {ser_bool});
            "#, 
            cmd = i.0,
            ser = func(ser),
            ser_bool = ser,
            env2 = env,
            env = if i.2 {
                "with"
            } else {
                "without"
            }
            };

            return s;
        };

        ser_stack.push_str(&func(ser, false));
        de_stack.push_str(&func(de, true));
    }

    let datatypes = ["Number", "Document", "DateTime", "Blob"];
    for dt in datatypes {
        println!(
            r#"
        /// Tests whether de-serialization feature for {dt} is properly feature gated
        #[test]
        fn feature_gate_test_for_{dt}_deserialization() {{
            // create files
            let cargo_project_path = create_cargo_dir({dt2:?});
            de_test(&cargo_project_path);
        }}

        /// Tests whether serialization feature for {dt} is properly feature gated
        #[test]
        fn feature_gate_test_for_{dt}_serialization() {{
            // create files
            let cargo_project_path = create_cargo_dir({dt2:?});
            ser_test(&cargo_project_path);
        }}
        "#,
            dt = dt.to_lowercase(),
            dt2 = dt,
        );
    }
    println!(
        "fn ser_test(cargo_project_path: &PathBuf) {{{}}}",
        ser_stack
    );
    println!("fn de_test(cargo_project_path: &PathBuf) {{{}}}", de_stack);
}

/// ensures serde features is not enabled when features are not enabled
/// Checks whether features are properly gated behind aws_sdk_unstable
fn base(dt: &str) {
    // data type
    let data_type = format!("aws_smithy_types::{dt}");
    // commands 2 run
    let array = [
        ("cargo check --all-features", [true; 2], false),
        ("cargo check --features serde-serialize", [true; 2], false),
        ("cargo check --features serde-deserialize", [true; 2], false),
        // checks if features are properly gated behind serde-serialize/deserialize
        ("cargo check", [true; 2], true),
        (
            "cargo check --features serde-serialize",
            [false, true],
            true,
        ),
        (
            "cargo check --features serde-deserialize",
            [true, false],
            true,
        ),
    ];

    // templates
    let deser = include_str!("../test_data/template/ser");
    let ser = include_str!("../test_data/template/deser");
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
        let func = |predicted| {
            let mut cmd = Command::new("bash");
            cmd.current_dir(&base_path);
            cmd.arg(&cmdpath.to_str().unwrap().to_string());
            if env {
                cmd.env("RUSTFLAGS", "--cfg aws_sdk_unstable");
            }
            println!("{:#?}", cmd);
            //let check = cmd.spawn().unwrap().wait_with_output().unwrap();
            /*
            assert_eq!(
                check.status.success(),
                predicted,
                "{:#?}",
                (cmd, cmd_txt, check_ser, check_deser, env, dt)
            );
             */
        };

        std::fs::write(&base_path.join("Cargo.toml"), &cargo).unwrap();

        std::fs::write(&main_path, ser.replace(PLACE_HOLDER, &data_type)).unwrap();
        func(check_ser);
        std::fs::write(&main_path, deser.replace(PLACE_HOLDER, &data_type)).unwrap();
        func(check_deser);
    }
}

// setup a cargo project for the test
fn create_cargo_dir(datatype: &str) -> PathBuf {
    let deser = include_str!("../test_data/template/ser");
    let ser = include_str!("../test_data/template/deser");
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

#[test]
fn number() {
    let cargo_project_path = create_cargo_dir("Number");

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

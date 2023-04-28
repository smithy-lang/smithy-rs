use std::{path::PathBuf, process::Command, str::FromStr};

/// ensures serde features is not enabled when features are not enabled
/// Checks whether features are properly gated behind aws_sdk_unstable
fn base(dt: &str) {
    // data type
    let data_type = format!("aws-smithy-types::{dt}");
    // commands 2 run
    let array = [
        ("cargo build --all-features", [true; 2], false),
        ("cargo build --features serde-serialize", [true; 2], false),
        ("cargo build --features serde-deserialize", [true; 2], false),
        // checks if features are properly gated behind serde-serialize/deserialize
        ("cargo build", [true; 2], true),
        (
            "cargo build --features serde-serialize",
            [true; 2], true,
        ),
        (
            "cargo build --features serde-deserialize",
            [true; 2], true,
        ),
    ];

    // const
    let replace_data_type = "ReplaceDataType";

    // templates
    let deser = include_str!("../test_data/template/ser.rs");
    let ser = include_str!("../test_data/template/deser.rs");
    let cargo = include_str!("../test_data/template/Cargo.toml");

    // paths
    let cmdpath = "/tmp/cmd.sh";
    let base_path = PathBuf::from_str("/tmp/").unwrap().join(dt);
    let src_path = base_path.join("src");
    let main_path = src_path.join("main.rs");

    for (cmd, b, env) in array {
        std::fs::write(cmdpath, cmd).unwrap();
        let func = || {
            let mut cmd = Command::new("bash");
            cmd.arg(cmdpath);
            if env {
                cmd.env("RUSTFLAGS", "--cfg aws_sdk_unstable");
            }

            let check = cmd.spawn()
                .unwrap()
                .wait()
                .unwrap()
                .success();
            assert!(!check);
        };

        std::fs::create_dir_all(&base_path).unwrap();
        std::fs::create_dir_all(&src_path).unwrap();
        std::fs::write(&base_path.join("Cargo.toml"), cargo).unwrap();

        if b[0] {
            std::fs::write(&main_path, ser.replace(replace_data_type, &data_type)).unwrap();
            func();
        }
        if b[1] {
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

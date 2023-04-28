use std::process::Command;

/// ensures serde features is not enabled when features are not enabled
fn base(dt: &str) {
    let data_type = format!("aws-smithy-types::{dt}");
    let array = [
        "cargo build --all-features",
        "cargo build --features serde-serialize",
        "cargo build --features serde-deserialize",
    ];
    let cmdpath = "/tmp/cmd.sh";
    let deser = include_str!("../test_data/template/ser.rs");
    let ser = include_str!("../test_data/template/deser.rs");
    let cargo = include_str!("../test_data/template/Cargo.toml");
    let replace_data_type = "ReplaceDataType";
    
    for cmd in array {
        std::fs::write(cmdpath, cmd).unwrap();
        let func = || {
            let check = Command::new("bash")
                .arg(cmdpath)
                .spawn()
                .unwrap()
                .wait()
                .unwrap()
                .success();
            assert!(!check);
        };
        std::fs::create_dir("/tmp/test").unwrap();
        std::fs::create_dir("/tmp/test/src").unwrap();
        std::fs::write(
            "/tmp/test/Cargo.toml",
            cargo,
        )
        .unwrap();
        std::fs::write(
            "/tmp/test/src/main.rs",
            ser.replace(replace_data_type, &data_type),
        )
        .unwrap();
        func();
        std::fs::write(
            "/tmp/test/src/main.rs",
            deser.replace(replace_data_type, &data_type),
        )
        .unwrap();
        func();
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
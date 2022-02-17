use fs_extra::{copy_items, dir::CopyOptions};
use std::{fs, path::Path, process::Command};

fn main() {
    let src_dir = Path::new("../../../../");
    let command = Command::new("./gradlew")
        .current_dir(src_dir)
        .arg(":codegen-server-test:assemble")
        .output()
        .unwrap();
    println!(
        "Gradle output: {}\nGradle error:: {}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    let mut options = CopyOptions::new();
    options.overwrite = true;
    let dest_dir = Path::new("pokemon-sdk");
    if !dest_dir.exists() {
        fs::create_dir(&dest_dir).unwrap_or_else(|_| panic!("Unable to create directory {}", dest_dir.display()));
    }
    copy_items(
        &[
            src_dir.join("codegen-server-test/build/smithyprojections/codegen-server-test/pokemon-sdk/rust-server-codegen/src"),
            src_dir.join("codegen-server-test/build/smithyprojections/codegen-server-test/pokemon-sdk/rust-server-codegen/Cargo.toml"),
        ],
        dest_dir,
        &options,
    ).unwrap_or_else(|e| panic!("Unable to copy codegenerated Pokémon service: {}", e));
}

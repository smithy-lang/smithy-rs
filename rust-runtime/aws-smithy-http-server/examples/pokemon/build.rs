/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use fs_extra::{copy_items, dir::CopyOptions};
use std::{path::Path, process::Command};

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
    let command = Command::new("./gradlew")
        .current_dir(src_dir)
        .arg(":codegen-test:assemble")
        .output()
        .unwrap();
    println!(
        "Gradle output: {}\nGradle error:: {}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    let mut options = CopyOptions::new();
    options.overwrite = true;
    let dest_dir = Path::new("pokemon-service-sdk");
    copy_items(
        &[
            src_dir.join("codegen-server-test/build/smithyprojections/codegen-server-test/pokemon-service-sdk/rust-server-codegen/src"),
            src_dir.join("codegen-server-test/build/smithyprojections/codegen-server-test/pokemon-service-sdk/rust-server-codegen/Cargo.toml"),
        ],
        dest_dir,
        &options,
    ).unwrap_or_else(|e| panic!("Unable to copy codegenerated Pokémon service: {}", e));
    let dest_dir = Path::new("pokemon-service-client");
    copy_items(
        &[
            src_dir.join("codegen-test/build/smithyprojections/codegen-test/pokemon-service-client/rust-codegen/src"),
            src_dir.join(
                "codegen-test/build/smithyprojections/codegen-test/pokemon-service-client/rust-codegen/Cargo.toml",
            ),
        ],
        dest_dir,
        &options,
    )
    .unwrap_or_else(|e| panic!("Unable to copy codegenerated Pokémon client: {}", e));
}

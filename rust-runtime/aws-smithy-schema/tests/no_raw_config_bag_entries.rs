/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `aws-smithy-schema` must not define its own `ConfigBag` entries.
//!
//! A `ConfigBag` entry is keyed by its Rust type, and a type defined here gets a new identity with
//! every incompatible release of this crate. A producer and consumer built against different
//! releases would then silently miss each other's values. Cross-crate entries therefore live in
//! `aws-smithy-runtime-api`: either as a small stable type (re-exported here), or as a versioned
//! slot such as `ConfiguredProtocol` whose payload implements `ConfigPayloadFor`.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable source dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn schema_does_not_implement_storable() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(
        !files.is_empty(),
        "no sources found under {}",
        src.display()
    );

    let offenders: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let text = fs::read_to_string(file).expect("readable source");
            text.lines()
                .enumerate()
                .filter(|(_, line)| line.contains("Storable for"))
                .map(|(n, line)| format!("{}:{}: {}", file.display(), n + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "aws-smithy-schema implements `Storable`, which ties a ConfigBag key to this crate's \
         version. Define the entry in aws-smithy-runtime-api instead (a stable type, or a \
         versioned slot plus `ConfigPayloadFor`):\n{}",
        offenders.join("\n"),
    );
}

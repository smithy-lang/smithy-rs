/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Agreement with Java Smithy 1.74.0 on checked-in fixtures. See `fixtures/java/README.md`.

use aws_smithy_lang::Model;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java")
}

fn outcomes() -> Vec<(String, bool)> {
    let text = std::fs::read_to_string(fixtures().join("expected/outcomes.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    document["outcomes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, outcome)| (name.clone(), outcome["accepted"].as_bool().unwrap()))
        .collect()
}

fn load(path: &Path) -> Result<Model, aws_smithy_lang::LoadError> {
    Model::from_json_file(path)
}

#[test]
fn every_fixture_has_an_outcome() {
    let recorded: Vec<_> = outcomes().into_iter().map(|(name, _)| name).collect();
    let mut on_disk = Vec::new();
    for group in ["valid", "invalid"] {
        for entry in std::fs::read_dir(fixtures().join(group)).unwrap() {
            let path = entry.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            on_disk.push(format!("{group}/{stem}"));
        }
    }
    on_disk.sort();
    assert_eq!(recorded, on_disk, "regenerate expected/ (see README.md)");
}

#[test]
fn outcomes_match_java() {
    for (name, accepted) in outcomes() {
        assert_eq!(
            name.starts_with("valid/"),
            accepted,
            "{name}: fixture is in the wrong group"
        );
        let result = load(&fixtures().join(format!("{name}.json")));
        match (accepted, result) {
            (true, Err(err)) => panic!(
                "{name}: Java accepts but Rust rejects: {:?}",
                err.diagnostics()
            ),
            (false, Ok(_)) => panic!("{name}: Java rejects but Rust accepts"),
            _ => {}
        }
    }
}

#[test]
fn java_serialization_is_equivalent() {
    for (name, accepted) in outcomes() {
        if !accepted {
            continue;
        }
        let stem = name.trim_start_matches("valid/");
        let original = load(&fixtures().join(format!("{name}.json"))).unwrap();
        let java = load(&fixtures().join(format!("expected/{stem}.json")))
            .unwrap_or_else(|e| panic!("{name}: Java output does not load: {:?}", e.diagnostics()));
        assert!(
            original.equivalent(&java),
            "{name}: Java output is not equivalent"
        );
    }
}

#[test]
fn all_shape_kinds_fixture_is_complete() {
    let model = load(&fixtures().join("valid/all_shape_kinds.json")).unwrap();
    let mut kinds: Vec<_> = model.non_prelude_shapes().map(|s| s.shape_type()).collect();
    kinds.sort();
    kinds.dedup();
    let mut every: Vec<_> = aws_smithy_lang::ShapeType::ALL
        .into_iter()
        .filter(|t| *t != aws_smithy_lang::ShapeType::Member)
        .collect();
    every.sort();
    assert_eq!(kinds, every);
}

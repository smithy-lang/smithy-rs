/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Agreement with Java Smithy 1.74.0 on checked-in fixtures. See `fixtures/java/README.md`.

use aws_smithy_lang::diagnostic::DiagnosticCode;
use aws_smithy_lang::Model;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java")
}

/// A fixture's recorded Java outcome.
struct Outcome {
    name: String,
    accepted: bool,
    /// Java validation event IDs with ERROR or DANGER severity.
    errors: Vec<String>,
}

fn outcomes() -> Vec<Outcome> {
    let text = std::fs::read_to_string(fixtures().join("expected/outcomes.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    document["outcomes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, outcome)| Outcome {
            name: name.clone(),
            accepted: outcome["accepted"].as_bool().unwrap(),
            errors: outcome["errors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_owned())
                .collect(),
        })
        .collect()
}

/// The Rust diagnostic code corresponding to a Java validation event ID.
///
/// Java reports most loader-level problems under the generic `Model` event, so that ID is
/// resolved per fixture. Unknown IDs fail the test so that regenerated fixtures cannot silently
/// lose their reason check.
fn rust_code(fixture: &str, java_event: &str) -> DiagnosticCode {
    use DiagnosticCode::*;
    match java_event {
        "EnumShape" | "Union" | "ResourceCycle" => InvalidShape,
        "Target" | "UnitType" => InvalidTarget,
        "Target.UnresolvedShape" => UnresolvedReference,
        "ShapeIdConflict" => CaseConflict,
        "Model" => match fixture {
            "invalid/enum_member_not_unit" => InvalidTarget,
            "invalid/invalid_shape_id" => InvalidShapeId,
            "invalid/trait_not_definition" => InvalidTrait,
            "invalid/unknown_shape_type" => UnknownShapeType,
            other => panic!("{other}: map Java's generic `Model` event to a Rust code"),
        },
        other => panic!("{fixture}: unmapped Java event `{other}`; add it to `rust_code`"),
    }
}

fn load(path: &Path) -> Result<Model, aws_smithy_lang::LoadError> {
    Model::from_json_file(path)
}

#[test]
fn every_fixture_has_an_outcome() {
    let recorded: Vec<_> = outcomes().into_iter().map(|o| o.name).collect();
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
    for Outcome {
        name,
        accepted,
        errors,
    } in outcomes()
    {
        assert_eq!(
            name.starts_with("valid/"),
            accepted,
            "{name}: fixture is in the wrong group"
        );
        assert_eq!(accepted, errors.is_empty(), "{name}: inconsistent outcome");
        let result = load(&fixtures().join(format!("{name}.json")));
        match (accepted, result) {
            (true, Err(err)) => panic!(
                "{name}: Java accepts but Rust rejects: {:?}",
                err.diagnostics()
            ),
            (false, Ok(_)) => panic!("{name}: Java rejects but Rust accepts"),
            (false, Err(err)) => {
                // Rust must reject for the same reasons Java does, not merely reject.
                let expected: HashSet<_> = errors.iter().map(|e| rust_code(&name, e)).collect();
                let actual: HashSet<_> = err.diagnostics().iter().map(|d| d.code()).collect();
                assert_eq!(
                    actual,
                    expected,
                    "{name}: Java events {errors:?}, Rust diagnostics {:?}",
                    err.diagnostics()
                );
            }
            (true, Ok(_)) => {}
        }
    }
}

#[test]
fn java_serialization_is_equivalent() {
    for Outcome { name, accepted, .. } in outcomes() {
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
        .iter()
        .copied()
        .filter(|t| *t != aws_smithy_lang::ShapeType::Member)
        .collect();
    every.sort();
    assert_eq!(kinds, every);
}

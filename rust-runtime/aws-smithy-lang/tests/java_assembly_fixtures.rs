/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Agreement with the Smithy reference implementation on assembler steps that file fixtures
//! cannot express: shapes built in code, models, metadata, and traits added in code. See
//! `fixtures/java/README.md`.

use aws_smithy_lang::diagnostic::{DiagnosticCode, Severity, SourceLocation};
use aws_smithy_lang::{prelude, AstWriter, Model, ModelAssembler, Node, NodeObject, ShapeId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java")
}

/// A case's recorded outcome.
struct Outcome {
    name: String,
    accepted: bool,
    /// Event IDs with ERROR or DANGER severity.
    errors: Vec<String>,
    /// Source file names of selected shapes, members, and traits (`SHAPE_ID@TRAIT_ID`); an
    /// empty name for one created in code.
    locations: Vec<(String, String)>,
}

fn outcomes() -> Vec<Outcome> {
    let path = fixtures().join("expected/assembly/outcomes.json");
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
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
            locations: outcome
                .get("locations")
                .and_then(|l| l.as_object())
                .map(|locations| {
                    locations
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// Converts a `case.json` value to a [`Node`].
fn node(value: &serde_json::Value) -> Node {
    match value {
        serde_json::Value::Null => Node::null(),
        serde_json::Value::Bool(b) => Node::from(*b),
        serde_json::Value::Number(n) => Node::from(n.as_i64().expect("integer values only")),
        serde_json::Value::String(s) => Node::from(s.as_str()),
        serde_json::Value::Array(elements) => {
            Node::from(elements.iter().map(node).collect::<Vec<_>>())
        }
        serde_json::Value::Object(entries) => Node::from(
            entries
                .iter()
                .map(|(k, v)| (k.as_str(), node(v)))
                .collect::<NodeObject>(),
        ),
    }
}

/// Adds a JSON AST or IDL file, by extension.
fn add_file(assembler: ModelAssembler, path: &Path) -> ModelAssembler {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => assembler.add_json_file(path),
        Some("smithy") => assembler.add_idl_file(path),
        _ => panic!("unexpected case file {}", path.display()),
    }
}

fn load(path: &Path, disable_prelude: bool) -> Model {
    let mut assembler = Model::assembler();
    if disable_prelude {
        assembler = assembler.disable_prelude();
    }
    add_file(assembler, path)
        .assemble()
        .into_result()
        .unwrap_or_else(|e| panic!("{}: {:?}", path.display(), e.diagnostics()))
}

/// Applies one `case.json` step, the way `Regenerate.java` does.
fn apply_step(assembler: ModelAssembler, dir: &Path, step: &serde_json::Value) -> ModelAssembler {
    let string = |key: &str| step.get(key).and_then(|v| v.as_str());
    if let Some(file) = string("addShapes") {
        // The shapes of a file, added one by one as if built in code. Like `Regenerate.java`,
        // shapes in the prelude namespace are skipped unless the file is loaded without the
        // prelude, including ones the file adds to the namespace.
        let disable_prelude = step
            .get("disablePrelude")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let source = load(&dir.join(file), disable_prelude);
        source
            .non_prelude_shapes()
            .filter(|shape| disable_prelude || !prelude::is_prelude_shape(shape.id()))
            .fold(assembler, |assembler, shape| {
                assembler.add_shape(shape.shape().clone())
            })
    } else if let Some(file) = string("addModel") {
        assembler.add_model(&load(&dir.join(file), false))
    } else if let Some(file) = string("addFile") {
        add_file(assembler, &dir.join(file))
    } else if let Some(key) = string("putMetadata") {
        assembler.put_metadata(key, node(&step["value"]))
    } else if let Some(target) = string("addTrait") {
        assembler.add_trait(target, string("trait").unwrap(), node(&step["value"]))
    } else {
        panic!("unknown step in {}: {step}", dir.display())
    }
}

fn assemble(name: &str) -> aws_smithy_lang::Validated<Model> {
    let dir = fixtures().join("assembly").join(name);
    let spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("case.json")).unwrap()).unwrap();
    spec["steps"]
        .as_array()
        .unwrap()
        .iter()
        .fold(Model::assembler(), |assembler, step| {
            apply_step(assembler, &dir, step)
        })
        .assemble()
}

/// Cases where only the order of concatenated values differs from the reference implementation.
///
/// When a JSON AST document redefines a shape built in code, the reference implementation
/// merges the document's first trait in document order before the shape's own traits, so the
/// order depends on whether `traits` or `members` comes first in the JSON object. This crate
/// merges the shape built in code's traits first; the specification leaves the order of
/// concatenated values unspecified. These cases still check which values are merged and which
/// definition provides the source locations.
const UNORDERED: &[&str] = &[
    "created_after_document",
    "created_json_map_value_first",
    "created_json_members_first",
    "created_json_traits_first",
];

/// A JSON AST document with every array sorted, to compare values regardless of their order.
fn without_array_order(json: &str) -> serde_json::Value {
    fn sort(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Array(elements) => {
                elements.iter_mut().for_each(sort);
                elements.sort_by_key(|element| element.to_string());
            }
            serde_json::Value::Object(entries) => entries.values_mut().for_each(sort),
            _ => {}
        }
    }
    let mut value = serde_json::from_str(json).unwrap();
    sort(&mut value);
    value
}

/// The Rust diagnostic code for a reference event ID on a rejected case.
fn rust_code(case: &str, event: &str) -> DiagnosticCode {
    match (case, event) {
        ("created_conflicting_document" | "document_prelude_conflict", "Model") => {
            DiagnosticCode::ShapeConflict
        }
        ("add_trait_prelude_shape", "Model") => DiagnosticCode::PreludeConflict,
        _ => panic!("{case}: map the reference event `{event}` to a Rust code"),
    }
}

/// The file name of a location, or an empty name for one created in code.
fn file_name(location: &SourceLocation) -> String {
    if location.is_none() {
        return String::new();
    }
    Path::new(location.source())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The location of a shape, member, or trait (`SHAPE_ID@TRAIT_ID`).
fn location_of(model: &Model, target: &str) -> SourceLocation {
    let (shape, trait_id) = match target.split_once('@') {
        Some((shape, trait_id)) => (shape, Some(trait_id)),
        None => (target, None),
    };
    let shape = model.expect_shape(&ShapeId::new(shape).unwrap()).unwrap();
    match trait_id {
        None => shape.source_location().clone(),
        Some(id) => shape
            .get_trait(&ShapeId::new(id).unwrap())
            .unwrap_or_else(|| panic!("`{target}` has no such trait"))
            .source_location()
            .clone(),
    }
}

#[test]
fn every_case_has_an_outcome() {
    let mut recorded: Vec<_> = outcomes().into_iter().map(|o| o.name).collect();
    recorded.sort();
    let mut on_disk: Vec<_> = std::fs::read_dir(fixtures().join("assembly"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    assert_eq!(recorded, on_disk, "regenerate expected/ (see README.md)");
}

#[test]
fn cases_match_the_reference_implementation() {
    let mut failures = Vec::new();
    for outcome in outcomes() {
        let name = &outcome.name;
        let validated = assemble(name);
        if !outcome.accepted {
            let errors: HashSet<_> = validated
                .diagnostics()
                .iter()
                .filter(|d| d.severity() == Severity::Error)
                .map(|d| d.code())
                .collect();
            let expected: HashSet<_> = outcome.errors.iter().map(|e| rust_code(name, e)).collect();
            if errors != expected {
                failures.push(format!(
                    "{name}: reference events {:?}, Rust diagnostics {:?}",
                    outcome.errors,
                    validated.diagnostics()
                ));
            }
            continue;
        }
        let model = match validated.into_result() {
            Ok(model) => model,
            Err(e) => {
                failures.push(format!("{name}: Rust rejects: {:?}", e.diagnostics()));
                continue;
            }
        };
        let expected =
            Model::from_json_file(fixtures().join(format!("expected/assembly/{name}.json")))
                .unwrap_or_else(|e| {
                    panic!(
                        "{name}: reference output does not load: {:?}",
                        e.diagnostics()
                    )
                });
        if !model.equivalent(&expected) {
            let writer = AstWriter::new().canonical(true).pretty(true);
            let unordered_match = UNORDERED.contains(&name.as_str())
                && without_array_order(&writer.to_string(&model).unwrap())
                    == without_array_order(&writer.to_string(&expected).unwrap());
            if !unordered_match {
                failures.push(format!(
                    "{name}: models differ\n--- rust\n{}\n--- reference\n{}",
                    writer.to_string(&model).unwrap(),
                    writer.to_string(&expected).unwrap()
                ));
                continue;
            }
        } else if UNORDERED.contains(&name.as_str()) {
            failures.push(format!(
                "{name}: matches the reference order; remove it from `UNORDERED`"
            ));
        }
        for (target, file) in &outcome.locations {
            let actual = file_name(&location_of(&model, target));
            if actual != *file {
                failures.push(format!(
                    "{name}: `{target}` comes from `{actual}`, the reference keeps `{file}`"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

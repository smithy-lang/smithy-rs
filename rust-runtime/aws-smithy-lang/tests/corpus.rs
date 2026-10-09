/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Loads, inspects, and round-trips every Smithy JSON AST model in `aws/sdk/aws-models`.

use aws_smithy_lang::traits::{Documentation, DOCUMENTATION};
use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{Model, ModelWriter, ShapeType};
use std::path::PathBuf;

/// Every JSON object in the corpus with top-level `smithy` and `shapes` properties.
fn corpus() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter(|path| {
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            value.get("smithy").is_some() && value.get("shapes").is_some()
        })
        .collect();
    paths.sort();
    paths
}

fn load(path: &PathBuf) -> Model {
    Model::from_json_file(path).unwrap_or_else(|err| {
        let diagnostics: Vec<_> = err.diagnostics().iter().map(|d| d.to_string()).collect();
        panic!("{}: {err}\n{}", path.display(), diagnostics.join("\n"))
    })
}

#[test]
fn corpus_loads_and_round_trips() {
    let paths = corpus();
    assert!(!paths.is_empty(), "no corpus models found");
    let writer = ModelWriter::new();
    let mut shapes = 0;
    for path in &paths {
        let model = load(path);
        shapes += model.non_prelude_shapes().len();

        let first = writer.to_string(&model).unwrap();
        assert_eq!(
            first,
            writer.to_string(&model).unwrap(),
            "{}",
            path.display()
        );
        let reloaded = Model::from_json_str("round-trip", &first)
            .unwrap_or_else(|e| panic!("{}: reload failed: {e}", path.display()));
        assert!(model.equivalent(&reloaded), "{}", path.display());
        assert_eq!(
            first,
            writer.to_string(&reloaded).unwrap(),
            "{}",
            path.display()
        );

        let pretty = ModelWriter::new().pretty(true).to_string(&model).unwrap();
        assert!(model.equivalent(&Model::from_json_str("pretty", &pretty).unwrap()));
    }
    // Logged as a baseline; the corpus changes over time, so this is not asserted.
    println!("corpus: {} models, {shapes} document shapes", paths.len());
}

#[test]
fn corpus_inspection() {
    let mut prelude_targets = 0;
    for path in corpus() {
        let model = load(&path);
        let name = path.display();
        let services: Vec<_> = model.services().collect();
        assert_eq!(services.len(), 1, "{name}: expected one service");
        let service = services[0];

        // Every operation in the service closure resolves with structure input/output.
        let closure: Vec<_> = Walker::new(&model).walk(*service).collect();
        assert!(closure.len() > service.operations().len(), "{name}");
        for shape in &closure {
            if let Some(operation) = shape.as_operation() {
                assert!(operation.input_shape().is_structure(), "{name}");
                assert!(operation.output_shape().is_structure(), "{name}");
                for error in operation.errors() {
                    assert!(model
                        .expect_shape(error)
                        .unwrap()
                        .has_trait("smithy.api#error"));
                }
            }
        }

        // Every member target resolves, including prelude targets.
        for shape in model.non_prelude_shapes() {
            for member in shape.members().iter() {
                let target = member.target_shape();
                assert_eq!(target.id(), member.target());
                let looked_up = model.expect_shape(member.id()).unwrap();
                assert_eq!(
                    looked_up.expect_member().unwrap().container().id(),
                    shape.id()
                );
            }
        }
        prelude_targets += model
            .non_prelude_shapes()
            .flat_map(|s| s.members().iter().collect::<Vec<_>>())
            .filter(|m| m.target_shape().is_prelude())
            .count();

        // Generic traits are preserved and typed decoding works where present.
        for shape in model.shapes_with_trait(DOCUMENTATION).take(20) {
            let documentation = shape.get_trait_as::<Documentation>().unwrap().unwrap();
            assert_eq!(
                Some(documentation.0.as_str()),
                shape.get_trait(DOCUMENTATION).unwrap().as_str()
            );
        }
        assert!(
            model
                .applied_trait_ids()
                .any(|id| id.namespace() != "smithy.api"),
            "{name}"
        );
        assert!(
            model.shapes_by_type(ShapeType::Structure).count() > 0,
            "{name}"
        );
    }
    assert!(
        prelude_targets > 0,
        "expected members targeting prelude shapes"
    );
}

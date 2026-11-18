/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Loads, inspects, and round-trips every Smithy JSON AST model in `aws/sdk/aws-models`.

use aws_smithy_lang::traits::{self, Documentation, DOCUMENTATION};
use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{AstWriter, Model, ShapeId, ShapeType};
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
    let writer = AstWriter::new();
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

        let pretty = AstWriter::new().pretty(true).to_string(&model).unwrap();
        assert!(model.equivalent(&Model::from_json_str("pretty", &pretty).unwrap()));
    }
    // Logged as a baseline; the corpus changes over time, so this is not asserted.
    println!("corpus: {} models, {shapes} document shapes", paths.len());
}

#[test]
fn corpus_assembles_into_one_model() {
    let paths = corpus();
    let mut assembler = Model::assembler();
    let mut expected = std::collections::HashMap::new();
    for path in &paths {
        let model = load(path);
        for shape in model.non_prelude_shapes() {
            expected.insert(shape.id().clone(), shape.shape().clone());
        }
        assembler = assembler.add_json_file(path);
    }
    let validated = assembler.assemble();
    let errors: Vec<_> = validated
        .diagnostics()
        .errors()
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    // Shapes shared by several models merge when equal; nothing else is reported.
    for diagnostic in validated.diagnostics() {
        assert_eq!(
            diagnostic.code(),
            aws_smithy_lang::diagnostic::DiagnosticCode::DuplicateDefinition,
            "{diagnostic}"
        );
    }
    let combined = validated.into_result().unwrap();
    assert_eq!(combined.services().count(), paths.len());
    assert_eq!(combined.non_prelude_shapes().len(), expected.len());
    for shape in combined.non_prelude_shapes() {
        assert_eq!(
            Some(shape.shape()),
            expected.get(shape.id()),
            "{}",
            shape.id()
        );
    }

    // The combined model also round-trips, and can be re-assembled from itself.
    let json = AstWriter::new().to_string(&combined).unwrap();
    assert!(combined.equivalent(&Model::from_json_str("combined", &json).unwrap()));
    let again = Model::assembler()
        .add_model(&combined)
        .assemble()
        .into_result()
        .unwrap();
    assert!(again.equivalent(&combined));
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
        let closure: Vec<_> = Walker::new().walk(service).collect();
        assert!(closure.len() > service.direct_operations().len(), "{name}");
        for shape in &closure {
            if let Some(operation) = shape.as_operation() {
                assert!(operation.input_shape().is_structure(), "{name}");
                assert!(operation.output_shape().is_structure(), "{name}");
                for error in operation.direct_errors() {
                    assert!(model.expect_shape(error).unwrap().has_trait(&traits::ERROR));
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
        for shape in model.shapes_with_trait(&DOCUMENTATION).take(20) {
            let documentation = shape.get_trait_as::<Documentation>().unwrap().unwrap();
            assert_eq!(
                Some(documentation.0.as_str()),
                shape.get_trait(&DOCUMENTATION).unwrap().as_str()
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

#[test]
fn corpus_knowledge_invariants() {
    use aws_smithy_lang::relationships::RelationshipType;
    use std::collections::{BTreeMap, BTreeSet};

    for path in corpus() {
        let model = load(&path);
        let name = path.display();
        let knowledge = model.knowledge();
        let service = model.services().next().unwrap();
        let operations = knowledge.operations();
        let bottom_up = knowledge.bottom_up();

        // Top-down matches an uncached walk restricted to containment edges.
        let walked: BTreeSet<String> = Walker::new()
            .filter(|r| r.kind().is_containment())
            .walk(service)
            .skip(1)
            .map(|s| s.id().to_string())
            .collect();
        let contained: Vec<_> = service.contained_operations().collect();
        let contained_ids: Vec<String> = contained.iter().map(|o| o.id().to_string()).collect();
        let resource_ids: Vec<String> = service
            .contained_resources()
            .map(|r| r.id().to_string())
            .collect();
        let mut sorted = contained_ids.clone();
        sorted.sort();
        assert_eq!(
            contained_ids, sorted,
            "{name}: contained operations are sorted"
        );
        let both: BTreeSet<String> = contained_ids.iter().chain(&resource_ids).cloned().collect();
        assert_eq!(both, walked, "{name}");
        assert_eq!(
            both.len(),
            contained_ids.len() + resource_ids.len(),
            "{name}"
        );
        for direct in service.direct_operations() {
            assert!(contained_ids.contains(&direct.to_string()), "{name}");
        }
        for direct in service.direct_resources() {
            assert!(resource_ids.contains(&direct.to_string()), "{name}");
        }
        assert!(!contained.is_empty(), "{name}");

        for operation in &contained {
            // Effective errors resolve to @error structures and include every direct error.
            let knowledge = operations.get(operation.id()).unwrap();
            let errors: Vec<_> = knowledge.errors_in(service.id()).unwrap().collect();
            assert!(errors.iter().all(|e| e.has_trait(&traits::ERROR)), "{name}");
            assert!(
                errors.len() >= operation.direct_errors().len(),
                "{name}: {}",
                operation.id()
            );
            // Reverse bindings point back to the operation.
            for binding in [
                operations.input_bindings(operation.input()).unwrap(),
                operations.output_bindings(operation.output()).unwrap(),
            ] {
                let ids: Vec<_> = binding.map(|o| o.id().clone()).collect();
                assert!(ids.contains(operation.id()), "{name}: {}", operation.id());
            }
            assert!(operations.is_input_structure(operation.input()), "{name}");
            assert!(operations.is_output_structure(operation.output()), "{name}");
        }

        // Every contained entity has a service-scoped parent that is itself in the service.
        for id in contained_ids.iter().chain(&resource_ids) {
            let shape_id: ShapeId = id.parse().unwrap();
            let bindings: Vec<_> = bottom_up
                .get_direct_bindings(service.id(), &shape_id)
                .unwrap()
                .collect();
            assert!(!bindings.is_empty(), "{name}: {id}");
            for binding in bindings {
                let parent = binding.parent().id().to_string();
                assert!(
                    parent == service.id().as_str() || resource_ids.contains(&parent),
                    "{name}: {id} -> {parent}"
                );
                assert!(binding.kind().is_containment(), "{name}");
            }
            if let Ok(Some(chain)) = bottom_up.parent_chain(service.id(), &shape_id) {
                assert_eq!(chain.service().id(), service.id(), "{name}");
            }
        }

        // Reverse relationship counts equal forward counts by kind.
        let relationships = knowledge.relationships();
        let mut forward: BTreeMap<String, usize> = BTreeMap::new();
        let mut reverse: BTreeMap<String, usize> = BTreeMap::new();
        for shape in model.shapes() {
            let refs = std::iter::once(aws_smithy_lang::ShapeRef::Shape(shape)).chain(
                shape
                    .members()
                    .iter()
                    .map(aws_smithy_lang::ShapeRef::Member),
            );
            for shape in refs {
                for r in relationships.get_outgoing(shape.id()).unwrap() {
                    *forward.entry(format!("{:?}", r.kind())).or_default() += 1;
                }
                for r in relationships.get_incoming(shape.id()).unwrap() {
                    *reverse.entry(format!("{:?}", r.kind())).or_default() += 1;
                }
            }
        }
        assert_eq!(forward, reverse, "{name}");
        assert!(
            forward[&format!("{:?}", RelationshipType::Input)] > 0,
            "{name}"
        );

        // Repeated queries are identical.
        let again: Vec<String> = service
            .contained_operations()
            .map(|o| o.id().to_string())
            .collect();
        assert_eq!(again, contained_ids, "{name}");
    }
}

/// Every corpus model builds, both in full and for its service, and the schemas agree with
/// the model.
#[cfg(feature = "runtime-schema")]
#[test]
fn corpus_runtime_schemas() {
    use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
    use aws_smithy_lang::shape::MemberView;
    use aws_smithy_schema::Schema;

    for path in corpus() {
        let model = load(&path);
        let name = path.display();
        let all = RuntimeSchemaSet::try_from(&model).unwrap_or_else(|e| panic!("{name}: {e}"));
        let service = model.services().next().unwrap();
        let scoped = RuntimeSchemaSet::builder(&model)
            .for_service(service.id())
            .build()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(scoped.len() <= all.len(), "{name}");
        assert_eq!(
            scoped.operations().len(),
            service.contained_operations().len(),
            "{name}"
        );

        let operations = model.knowledge().operations();
        for operation in scoped.operations() {
            let operation_id = ShapeId::new(operation.id().as_str()).unwrap();
            let knowledge = operations.get(&operation_id).unwrap();
            let view = knowledge.operation();
            let input = operation.input();
            assert_eq!(input.shape_id().as_str(), view.input().as_str(), "{name}");
            assert_eq!(
                input.members().len(),
                knowledge.input_members().len(),
                "{name}: {operation_id}"
            );
            assert_eq!(
                input.http().is_some(),
                view.has_trait(&ShapeId::from_static("smithy.api#http")),
                "{name}: {operation_id}"
            );
            assert_eq!(
                operation.output().shape_id().as_str(),
                view.output().as_str()
            );
            let errors: Vec<_> = operation
                .errors()
                .iter()
                .map(|error| error.shape_id().as_str())
                .collect();
            let expected: Vec<_> = knowledge
                .errors_in(service.id())
                .unwrap()
                .map(|error| error.id().as_str())
                .collect();
            assert_eq!(errors, expected, "{name}: {operation_id}");
        }

        // The service scope's own containment walk selects exactly `TopDownIndex`'s
        // operations, in the same order.
        let contained: Vec<&str> = service
            .contained_operations()
            .map(|operation| operation.id().as_str())
            .collect();
        let selected: Vec<&str> = scoped.operations().map(|o| o.id().as_str()).collect();
        assert_eq!(selected, contained, "{name}");

        // Filtered scopes select only the requested operations, which agree with the full
        // service scope. First, middle, and last, alone and together (given in reverse).
        let ids: Vec<ShapeId> = service
            .contained_operations()
            .map(|operation| operation.id().clone())
            .collect();
        let picks = [0, ids.len() / 2, ids.len() - 1].map(|i| ids[i].clone());
        let mut selections: Vec<Vec<ShapeId>> = picks.iter().map(|id| vec![id.clone()]).collect();
        selections.push(picks.iter().rev().cloned().collect());
        for selection in selections {
            let filtered = RuntimeSchemaSet::builder(&model)
                .for_service_operations(service.id(), &selection)
                .build()
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(filtered.len() <= scoped.len(), "{name}");
            let mut expected: Vec<&str> = selection.iter().map(|id| id.as_str()).collect();
            expected.sort_unstable();
            expected.dedup();
            let actual: Vec<&str> = filtered.operations().map(|o| o.id().as_str()).collect();
            assert_eq!(actual, expected, "{name}");
            for operation in filtered.operations() {
                let id = ShapeId::new(operation.id().as_str()).unwrap();
                let full = scoped.operation(&id).unwrap();
                let error_ids = |o: &aws_smithy_lang::runtime_schema::OperationSchema<'_>| {
                    o.errors()
                        .iter()
                        .map(|error| error.shape_id().as_str().to_owned())
                        .collect::<Vec<_>>()
                };
                assert_eq!(error_ids(&operation), error_ids(&full), "{name}: {id}");
                assert_eq!(
                    operation.input().members().len(),
                    full.input().members().len(),
                    "{name}: {id}"
                );
                assert_eq!(
                    operation.input().http().map(|http| http.uri()),
                    full.input().http().map(|http| http.uri()),
                    "{name}: {id}"
                );
            }
        }

        // Every member schema mirrors its model member, and is the one schema for that
        // member: structure and union member slices, list members, and map keys and values
        // all point at the schema registered under the member's ID.
        let same_member = |schema: &Schema<'_>, view: MemberView<'_>| {
            assert_eq!(schema.member_name(), Some(view.name()), "{name}");
            assert_eq!(schema.shape_id().as_str(), view.id().as_str(), "{name}");
            assert!(
                std::ptr::eq(schema, all.schema(view.id()).unwrap()),
                "{name}: {}",
                view.id()
            );
        };
        for shape in model.non_prelude_shapes() {
            let Some(schema) = all.schema(shape.id()) else {
                continue;
            };
            match shape.shape_type() {
                ShapeType::Structure | ShapeType::Union => {
                    assert_eq!(schema.members().len(), shape.members().len(), "{name}");
                    for (member, view) in schema.members().iter().zip(shape.members().iter()) {
                        same_member(member, view);
                    }
                }
                ShapeType::List => {
                    let list = shape.as_list().unwrap();
                    same_member(schema.member().unwrap(), list.member());
                }
                ShapeType::Map => {
                    let map = shape.as_map().unwrap();
                    same_member(schema.key().unwrap(), map.key());
                    // `Schema::member` is a map's value.
                    same_member(schema.member().unwrap(), map.value());
                }
                _ => {}
            }
        }
    }
}

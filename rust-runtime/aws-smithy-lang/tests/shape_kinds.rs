/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Positive and malformed fixtures for every Smithy 2.0 shape kind.

use aws_smithy_lang::diagnostic::DiagnosticCode;
use aws_smithy_lang::{Model, ModelWriter, ShapeType};

fn document(shapes: &str) -> String {
    format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#)
}

fn load(shapes: &str) -> Model {
    Model::from_json_str("t.json", &document(shapes))
        .unwrap_or_else(|e| panic!("{shapes}: {:?}", e.diagnostics()))
}

/// Asserts that `shapes` fails with `code` at `pointer`.
fn rejects(shapes: &str, code: DiagnosticCode, pointer: &str) {
    let err = Model::from_json_str("t.json", &document(shapes))
        .err()
        .unwrap_or_else(|| panic!("expected failure: {shapes}"));
    let found = err.diagnostics().iter().any(|d| {
        d.code() == code && d.source_location().and_then(|l| l.pointer()) == Some(pointer)
    });
    assert!(
        found,
        "{shapes}: expected {code:?} at {pointer}, got {:?}",
        err.diagnostics()
    );
}

/// Loads the shape, checks its type, and checks it round-trips.
fn accepts(shapes: &str, id: &str, shape_type: ShapeType) -> Model {
    let model = load(shapes);
    assert_eq!(
        model.expect_shape(id).unwrap().shape_type(),
        shape_type,
        "{shapes}"
    );
    let out = ModelWriter::new().to_string(&model).unwrap();
    assert!(
        model.equivalent(&Model::from_json_str("rt", &out).unwrap()),
        "{shapes}"
    );
    model
}

#[test]
fn simple_shapes() {
    let kinds = [
        ("blob", ShapeType::Blob),
        ("boolean", ShapeType::Boolean),
        ("document", ShapeType::Document),
        ("string", ShapeType::String),
        ("byte", ShapeType::Byte),
        ("short", ShapeType::Short),
        ("integer", ShapeType::Integer),
        ("long", ShapeType::Long),
        ("float", ShapeType::Float),
        ("double", ShapeType::Double),
        ("bigInteger", ShapeType::BigInteger),
        ("bigDecimal", ShapeType::BigDecimal),
        ("timestamp", ShapeType::Timestamp),
    ];
    for (name, shape_type) in kinds {
        assert_eq!(shape_type.as_str(), name);
        assert!(shape_type.is_simple());
        let model = accepts(
            &format!(r#""a#S":{{"type":"{name}","traits":{{"smithy.api#documentation":"d"}}}}"#),
            "a#S",
            shape_type,
        );
        assert!(model.expect_shape("a#S").unwrap().members().is_empty());
        rejects(
            &format!(r#""a#S":{{"type":"{name}","member":{{"target":"smithy.api#String"}}}}"#),
            DiagnosticCode::UnknownProperty,
            "/shapes/a#S/member",
        );
    }
}

#[test]
fn list() {
    let model = accepts(
        r#""a#L":{"type":"list","member":{"target":"smithy.api#String","traits":{"smithy.api#length":{"min":1}}}}"#,
        "a#L",
        ShapeType::List,
    );
    let list = model.expect_shape("a#L").unwrap().expect_list().unwrap();
    assert!(list.member().has_trait("smithy.api#length"));
    rejects(
        r#""a#L":{"type":"list"}"#,
        DiagnosticCode::MissingProperty,
        "/shapes/a#L",
    );
    rejects(
        r#""a#L":{"type":"list","member":{"target":"a#Missing"}}"#,
        DiagnosticCode::UnresolvedReference,
        "/shapes/a#L/member/target",
    );
    rejects(
        r#""a#L":{"type":"list","member":"smithy.api#String"}"#,
        DiagnosticCode::InvalidProperty,
        "/shapes/a#L/member",
    );
}

#[test]
fn map() {
    let model = accepts(
        r#""a#M":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"smithy.api#Integer"}}"#,
        "a#M",
        ShapeType::Map,
    );
    let map = model.expect_shape("a#M").unwrap().expect_map().unwrap();
    assert_eq!(map.value().target(), "smithy.api#Integer");
    rejects(
        r#""a#M":{"type":"map","key":{"target":"smithy.api#String"}}"#,
        DiagnosticCode::MissingProperty,
        "/shapes/a#M",
    );
    rejects(
        r#""a#M":{"type":"map","key":{"target":"smithy.api#Blob"},"value":{"target":"smithy.api#Blob"}}"#,
        DiagnosticCode::InvalidTarget,
        "/shapes/a#M/key/target",
    );
}

#[test]
fn structure() {
    accepts(r#""a#S":{"type":"structure"}"#, "a#S", ShapeType::Structure);
    let model = accepts(
        r#""a#S":{"type":"structure","members":{"b":{"target":"smithy.api#String"},"a":{"target":"a#S"}}}"#,
        "a#S",
        ShapeType::Structure,
    );
    let names: Vec<_> = model
        .expect_shape("a#S")
        .unwrap()
        .members()
        .names()
        .collect();
    assert_eq!(names, ["b", "a"]);
    rejects(
        r#""a#S":{"type":"structure","members":{"a":{"target":"smithy.api#String"},"A":{"target":"smithy.api#String"}}}"#,
        DiagnosticCode::CaseConflict,
        "/shapes/a#S/members/A",
    );
    rejects(
        r#""a#S":{"type":"structure","members":{"a":{"target":"smithy.api#Unit"}}}"#,
        DiagnosticCode::InvalidTarget,
        "/shapes/a#S/members/a/target",
    );
    rejects(
        r#""a#S":{"type":"structure","members":{"a-b":{"target":"smithy.api#String"}}}"#,
        DiagnosticCode::InvalidShapeId,
        "/shapes/a#S/members/a-b",
    );
}

#[test]
fn union() {
    accepts(
        r#""a#U":{"type":"union","members":{"a":{"target":"smithy.api#Unit"},"b":{"target":"smithy.api#String"}}}"#,
        "a#U",
        ShapeType::Union,
    );
    rejects(
        r#""a#U":{"type":"union"}"#,
        DiagnosticCode::InvalidShape,
        "/shapes/a#U",
    );
}

#[test]
fn enum_shapes() {
    let model = accepts(
        r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"}}}"#,
        "a#E",
        ShapeType::Enum,
    );
    let member = model
        .expect_shape("a#E$A")
        .unwrap()
        .expect_member()
        .unwrap();
    assert_eq!(
        member.enum_value(),
        Some(aws_smithy_lang::shape::EnumValue::String("A"))
    );
    rejects(
        r#""a#E":{"type":"enum"}"#,
        DiagnosticCode::InvalidShape,
        "/shapes/a#E",
    );
    rejects(
        r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#String"}}}"#,
        DiagnosticCode::InvalidTarget,
        "/shapes/a#E/members/A/target",
    );

    accepts(
        r#""a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":7}}}}"#,
        "a#I",
        ShapeType::IntEnum,
    );
    rejects(
        r#""a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit"}}}"#,
        DiagnosticCode::InvalidShape,
        "/shapes/a#I/members/A",
    );
}

#[test]
fn service() {
    let model = accepts(
        r#""a#Svc":{"type":"service","version":"2020","operations":[{"target":"a#Op"}],
            "resources":[{"target":"a#R"}],"errors":[{"target":"a#E"}],"rename":{"a#E":"Renamed"}},
           "a#Op":{"type":"operation"},"a#R":{"type":"resource"},
           "a#E":{"type":"structure","traits":{"smithy.api#error":"client"}}"#,
        "a#Svc",
        ShapeType::Service,
    );
    let service = model
        .expect_shape("a#Svc")
        .unwrap()
        .expect_service()
        .unwrap();
    assert_eq!(service.version(), Some("2020"));
    accepts(r#""a#Svc":{"type":"service"}"#, "a#Svc", ShapeType::Service);
    rejects(
        r#""a#Svc":{"type":"service","version":1}"#,
        DiagnosticCode::InvalidProperty,
        "/shapes/a#Svc/version",
    );
    rejects(
        r#""a#Svc":{"type":"service","operations":{"target":"a#Op"}}"#,
        DiagnosticCode::InvalidProperty,
        "/shapes/a#Svc/operations",
    );
}

#[test]
fn operation() {
    let model = accepts(
        r#""a#Op":{"type":"operation","input":{"target":"a#In"},"output":{"target":"a#In"},
           "errors":[{"target":"a#E"}]},
           "a#In":{"type":"structure"},
           "a#E":{"type":"structure","traits":{"smithy.api#error":"server"}}"#,
        "a#Op",
        ShapeType::Operation,
    );
    let operation = model
        .expect_shape("a#Op")
        .unwrap()
        .expect_operation()
        .unwrap();
    assert_eq!(operation.output(), "a#In");
    rejects(
        r#""a#Op":{"type":"operation","input":{"target":"smithy.api#String"}}"#,
        DiagnosticCode::InvalidTarget,
        "/shapes/a#Op/input/target",
    );
    rejects(
        r#""a#Op":{"type":"operation","input":{}}"#,
        DiagnosticCode::MissingProperty,
        "/shapes/a#Op/input",
    );
}

#[test]
fn resource() {
    let model = accepts(
        r#""a#R":{"type":"resource","identifiers":{"id":{"target":"smithy.api#String"}},
           "properties":{"p":{"target":"smithy.api#Integer"}},"create":{"target":"a#Op"},
           "put":{"target":"a#Op"},"read":{"target":"a#Op"},"update":{"target":"a#Op"},
           "delete":{"target":"a#Op"},"list":{"target":"a#Op"},"operations":[{"target":"a#Op"}],
           "collectionOperations":[{"target":"a#Op"}],"resources":[{"target":"a#Child"}]},
           "a#Op":{"type":"operation"},"a#Child":{"type":"resource"}"#,
        "a#R",
        ShapeType::Resource,
    );
    let resource = model
        .expect_shape("a#R")
        .unwrap()
        .expect_resource()
        .unwrap();
    assert_eq!(resource.list().unwrap(), "a#Op");
    assert_eq!(resource.properties().count(), 1);
    rejects(
        r#""a#R":{"type":"resource","identifiers":{"bad-id":{"target":"smithy.api#String"}}}"#,
        DiagnosticCode::InvalidProperty,
        "/shapes/a#R/identifiers/bad-id",
    );
    rejects(
        r#""a#R":{"type":"resource","resources":[{"target":"a#R"}]}"#,
        DiagnosticCode::InvalidShape,
        "/shapes/a#R/resources/0/target",
    );
}

#[test]
fn unknown_and_unsupported_kinds() {
    rejects(
        r#""a#S":{"type":"widget"}"#,
        DiagnosticCode::UnknownShapeType,
        "/shapes/a#S/type",
    );
    rejects(
        r#""a#S":{"type":"member"}"#,
        DiagnosticCode::UnknownShapeType,
        "/shapes/a#S/type",
    );
    rejects(
        r#""a#S":{"type":"set","member":{"target":"smithy.api#String"}}"#,
        DiagnosticCode::UnsupportedFeature,
        "/shapes/a#S/type",
    );
    rejects(
        r#""a#S":{"type":"apply"}"#,
        DiagnosticCode::UnsupportedFeature,
        "/shapes/a#S/type",
    );
}

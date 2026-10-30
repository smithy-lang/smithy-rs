/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Unit tests, and the Miri suite for the ownership component:
//!
//! ```text
//! cargo +nightly miri nextest run -p aws-smithy-lang --features runtime-schema --lib \
//!     -E 'test(/^runtime_schema::/)'
//! ```
//!
//! CI runs this, with `-Zmiri-strict-provenance`, from the crate's `additional-ci` script.
//!
//! Models used under Miri disable the prelude, because parsing it is slow there; tests that
//! need it, or load an AWS model, are ignored under Miri. `additional-ci` runs two of them
//! under Miri separately, with `--run-ignored only`.
//!
//! Sets are eager unless a test builds them with `Materialization::Lazy`; `LAZY` and
//! `service_set` are the fixtures for lazy materialization.

use super::owner::tests::{count_allocations, with_panic_after, Sentinel};
use super::plan::Slot;
use super::{Materialization, RuntimeSchemaSet, SchemaBuildError};
use crate::diagnostic::DiagnosticCode;
use crate::{Model, ShapeId};
use aws_smithy_schema::extension::SchemaExtensionKey;
use aws_smithy_schema::traits::TimestampFormat;
use aws_smithy_schema::{PayloadHint, Schema, ShapeType};
use std::sync::atomic::{AtomicUsize, Ordering};

fn id(value: &str) -> ShapeId {
    ShapeId::new(value).unwrap()
}

/// A model without the prelude, with `shapes` (a JSON object body) plus `ex#String` and the
/// `smithy.api#Unit` that omitted operation inputs and outputs refer to.
fn model(shapes: &str) -> Model {
    let json = format!(
        r#"{{"smithy": "2.0", "shapes": {{ "ex#String": {{"type": "string"}}, "smithy.api#Unit": {{"type": "structure", "members": {{}}}}{}{shapes} }} }}"#,
        if shapes.trim().is_empty() { "" } else { "," }
    );
    Model::assembler()
        .disable_prelude()
        .add_json_str("m.json", json)
        .assemble()
        .into_result()
        .unwrap_or_else(|err| panic!("{err}: {:?}", err.diagnostics()))
}

fn build(model: &Model) -> RuntimeSchemaSet {
    RuntimeSchemaSet::try_from(model).unwrap_or_else(|err| panic!("{err}"))
}

fn build_err(model: &Model) -> SchemaBuildError {
    RuntimeSchemaSet::try_from(model).expect_err("build should fail")
}

fn schema<'a>(set: &'a RuntimeSchemaSet, value: &str) -> &'a Schema<'a> {
    set.schema(&id(value))
        .unwrap_or_else(|| panic!("no schema for {value}"))
}

const EVERY_KIND: &str = r#"
    "ex#Blob": {"type": "blob"},
    "ex#Boolean": {"type": "boolean"},
    "ex#Document": {"type": "document"},
    "ex#Byte": {"type": "byte"},
    "ex#Short": {"type": "short"},
    "ex#Integer": {"type": "integer"},
    "ex#Long": {"type": "long"},
    "ex#Float": {"type": "float"},
    "ex#Double": {"type": "double"},
    "ex#BigInteger": {"type": "bigInteger"},
    "ex#BigDecimal": {"type": "bigDecimal"},
    "ex#Timestamp": {"type": "timestamp"},
    "ex#Enum": {"type": "enum", "members": {"A": {"target": "smithy.api#Unit"}}},
    "ex#IntEnum": {"type": "intEnum", "members": {"A": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": 1}}}},
    "ex#List": {"type": "list", "member": {"target": "ex#String"}},
    "ex#Map": {"type": "map", "key": {"target": "ex#String"}, "value": {"target": "ex#Integer"}},
    "ex#Structure": {"type": "structure", "members": {"a": {"target": "ex#String"}, "b": {"target": "ex#List"}}},
    "ex#Union": {"type": "union", "members": {"x": {"target": "ex#Integer"}}},
    "ex#Service": {"type": "service"}
"#;

#[test]
fn every_shape_kind_has_the_schema_type_generated_code_uses() {
    let set = build(&model(EVERY_KIND));
    let expected = [
        ("ex#String", ShapeType::String),
        ("ex#Blob", ShapeType::Blob),
        ("ex#Boolean", ShapeType::Boolean),
        ("ex#Document", ShapeType::Document),
        ("ex#Byte", ShapeType::Byte),
        ("ex#Short", ShapeType::Short),
        ("ex#Integer", ShapeType::Integer),
        ("ex#Long", ShapeType::Long),
        ("ex#Float", ShapeType::Float),
        ("ex#Double", ShapeType::Double),
        ("ex#BigInteger", ShapeType::BigInteger),
        ("ex#BigDecimal", ShapeType::BigDecimal),
        ("ex#Timestamp", ShapeType::Timestamp),
        ("ex#Enum", ShapeType::String),
        ("ex#IntEnum", ShapeType::Integer),
        ("ex#List", ShapeType::List),
        ("ex#Map", ShapeType::Map),
        ("ex#Structure", ShapeType::Structure),
        ("ex#Union", ShapeType::Union),
        // Defined by `model`, because the prelude is disabled.
        ("smithy.api#Unit", ShapeType::Structure),
    ];
    for (shape, shape_type) in expected {
        assert_eq!(schema(&set, shape).shape_type(), shape_type, "{shape}");
    }
    assert_eq!(set.len(), expected.len());
    assert!(set.schema(&id("ex#Service")).is_none());
    // Enum members are values, not member schemas.
    assert!(set.schema(&id("ex#Enum$A")).is_none());
    assert!(set.operations().next().is_none());
}

#[test]
fn ids_names_and_indexes_follow_the_model() {
    let set = build(&model(EVERY_KIND));
    let root = schema(&set, "ex#Structure");
    let id = root.shape_id();
    assert_eq!(
        (
            id.as_str(),
            id.namespace(),
            id.shape_name(),
            id.member_name()
        ),
        ("ex#Structure", "ex", "Structure", None)
    );
    let names: Vec<_> = root.members().iter().map(|m| m.member_name()).collect();
    assert_eq!(names, [Some("a"), Some("b")]);

    let b = root.member_schema("b").unwrap();
    let id = b.shape_id();
    assert_eq!(
        (
            id.as_str(),
            id.namespace(),
            id.shape_name(),
            id.member_name()
        ),
        ("ex#Structure$b", "ex", "Structure", Some("b"))
    );
    assert_eq!(b.member_index(), Some(1));
    assert_eq!(b.shape_type(), ShapeType::List);
    assert!(std::ptr::eq(b, schema(&set, "ex#Structure$b")));
    assert!(std::ptr::eq(root.member_schema_by_index(1).unwrap(), b));

    let map = schema(&set, "ex#Map");
    let (key, value) = (map.key().unwrap(), map.member().unwrap());
    assert_eq!(
        (key.member_name(), key.member_index()),
        (Some("key"), Some(0))
    );
    assert_eq!(
        (value.member_name(), value.member_index()),
        (Some("value"), Some(1))
    );
    assert_eq!(value.shape_type(), ShapeType::Integer);
    let list = schema(&set, "ex#List").member().unwrap();
    assert_eq!(
        (list.member_name(), list.member_index()),
        (Some("member"), Some(0))
    );
}

#[test]
fn nested_collection_members_are_shared() {
    let set = build(&model(
        r#"
        "ex#Inner": {"type": "list", "member": {"target": "ex#String"}},
        "ex#Outer": {"type": "map", "key": {"target": "ex#String"}, "value": {"target": "ex#Inner"}},
        "ex#S": {"type": "structure", "members": {
            "a": {"target": "ex#Outer"},
            "b": {"target": "ex#Outer"}
        }}"#,
    ));
    let inner_member = schema(&set, "ex#Inner$member");
    let outer = schema(&set, "ex#Outer");
    let outer_value = outer.member().unwrap();
    assert!(std::ptr::eq(outer_value.member().unwrap(), inner_member));
    for name in ["a", "b"] {
        let member = schema(&set, "ex#S").member_schema(name).unwrap();
        assert!(std::ptr::eq(member.key().unwrap(), outer.key().unwrap()));
        assert!(std::ptr::eq(member.member().unwrap(), outer_value));
    }
}

#[test]
fn recursion_through_structures_is_allowed() {
    let set = build(&model(
        r#"
        "ex#Node": {"type": "structure", "members": {"children": {"target": "ex#Nodes"}}},
        "ex#Nodes": {"type": "list", "member": {"target": "ex#Node"}}"#,
    ));
    let children = schema(&set, "ex#Node").member_schema("children").unwrap();
    assert_eq!(
        children.member().unwrap().shape_type(),
        ShapeType::Structure
    );
}

#[test]
fn collection_only_cycles_are_errors() {
    let err = build_err(&model(
        r#"
        "ex#Self": {"type": "list", "member": {"target": "ex#Self"}},
        "ex#A": {"type": "map", "key": {"target": "ex#String"}, "value": {"target": "ex#B"}},
        "ex#B": {"type": "list", "member": {"target": "ex#A"}}"#,
    ));
    let messages: Vec<_> = err.diagnostics().iter().map(|d| d.to_string()).collect();
    assert_eq!(messages.len(), 2, "{messages:#?}");
    assert!(err
        .diagnostics()
        .iter()
        .all(|d| d.code() == DiagnosticCode::InvalidShape));
    assert!(
        messages[0].contains("`ex#Self$member` -> `ex#Self$member`"),
        "{}",
        messages[0]
    );
    assert!(
        messages[1].contains("`ex#A$value` -> `ex#B$member` -> `ex#A$value`"),
        "{}",
        messages[1]
    );
}

#[test]
fn member_traits_and_target_propagation() {
    let set = build(&model(
        r#"
        "ex#Time": {"type": "timestamp", "traits": {"smithy.api#timestampFormat": "epoch-seconds"}},
        "ex#Json": {"type": "string", "traits": {"smithy.api#mediaType": "application/json"}},
        "ex#Stream": {"type": "blob", "traits": {"smithy.api#streaming": {}}},
        "ex#S": {"type": "structure", "members": {
            "inherited": {"target": "ex#Time"},
            "own": {"target": "ex#Time", "traits": {"smithy.api#timestampFormat": "http-date"}},
            "json": {"target": "ex#Json"},
            "body": {"target": "ex#Stream", "traits": {"smithy.api#httpPayload": {}}},
            "all": {"target": "ex#String", "traits": {
                "smithy.api#sensitive": {},
                "smithy.api#jsonName": "ALL",
                "smithy.api#xmlName": "All",
                "smithy.api#xmlAttribute": {},
                "smithy.api#xmlFlattened": {},
                "smithy.api#xmlNamespace": {"uri": "https://example.com", "prefix": "e"},
                "smithy.api#httpHeader": "x-all",
                "smithy.api#httpLabel": {},
                "smithy.api#httpPrefixHeaders": "x-p-",
                "smithy.api#httpQuery": "q",
                "smithy.api#httpQueryParams": {},
                "smithy.api#httpResponseCode": {},
                "smithy.api#eventHeader": {},
                "smithy.api#eventPayload": {},
                "smithy.api#hostLabel": {}
            }}
        }}"#,
    ));
    let s = schema(&set, "ex#S");
    let member = |name| s.member_schema(name).unwrap();
    let format = |name| member(name).timestamp_format().map(|f| f.format());
    assert_eq!(format("inherited"), Some(TimestampFormat::EpochSeconds));
    assert_eq!(format("own"), Some(TimestampFormat::HttpDate));
    assert_eq!(
        member("json").media_type().unwrap().value(),
        "application/json"
    );
    assert!(member("body").streaming());
    assert!(member("body").http_payload().is_some());

    let all = member("all");
    assert!(all.sensitive().is_some());
    assert_eq!(all.json_name().unwrap().value(), "ALL");
    assert_eq!(all.xml_name().unwrap().value(), "All");
    assert!(all.xml_attribute() && all.xml_flattened());
    let namespace = all.xml_namespace().unwrap();
    assert_eq!(
        (namespace.uri(), namespace.prefix()),
        ("https://example.com", Some("e"))
    );
    assert_eq!(all.http_header().unwrap().value(), "x-all");
    assert!(all.http_label().is_some());
    assert_eq!(all.http_prefix_headers().unwrap().value(), "x-p-");
    assert_eq!(all.http_query().unwrap().value(), "q");
    assert!(all.http_query_params().is_some() && all.http_response_code().is_some());
    // `Schema` has no getters for these yet; its `Debug` output shows them.
    let debug = format!("{all:?}");
    for field in [
        "event_header: Some(EventHeaderTrait)",
        "event_payload: Some(EventPayloadTrait)",
        "host_label: Some(HostLabelTrait)",
    ] {
        assert!(debug.contains(field), "{field} in {debug}");
    }

    // Like generated code, a scalar root carries no traits.
    assert!(schema(&set, "ex#Time").timestamp_format().is_none());
}

#[test]
fn structure_roots_carry_traits() {
    let set = build(&model(
        r#"
        "ex#S": {"type": "structure", "members": {}, "traits": {
            "smithy.api#xmlName": "Root",
            "smithy.api#xmlNamespace": {"uri": "https://example.com"},
            "smithy.api#sensitive": true
        }}"#,
    ));
    let s = schema(&set, "ex#S");
    assert_eq!(s.xml_name().unwrap().value(), "Root");
    assert_eq!(s.xml_namespace().unwrap().prefix(), None);
    assert!(
        s.sensitive().is_some(),
        "the Smithy 1.0 `true` form is accepted"
    );
}

#[test]
fn malformed_traits_are_located_errors() {
    let model = model(
        r#"
        "ex#S": {"type": "structure", "members": {
            "a": {"target": "ex#String", "traits": {"smithy.api#jsonName": 1}},
            "b": {"target": "ex#String", "traits": {"smithy.api#timestampFormat": "iso"}},
            "c": {"target": "ex#String", "traits": {"smithy.api#xmlNamespace": {"prefix": "p"}}},
            "d": {"target": "ex#String", "traits": {"smithy.api#httpLabel": false}}
        }},
        "ex#Op": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/", "code": 42}}}"#,
    );
    let err = build_err(&model);
    let diagnostics: Vec<_> = err.diagnostics().iter().collect();
    let found: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            let location = d.source_location().unwrap();
            (
                d.code(),
                d.shape_id().unwrap().to_string(),
                location.line().unwrap(),
                location.pointer().unwrap().to_owned(),
            )
        })
        .collect();
    let invalid = DiagnosticCode::InvalidTrait;
    assert_eq!(
        found,
        [
            (
                invalid,
                "ex#S$a".into(),
                3,
                "/shapes/ex#S/members/a/traits/smithy.api#jsonName".into()
            ),
            (
                invalid,
                "ex#S$b".into(),
                4,
                "/shapes/ex#S/members/b/traits/smithy.api#timestampFormat".into()
            ),
            (
                invalid,
                "ex#S$c".into(),
                5,
                "/shapes/ex#S/members/c/traits/smithy.api#xmlNamespace".into()
            ),
            (
                invalid,
                "ex#S$d".into(),
                6,
                "/shapes/ex#S/members/d/traits/smithy.api#httpLabel".into()
            ),
            (
                invalid,
                "ex#Op".into(),
                8,
                "/shapes/ex#Op/traits/smithy.api#http/code".into()
            ),
        ]
    );
    assert!(diagnostics[1].message().contains("found `iso`"));
    assert!(diagnostics[2]
        .message()
        .contains("missing required property `uri`"));
    assert_eq!(
        err.to_string(),
        "failed to build runtime schemas (5 errors); first at m.json:3:53: trait \
         `smithy.api#jsonName` on `ex#S$a` must be a string, found number"
    );

    let limited = RuntimeSchemaSet::builder(&model)
        .max_diagnostics(2)
        .build()
        .unwrap_err();
    assert_eq!(limited.diagnostics().len(), 3);
    assert_eq!(limited.diagnostics().omitted(), 3);
}

const OPERATIONS: &str = r#"
    "ex#Shared": {"type": "structure", "members": {
        "id": {"target": "ex#String", "traits": {"smithy.api#httpLabel": {}}},
        "body": {"target": "ex#String"}
    }, "traits": {"smithy.api#xmlName": "SharedRoot"}},
    "ex#Read": {"type": "operation", "input": {"target": "ex#Shared"},
        "traits": {"smithy.api#http": {"method": "GET", "uri": "/read/{id}"}}},
    "ex#Write": {"type": "operation", "input": {"target": "ex#Shared"},
        "traits": {"smithy.api#http": {"method": "PUT", "uri": "/write/{id}", "code": 201}}},
    "ex#Ping": {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/ping"}}},
    "ex#Pong": {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/pong"}}}
"#;

#[test]
fn operations_sharing_an_input_have_separate_inputs() {
    let set = build(&model(OPERATIONS));
    let base = schema(&set, "ex#Shared");
    assert!(base.http().is_none());
    let read = set.operation(&id("ex#Read")).unwrap().input();
    let write = set.operation(&id("ex#Write")).unwrap().input();
    for (input, method, uri, code) in [
        (read, "GET", "/read/{id}", 200),
        (write, "PUT", "/write/{id}", 201),
    ] {
        assert!(!std::ptr::eq(input, base));
        assert_eq!(input.shape_id().as_str(), "ex#Shared");
        assert_eq!(input.xml_name().unwrap().value(), "SharedRoot");
        let http = input.http().unwrap();
        assert_eq!(
            (http.method(), http.uri(), http.code()),
            (method, uri, code)
        );
        assert_eq!(input.payload_hint(), PayloadHint::NoStructPayload);
        assert!(input.has_body_members());
        // Members are shared with the base schema.
        assert!(std::ptr::eq(input.members()[0], base.members()[0]));
    }
}

#[test]
fn omitted_inputs_are_separate_unit_inputs() {
    let set = build(&model(OPERATIONS));
    let ping = set.operation(&id("ex#Ping")).unwrap();
    let pong = set.operation(&id("ex#Pong")).unwrap();
    assert!(!std::ptr::eq(ping.input(), pong.input()));
    for (operation, uri) in [(&ping, "/ping"), (&pong, "/pong")] {
        assert_eq!(operation.input().shape_id().as_str(), "smithy.api#Unit");
        assert_eq!(operation.input().http().unwrap().uri(), uri);
        assert!(operation.input().members().is_empty());
        assert!(!operation.input().has_body_members());
        assert_eq!(operation.output().shape_id().as_str(), "smithy.api#Unit");
    }
    // Outputs without an operation-specific behavior are shared.
    assert!(std::ptr::eq(ping.output(), pong.output()));
    assert_eq!(
        set.operations()
            .map(|o| o.id().to_string())
            .collect::<Vec<_>>(),
        ["ex#Read", "ex#Write", "ex#Ping", "ex#Pong"]
    );
}

#[test]
fn input_body_hints_follow_http_bindings() {
    let set = build(&model(
        r#"
        "ex#Payload": {"type": "structure", "members": {}},
        "ex#Bytes": {"type": "blob"},
        "ex#HeadersOnly": {"type": "structure", "members": {
            "h": {"target": "ex#String", "traits": {"smithy.api#httpHeader": "x-h"}},
            "raw": {"target": "ex#Bytes", "traits": {"smithy.api#httpPayload": {}}}
        }},
        "ex#Framed": {"type": "structure", "members": {
            "p": {"target": "ex#Payload", "traits": {"smithy.api#httpPayload": {}}}
        }},
        "ex#A": {"type": "operation", "input": {"target": "ex#HeadersOnly"}},
        "ex#B": {"type": "operation", "input": {"target": "ex#Framed"}}"#,
    ));
    let a = set.operation(&id("ex#A")).unwrap().input();
    assert!(!a.has_body_members());
    assert_eq!(a.payload_hint(), PayloadHint::NoStructPayload);
    assert!(a.http().is_none());
    let b = set.operation(&id("ex#B")).unwrap().input();
    assert!(b.has_body_members());
    assert_eq!(b.payload_hint(), PayloadHint::StructPayload);
}

#[test]
fn unwrapped_xml_output_is_operation_specific() {
    let set = build(&model(
        r#"
        "ex#Out": {"type": "structure", "members": {"v": {"target": "ex#String"}}},
        "ex#Wrapped": {"type": "operation", "output": {"target": "ex#Out"}},
        "ex#Unwrapped": {"type": "operation", "output": {"target": "ex#Out"},
            "traits": {"aws.customizations#s3UnwrappedXmlOutput": {}}}"#,
    ));
    let base = schema(&set, "ex#Out");
    let wrapped = set.operation(&id("ex#Wrapped")).unwrap().output();
    let unwrapped = set.operation(&id("ex#Unwrapped")).unwrap().output();
    assert!(std::ptr::eq(wrapped, base));
    assert!(!base.xml_unwrapped_output());
    assert!(unwrapped.xml_unwrapped_output());
    assert_eq!(unwrapped.shape_id().as_str(), "ex#Out");
    assert!(std::ptr::eq(unwrapped.members()[0], base.members()[0]));
}

const SERVICE: &str = r#"
    "ex#Service": {"type": "service", "operations": [{"target": "ex#Direct"}],
        "resources": [{"target": "ex#Resource"}], "errors": [{"target": "ex#Common"}]},
    "ex#Resource": {"type": "resource", "read": {"target": "ex#Bound"}},
    "ex#Direct": {"type": "operation", "input": {"target": "ex#In"},
        "errors": [{"target": "ex#Own"}, {"target": "ex#Common"}]},
    "ex#Bound": {"type": "operation"},
    "ex#Other": {"type": "operation", "input": {"target": "ex#Unrelated"}},
    "ex#In": {"type": "structure", "members": {"n": {"target": "ex#Nested"}}},
    "ex#Nested": {"type": "list", "member": {"target": "ex#String"}},
    "ex#Unrelated": {"type": "structure", "members": {}},
    "ex#Own": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "client"}},
    "ex#Common": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "server"}}
"#;

#[test]
fn service_scope_selects_the_service_closure() {
    let model = model(SERVICE);
    let set = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#Service"))
        .build()
        .unwrap();
    assert_eq!(set.service(), Some(&id("ex#Service")));
    assert_eq!(
        set.operations()
            .map(|o| o.id().to_string())
            .collect::<Vec<_>>(),
        ["ex#Bound", "ex#Direct"]
    );
    assert!(set.operation(&id("ex#Other")).is_none());
    assert!(set.schema(&id("ex#Unrelated")).is_none());
    for shape in ["ex#In", "ex#Nested", "ex#String", "ex#Own", "ex#Common"] {
        schema(&set, shape);
    }
    let errors: Vec<_> = set
        .operation(&id("ex#Direct"))
        .unwrap()
        .errors()
        .iter()
        .map(|e| e.shape_id().as_str())
        .collect();
    assert_eq!(errors, ["ex#Common", "ex#Own"]);
    let bound = set.operation(&id("ex#Bound")).unwrap();
    assert_eq!(bound.errors().len(), 1);

    // Without a service, operations list only their own errors.
    let all = build(&model);
    let errors: Vec<_> = all
        .operation(&id("ex#Direct"))
        .unwrap()
        .errors()
        .iter()
        .map(|e| e.shape_id().as_str())
        .collect();
    assert_eq!(errors, ["ex#Own", "ex#Common"]);
    assert!(all.operation(&id("ex#Bound")).unwrap().errors().is_empty());
    assert!(all.schema(&id("ex#Unrelated")).is_some());
}

#[test]
fn missing_or_wrong_service_is_an_error() {
    let model = model(SERVICE);
    let missing = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#Nope"))
        .build()
        .unwrap_err();
    assert_eq!(
        missing.to_string(),
        "failed to build runtime schemas: service `ex#Nope` does not exist"
    );
    let wrong = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#In"))
        .build()
        .unwrap_err();
    let diagnostic = wrong.diagnostics().iter().next().unwrap();
    assert_eq!(diagnostic.code(), DiagnosticCode::InvalidTarget);
    assert!(diagnostic
        .message()
        .contains("is a structure, not a service"));
    assert_eq!(diagnostic.source_location().unwrap().line(), Some(9));
}

/// A service whose operations are bound directly, through lifecycle, instance, and collection
/// bindings, through a nested resource, and through a resource bound twice.
const RESOURCES: &str = r#"
    "ex#Service": {"type": "service",
        "operations": [{"target": "ex#Direct"}],
        "resources": [{"target": "ex#Parent"}, {"target": "ex#Shared"}],
        "errors": [{"target": "ex#Common"}]},
    "ex#Parent": {"type": "resource",
        "create": {"target": "ex#Create"}, "put": {"target": "ex#Put"},
        "read": {"target": "ex#Read"}, "update": {"target": "ex#Update"},
        "delete": {"target": "ex#Delete"}, "list": {"target": "ex#List"},
        "operations": [{"target": "ex#Instance"}],
        "collectionOperations": [{"target": "ex#Collection"}],
        "resources": [{"target": "ex#Child"}, {"target": "ex#Shared"}]},
    "ex#Child": {"type": "resource", "operations": [{"target": "ex#Nested"}]},
    "ex#Shared": {"type": "resource", "read": {"target": "ex#SharedRead"}},
    "ex#Direct": {"type": "operation", "input": {"target": "ex#DirectInput"},
        "errors": [{"target": "ex#Own"}, {"target": "ex#Common"}]},
    "ex#Create": {"type": "operation"}, "ex#Put": {"type": "operation"},
    "ex#Read": {"type": "operation"}, "ex#Update": {"type": "operation"},
    "ex#Delete": {"type": "operation"}, "ex#List": {"type": "operation"},
    "ex#Instance": {"type": "operation"}, "ex#Collection": {"type": "operation"},
    "ex#Nested": {"type": "operation", "input": {"target": "ex#NestedInput"}},
    "ex#SharedRead": {"type": "operation"},
    "ex#Outside": {"type": "operation"},
    "ex#DirectInput": {"type": "structure", "members": {"n": {"target": "ex#Names"}}},
    "ex#Names": {"type": "list", "member": {"target": "ex#String"}},
    "ex#NestedInput": {"type": "structure", "members": {
        "bad": {"target": "ex#String", "traits": {"smithy.api#jsonName": 1}}}},
    "ex#Own": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "client"}},
    "ex#Common": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "server"}}
"#;

fn operation_ids(set: &RuntimeSchemaSet) -> Vec<String> {
    set.operations().map(|o| o.id().to_string()).collect()
}

fn filtered(model: &Model, operations: &[&str]) -> Result<RuntimeSchemaSet, SchemaBuildError> {
    let operations: Vec<_> = operations.iter().map(|o| id(o)).collect();
    RuntimeSchemaSet::builder(model)
        .for_service_operations(&id("ex#Service"), &operations)
        .build()
}

#[test]
fn containment_walk_matches_top_down() {
    let model = model(RESOURCES);
    let service = model
        .expect_shape(&id("ex#Service"))
        .unwrap()
        .as_service()
        .unwrap();
    let walked: Vec<_> = super::plan::contained_operations(&model, service)
        .iter()
        .map(|o| o.id().clone())
        .collect();
    let expected: Vec<_> = service
        .contained_operations()
        .map(|o| o.id().clone())
        .collect();
    assert_eq!(walked, expected);
    assert_eq!(walked.len(), 11);
    assert!(!walked.contains(&id("ex#Outside")));
}

#[test]
fn service_operations_selects_only_the_requested_closures() {
    let model = model(RESOURCES);
    // Out of order and duplicated; selected in service order without duplicates.
    let set = filtered(&model, &["ex#SharedRead", "ex#Direct", "ex#SharedRead"]).unwrap();
    assert_eq!(set.service(), Some(&id("ex#Service")));
    assert_eq!(operation_ids(&set), ["ex#Direct", "ex#SharedRead"]);
    assert!(set.operation(&id("ex#Nested")).is_none());
    // `ex#NestedInput` has a malformed trait, but only `ex#Nested` reaches it.
    assert!(set.schema(&id("ex#NestedInput")).is_none());
    for shape in [
        "ex#DirectInput",
        "ex#Names",
        "ex#String",
        "ex#Own",
        "ex#Common",
    ] {
        schema(&set, shape);
    }
    let errors = |operation: &str| -> Vec<&str> {
        set.operation(&id(operation))
            .unwrap()
            .errors()
            .iter()
            .map(|e| e.shape_id().as_str())
            .collect()
    };
    assert_eq!(errors("ex#Direct"), ["ex#Common", "ex#Own"]);
    assert_eq!(errors("ex#SharedRead"), ["ex#Common"]);

    // The full service scope reaches the malformed trait.
    let full = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#Service"))
        .max_diagnostics(10)
        .build()
        .unwrap_err();
    assert_eq!(
        full.diagnostics().len(),
        1,
        "the malformed trait fails the full scope"
    );
}

#[test]
fn service_operations_reaches_resource_bound_operations() {
    let model = model(RESOURCES);
    let every = [
        "ex#Create",
        "ex#Put",
        "ex#Read",
        "ex#Update",
        "ex#Delete",
        "ex#List",
        "ex#Instance",
        "ex#Collection",
        "ex#SharedRead",
    ];
    let set = filtered(&model, &every).unwrap();
    let mut expected: Vec<_> = every.iter().map(|o| o.to_string()).collect();
    expected.sort();
    assert_eq!(operation_ids(&set), expected);
    // A malformed trait in a selected closure fails the build.
    let error = filtered(&model, &["ex#Nested"]).unwrap_err();
    let diagnostic = error.diagnostics().iter().next().unwrap();
    assert_eq!(diagnostic.code(), DiagnosticCode::InvalidTrait);
}

#[test]
fn empty_service_operations_selects_nothing() {
    let model = model(RESOURCES);
    let set = filtered(&model, &[]).unwrap();
    assert_eq!(set.service(), Some(&id("ex#Service")));
    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
    assert_eq!(set.operations().len(), 0);
    // Common errors are only selected through operations that return them.
    assert!(set.schema(&id("ex#Common")).is_none());
    assert_eq!(set.storage.schema_count(), 0);
}

#[test]
fn invalid_service_operations_are_errors() {
    let model = model(RESOURCES);
    let error = filtered(&model, &["ex#Nope", "ex#Names", "ex#Outside", "ex#Direct"]).unwrap_err();
    let diagnostics: Vec<_> = error
        .diagnostics()
        .iter()
        .map(|d| {
            (
                d.code(),
                d.message().to_string(),
                d.source_location().is_some(),
            )
        })
        .collect();
    assert_eq!(
        diagnostics,
        [
            (
                DiagnosticCode::UnresolvedReference,
                "operation `ex#Nope` selected for service `ex#Service` does not exist".into(),
                false
            ),
            (
                DiagnosticCode::InvalidTarget,
                "`ex#Names` selected for service `ex#Service` is a list, not an operation".into(),
                true
            ),
            (
                DiagnosticCode::InvalidTarget,
                "operation `ex#Outside` is not contained in service `ex#Service`".into(),
                true
            ),
        ]
    );
    // The service itself is still checked first.
    let missing = RuntimeSchemaSet::builder(&model)
        .for_service_operations(&id("ex#Nope"), &[id("ex#Direct")])
        .build()
        .unwrap_err();
    assert_eq!(
        missing.to_string(),
        "failed to build runtime schemas: service `ex#Nope` does not exist"
    );
}

#[test]
fn last_scope_wins() {
    let model = model(RESOURCES);
    let set = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#Service"))
        .for_service_operations(&id("ex#Service"), &[id("ex#Direct")])
        .build()
        .unwrap();
    assert_eq!(operation_ids(&set), ["ex#Direct"]);
    let set = RuntimeSchemaSet::builder(&model)
        .for_service_operations(&id("ex#Service"), &[id("ex#Nested")])
        .all_shapes()
        .build();
    // All shapes includes the malformed trait.
    assert!(set.is_err());
}

#[test]
#[cfg_attr(miri, ignore = "parsing the prelude is slow under Miri")]
fn referenced_prelude_shapes_are_built() {
    let model = Model::from_json_str(
        "m.json",
        r#"{"smithy": "2.0", "shapes": {
            "ex#S": {"type": "structure", "members": {"a": {"target": "smithy.api#String"}}},
            "ex#Op": {"type": "operation", "input": {"target": "ex#S"}}
        }}"#,
    )
    .unwrap();
    let set = build(&model);
    assert_eq!(
        schema(&set, "smithy.api#String").shape_type(),
        ShapeType::String
    );
    assert_eq!(
        schema(&set, "smithy.api#Unit").shape_type(),
        ShapeType::Structure
    );
    assert!(set.schema(&id("smithy.api#Integer")).is_none());
    assert_eq!(set.len(), 3);
    let op = set.operation(&id("ex#Op")).unwrap();
    assert!(std::ptr::eq(op.output(), schema(&set, "smithy.api#Unit")));
}

// --- Ownership (Miri) ---

static SET_DROPS: AtomicUsize = AtomicUsize::new(0);
static SET_SENTINEL: SchemaExtensionKey<Sentinel> =
    SchemaExtensionKey::new(|_| Sentinel(&SET_DROPS));

/// Visits every materialized root and member schema, then every operation's schemas
/// (materializing them).
fn for_each_schema<'a>(set: &'a RuntimeSchemaSet, mut f: impl FnMut(&'a Schema<'a>)) {
    let roots = (0..set.plan.roots.len()).map(Slot::Root);
    let members = (0..set.plan.members.len()).map(Slot::Member);
    for slot in roots.chain(members) {
        if let Some(schema) = set.materialized(slot) {
            f(schema);
        }
    }
    for operation in set.operations() {
        f(operation.input());
        f(operation.output());
        operation.errors().iter().for_each(|error| f(error));
    }
}

/// INV-1, INV-4: a set can move, including to another thread, and every reference into it
/// stays valid while it lives.
#[test]
fn sets_move_between_threads() {
    let set = build(&model(OPERATIONS));
    let set = Box::new(set);
    let set = std::thread::spawn(move || {
        let read = set.operation(&id("ex#Read")).unwrap();
        assert_eq!(read.input().http().unwrap().uri(), "/read/{id}");
        set
    })
    .join()
    .unwrap();
    let mut names = Vec::new();
    for_each_schema(&set, |schema| {
        names.push(schema.shape_id().as_str());
        for member in schema.members() {
            names.push(member.member_name().unwrap());
        }
    });
    assert!(names.contains(&"ex#Shared$body") && names.contains(&"body"));
}

/// INV-3, INV-5: extensions are dropped exactly once with their set, across repeated builds.
#[test]
fn extensions_drop_with_the_set() {
    let model = model(OPERATIONS);
    let mut forced = 0;
    let before = SET_DROPS.load(Ordering::SeqCst);
    for _ in 0..2 {
        let set = build(&model);
        let schemas = set.storage.schema_count();
        let mut seen = std::collections::HashSet::new();
        for_each_schema(&set, |schema| {
            schema.extension(&SET_SENTINEL);
            if seen.insert(schema as *const Schema<'_>) {
                forced += 1;
            }
        });
        assert_eq!(seen.len(), schemas, "every schema is reachable");
        drop(set);
    }
    assert_eq!(SET_DROPS.load(Ordering::SeqCst) - before, forced);
}

/// INV-3, INV-5: a panic after any allocation releases everything allocated so far, once.
/// Miri reports any leak or double free.
#[test]
fn a_panic_during_any_allocation_releases_the_partial_graph() {
    let model = model(SERVICE);
    let allocations = count_allocations(|| {
        build(&model);
    });
    assert!(allocations > 10, "{allocations}");
    for fail_after in 0..allocations {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_panic_after(fail_after, || build(&model))
        }));
        assert!(result.is_err(), "allocation {fail_after} did not fail");
    }
    // The model is unaffected and can still be built.
    build(&model);
}

/// The header-name invariant: an `@httpHeader` name is interned, never borrowed from the
/// model, so `value_static` returns a name that outlives the set and the model.
#[test]
fn header_names_outlive_the_set() {
    let name: &'static str = {
        let model = model(
            r#""ex#S": {"type": "structure", "members": {
                "h": {"target": "ex#String", "traits": {"smithy.api#httpHeader": "x-runtime-schema-test"}}
            }}"#,
        );
        let set = build(&model);
        let header = schema(&set, "ex#S$h").http_header().unwrap();
        let name = header
            .value_static()
            .expect("interned header names are static");
        assert!(std::ptr::eq(name, header.value()));
        name
    };
    assert_eq!(name, "x-runtime-schema-test");
    assert!(std::ptr::eq(
        name,
        aws_smithy_schema::intern_header_name(name)
    ));
}

#[test]
fn owned_and_borrowed_conversions_agree() {
    let model = model(OPERATIONS);
    let borrowed = RuntimeSchemaSet::try_from(&model).unwrap();
    let owned = RuntimeSchemaSet::try_from(model.clone()).unwrap();
    assert_eq!(format!("{borrowed:?}"), format!("{owned:?}"));
    assert!(borrowed.model().equivalent(&model));
    assert_eq!(
        format!("{borrowed:?}"),
        "RuntimeSchemaSet { service: None, materialization: Eager, roots: 3, operations: 4, built_schemas: 9, built_operations: 4, .. }"
    );
    let read = borrowed.operation(&id("ex#Read")).unwrap();
    assert_eq!(
        format!("{read:?}"),
        r#"OperationSchema { id: "ex#Read", input: "ex#Shared", output: "smithy.api#Unit", errors: [] }"#
    );
}

#[test]
fn operation_descriptors_borrow_the_model_id() {
    let set = build(&model(OPERATIONS));
    let schemas = set.storage.schema_count();
    for operation in set.operations() {
        let model_id = set
            .plan
            .operations
            .get_key_value(operation.id().as_str())
            .unwrap()
            .0;
        let schema_id = operation.id();
        // Every component, so inconsistent `from_parts` arguments cannot pass.
        assert_eq!(schema_id.as_str(), model_id.as_str());
        assert_eq!(schema_id.namespace(), model_id.namespace());
        assert_eq!(schema_id.shape_name(), model_id.name());
        assert_eq!(schema_id.member_name(), None);
        // Borrowed from the model, not copied.
        assert!(std::ptr::eq(schema_id.as_str(), model_id.as_str()));
    }
    // Descriptors are built by value and allocate nothing in the set.
    let _ = set.operation(&id("ex#Read")).unwrap();
    assert_eq!(set.storage.schema_count(), schemas);
}

// --- Lazy materialization (Miri) ---

/// Operations that share inputs, nested collections, outputs, and errors. `ex#Lonely` is not
/// reachable from the service.
const LAZY: &str = r#"
    "ex#Service": {"type": "service", "operations": [
        {"target": "ex#A"}, {"target": "ex#B"}, {"target": "ex#C"}, {"target": "ex#D"}]},
    "ex#A": {"type": "operation", "input": {"target": "ex#AIn"}, "output": {"target": "ex#Out"},
        "errors": [{"target": "ex#Err"}],
        "traits": {"smithy.api#http": {"method": "POST", "uri": "/a"}}},
    "ex#B": {"type": "operation", "input": {"target": "ex#BIn"}, "output": {"target": "ex#Out"}},
    "ex#C": {"type": "operation", "input": {"target": "ex#Shared"},
        "traits": {"smithy.api#http": {"method": "GET", "uri": "/c"}}},
    "ex#D": {"type": "operation", "input": {"target": "ex#Shared"},
        "traits": {"smithy.api#http": {"method": "GET", "uri": "/d"}}},
    "ex#AIn": {"type": "structure", "members": {
        "shared": {"target": "ex#Shared"},
        "names": {"target": "ex#Names", "traits": {"smithy.api#httpHeader": "x-names"}}}},
    "ex#BIn": {"type": "structure", "members": {"shared": {"target": "ex#Shared"}}},
    "ex#Shared": {"type": "structure", "members": {
        "list": {"target": "ex#Names"}, "map": {"target": "ex#Map"}}},
    "ex#Names": {"type": "list", "member": {"target": "ex#String"}},
    "ex#Map": {"type": "map", "key": {"target": "ex#String"}, "value": {"target": "ex#Names"}},
    "ex#Out": {"type": "structure", "members": {"v": {"target": "ex#String",
        "traits": {"smithy.api#jsonName": "V"}}}},
    "ex#Err": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "client"}},
    "ex#Lonely": {"type": "structure", "members": {}}
"#;

fn service_set(model: &Model, operations: Option<&[&str]>, lazy: bool) -> RuntimeSchemaSet {
    let service = id("ex#Service");
    let builder = RuntimeSchemaSet::builder(model);
    let builder = match operations {
        None => builder.for_service(&service),
        Some(operations) => {
            let operations: Vec<_> = operations.iter().map(|o| id(o)).collect();
            builder.for_service_operations(&service, &operations)
        }
    };
    builder
        .materialization(materialization(lazy))
        .build()
        .unwrap_or_else(|err| panic!("{err}"))
}

fn materialization(lazy: bool) -> Materialization {
    if lazy {
        Materialization::Lazy
    } else {
        Materialization::Eager
    }
}

fn slots(set: &RuntimeSchemaSet) -> impl Iterator<Item = Slot> {
    let roots = (0..set.plan.roots.len()).map(Slot::Root);
    roots.chain((0..set.plan.members.len()).map(Slot::Member))
}

fn materialized_operations(set: &RuntimeSchemaSet) -> usize {
    set.plan
        .operations
        .values()
        .filter(|operation| operation.record.get().is_some())
        .count()
}

/// How closely [`assert_matches_eager`] compares schemas. Formatting is slow under Miri.
#[derive(Clone, Copy, PartialEq)]
enum Compare {
    /// Complete `Debug` output.
    Debug,
    /// [`fingerprint`]s.
    Fingerprint,
    /// Shape IDs, types, and member counts.
    Structure,
}

/// Every materialized schema of `lazy` equals the eager set's schema in the same slot,
/// references the published schemas of its members, and every materialized operation equals
/// the eager operation.
fn assert_matches_eager(lazy: &RuntimeSchemaSet, eager: &RuntimeSchemaSet, compare: Compare) {
    let same =
        |actual: &Schema<'_>, expected: &Schema<'_>, context: &dyn std::fmt::Debug| match compare {
            Compare::Debug => assert_eq!(
                format!("{actual:?}"),
                format!("{expected:?}"),
                "{context:?}"
            ),
            Compare::Fingerprint => {
                assert_eq!(fingerprint(actual), fingerprint(expected), "{context:?}")
            }
            Compare::Structure => {
                assert_eq!(actual.shape_id().as_str(), expected.shape_id().as_str());
                assert_eq!(actual.shape_type(), expected.shape_type(), "{context:?}");
                assert_eq!(actual.members().len(), expected.members().len());
            }
        };
    assert_eq!(lazy.len(), eager.len());
    for slot in slots(lazy) {
        let Some(schema) = lazy.materialized(slot) else {
            continue;
        };
        same(schema, eager.materialized(slot).unwrap(), &slot);
        if let Slot::Root(root) = slot {
            let start = lazy.plan.roots[root].member_start as usize;
            for (i, member) in schema.members().iter().enumerate() {
                let published = lazy.materialized(Slot::Member(start + i)).unwrap();
                assert!(std::ptr::eq(*member, published), "{slot:?}");
            }
        }
    }
    for (index, (id, operation)) in lazy.plan.operations.iter().enumerate() {
        if operation.record.get().is_some() {
            let actual = lazy.operation_schema(index, id);
            let expected = eager.operation(id).unwrap();
            if compare != Compare::Structure {
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            }
            same(actual.input(), expected.input(), id);
            same(actual.output(), expected.output(), id);
        }
    }
}

#[test]
fn lazy_sets_allocate_nothing_until_used() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let eager = service_set(&model, None, false);
    assert_eq!(lazy.storage.schema_count(), 0);
    assert_eq!(lazy.len(), eager.len());
    assert!(!lazy.is_empty());
    assert_eq!(lazy.service(), eager.service());
    // Debug neither materializes nor hides that nothing is materialized.
    assert_eq!(
        format!("{lazy:?}"),
        "RuntimeSchemaSet { service: Some(ShapeId(\"ex#Service\")), materialization: Lazy, roots: 9, operations: 4, built_schemas: 0, built_operations: 0, .. }"
    );
    assert_eq!(lazy.storage.schema_count(), 0);

    // Operation IDs build nothing, and come in service order, not the order requested.
    let ids: Vec<_> = lazy.operation_ids().map(ShapeId::as_str).collect();
    assert_eq!(ids, ["ex#A", "ex#B", "ex#C", "ex#D"]);
    assert_eq!(lazy.operation_ids().len(), 4);
    let filtered = service_set(&model, Some(&["ex#C", "ex#A", "ex#C"]), true);
    let ids: Vec<_> = filtered.operation_ids().map(ShapeId::as_str).collect();
    assert_eq!(ids, ["ex#A", "ex#C"]);
    assert_eq!(filtered.storage.schema_count(), 0);
    assert_eq!(lazy.storage.schema_count(), 0);

    // Debug counts what is built, without building anything.
    lazy.operation(&id("ex#B")).unwrap();
    let built = lazy.storage.schema_count();
    assert_eq!(
        format!("{lazy:?}"),
        format!(
            "RuntimeSchemaSet {{ service: Some(ShapeId(\"ex#Service\")), materialization: Lazy, \
             roots: 9, operations: 4, built_schemas: {built}, built_operations: 1, .. }}"
        )
    );
    assert_eq!(lazy.storage.schema_count(), built);
    // INV-3: dropping a partly materialized set is fine (Miri checks for leaks).
}

#[test]
fn first_operation_builds_only_its_closure() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let b = lazy.operation(&id("ex#B")).unwrap();
    // Exactly what an eager set of only `ex#B` contains.
    let only_b = service_set(&model, Some(&["ex#B"]), false);
    assert_eq!(lazy.storage.schema_count(), only_b.storage.schema_count());
    assert_eq!(materialized_operations(&lazy), 1);
    assert!(lazy
        .materialized(lazy.plan.slot(model.data(), &id("ex#AIn")).unwrap())
        .is_none());
    // A second lookup allocates nothing and returns the same schemas.
    let input = b.input();
    let again = lazy.operation(&id("ex#B")).unwrap();
    assert!(std::ptr::eq(input, again.input()));
    assert!(std::ptr::eq(b.output(), again.output()));
    assert_eq!(lazy.storage.schema_count(), only_b.storage.schema_count());
    assert_matches_eager(&lazy, &service_set(&model, None, false), Compare::Debug);
}

#[test]
fn later_operations_share_built_schemas() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let b = lazy.operation(&id("ex#B")).unwrap();
    let shared = schema(&lazy, "ex#Shared");
    let names_member = schema(&lazy, "ex#Names$member");
    let a = lazy.operation(&id("ex#A")).unwrap();
    // Only the schemas `ex#A` does not share with `ex#B` are added.
    let a_and_b = service_set(&model, Some(&["ex#A", "ex#B"]), false);
    assert_eq!(lazy.storage.schema_count(), a_and_b.storage.schema_count());
    assert!(std::ptr::eq(shared, schema(&lazy, "ex#Shared")));
    let names = a.input().member_schema("names").unwrap();
    assert!(std::ptr::eq(names.member().unwrap(), names_member));
    assert!(std::ptr::eq(a.output(), b.output()));
    assert_eq!(a.errors()[0].shape_id().as_str(), "ex#Err");

    // `ex#C` and `ex#D` share an input structure but get their own inputs.
    let c = lazy.operation(&id("ex#C")).unwrap();
    let d = lazy.operation(&id("ex#D")).unwrap();
    assert!(!std::ptr::eq(c.input(), d.input()));
    assert!(!std::ptr::eq(c.input(), shared));
    assert!(std::ptr::eq(c.input().members()[0], shared.members()[0]));
    assert_eq!(d.input().http().unwrap().uri(), "/d");

    // Everything materialized lazily is the eager set.
    let eager = service_set(&model, None, false);
    assert_eq!(lazy.storage.schema_count(), eager.storage.schema_count());
    assert_matches_eager(&lazy, &eager, Compare::Fingerprint);
}

#[test]
fn lazy_schema_lookup_builds_the_containing_closure() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    // A member builds its container and everything the container reaches.
    let member = schema(&lazy, "ex#Names$member");
    assert_eq!(member.member_name(), Some("member"));
    // `ex#Names`, its member, and `ex#String`.
    assert_eq!(lazy.storage.schema_count(), 3);
    assert_eq!(materialized_operations(&lazy), 0);
    assert!(std::ptr::eq(
        schema(&lazy, "ex#Names").member().unwrap(),
        member
    ));
    assert_eq!(lazy.storage.schema_count(), 3);
    // Unselected or nonexistent IDs are `None`, cold or not.
    for missing in [
        "ex#Lonely",
        "ex#Names$nope",
        "ex#Nope",
        "ex#A",
        "ex#Service",
    ] {
        assert!(lazy.schema(&id(missing)).is_none(), "{missing}");
    }
    assert_eq!(lazy.storage.schema_count(), 3);
    assert_matches_eager(&lazy, &service_set(&model, None, false), Compare::Debug);
}

#[test]
fn operations_iterator_materializes_as_it_goes() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let mut operations = lazy.operations();
    assert_eq!(operations.len(), 4);
    assert_eq!(materialized_operations(&lazy), 0);
    let first = operations.next().unwrap();
    assert_eq!(first.id().as_str(), "ex#A");
    assert_eq!(materialized_operations(&lazy), 1);
    drop(operations);
    assert_eq!(materialized_operations(&lazy), 1);
    // `take(1)` builds only the first operation, even on a fresh set.
    let lazy = service_set(&model, None, true);
    assert_eq!(lazy.operations().take(1).count(), 1);
    assert_eq!(materialized_operations(&lazy), 1);
}

/// The three scopes of the `LAZY` model: every shape, the service, and one operation.
fn scopes(
    builder: super::RuntimeSchemaSetBuilder<'_>,
    scope: usize,
) -> super::RuntimeSchemaSetBuilder<'_> {
    match scope {
        0 => builder.all_shapes(),
        1 => builder.for_service(&id("ex#Service")),
        _ => builder.for_service_operations(&id("ex#Service"), &[id("ex#B")]),
    }
}

fn fully_materialized(set: &RuntimeSchemaSet) -> bool {
    slots(set).all(|slot| set.materialized(slot).is_some())
        && materialized_operations(set) == set.plan.operations.len()
}

#[test]
fn eager_is_the_default_for_every_scope() {
    let model = model(LAZY);
    let from = RuntimeSchemaSet::try_from(&model).unwrap();
    assert_eq!(from.materialization(), Materialization::Eager);
    assert!(fully_materialized(&from));
    for scope in 0..3 {
        let set = scopes(RuntimeSchemaSet::builder(&model), scope)
            .build()
            .unwrap();
        assert_eq!(
            set.materialization(),
            Materialization::Eager,
            "scope {scope}"
        );
        assert!(fully_materialized(&set), "scope {scope}");
        // Lookups find everything built and allocate nothing.
        let schemas = set.storage.schema_count();
        let allocations = count_allocations(|| {
            for id in set.operation_ids() {
                set.operation(id).unwrap();
            }
            schema(&set, "ex#Shared$list");
            set.materialize_all();
        });
        assert_eq!(
            (allocations, set.storage.schema_count()),
            (0, schemas),
            "scope {scope}"
        );
    }
}

#[test]
fn lazy_is_honored_for_every_scope_in_any_order() {
    let model = model(LAZY);
    for scope in 0..3 {
        let eager = scopes(RuntimeSchemaSet::builder(&model), scope)
            .build()
            .unwrap();
        let before = scopes(
            RuntimeSchemaSet::builder(&model).materialization(Materialization::Lazy),
            scope,
        );
        let after =
            scopes(RuntimeSchemaSet::builder(&model), scope).materialization(Materialization::Lazy);
        for builder in [before, after] {
            let lazy = builder.build().unwrap();
            assert_eq!(
                lazy.materialization(),
                Materialization::Lazy,
                "scope {scope}"
            );
            assert_eq!(lazy.storage.schema_count(), 0, "scope {scope}");
            assert_eq!(lazy.len(), eager.len(), "scope {scope}");
            assert!(
                lazy.operation_ids().eq(eager.operation_ids()),
                "scope {scope}"
            );
            lazy.materialize_all();
            assert!(fully_materialized(&lazy), "scope {scope}");
            assert_eq!(
                lazy.storage.schema_count(),
                eager.storage.schema_count(),
                "scope {scope}"
            );
            assert_matches_eager(&lazy, &eager, Compare::Structure);
        }
    }
    // A later choice replaces an earlier one.
    let set = RuntimeSchemaSet::builder(&model)
        .materialization(Materialization::Lazy)
        .materialization(Materialization::Eager)
        .build()
        .unwrap();
    assert!(fully_materialized(&set));
}

#[test]
fn materialize_all_completes_a_partly_built_set_once() {
    let model = model(LAZY);
    let eager = service_set(&model, None, false);
    let lazy = service_set(&model, None, true);
    let b = lazy.operation(&id("ex#B")).unwrap();
    let shared = schema(&lazy, "ex#Shared");
    lazy.materialize_all();
    assert!(fully_materialized(&lazy));
    assert_eq!(lazy.storage.schema_count(), eager.storage.schema_count());
    // What was built before is kept, not rebuilt.
    assert!(std::ptr::eq(
        b.input(),
        lazy.operation(&id("ex#B")).unwrap().input()
    ));
    assert!(std::ptr::eq(shared, schema(&lazy, "ex#Shared")));
    assert_matches_eager(&lazy, &eager, Compare::Fingerprint);
    // A second call does nothing.
    let schemas = lazy.storage.schema_count();
    assert_eq!(count_allocations(|| lazy.materialize_all()), 0);
    assert_eq!(lazy.storage.schema_count(), schemas);
}

/// INV-6, INV-7, INV-8: threads racing to materialize the same operation observe one set of
/// schemas, and threads materializing different operations share the schemas they have in
/// common.
#[test]
fn concurrent_materialization() {
    let model = model(LAZY);
    let eager = service_set(&model, None, false);
    let one = service_set(&model, Some(&["ex#A"]), false);

    let lazy = service_set(&model, None, true);
    let inputs: Vec<usize> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..3)
            .map(|_| {
                let lazy = &lazy;
                scope.spawn(move || {
                    let a = lazy.operation(&id("ex#A")).unwrap();
                    assert_eq!(a.input().http().unwrap().uri(), "/a");
                    a.input() as *const Schema<'_> as usize
                })
            })
            .collect();
        threads.into_iter().map(|t| t.join().unwrap()).collect()
    });
    assert!(inputs.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(lazy.storage.schema_count(), one.storage.schema_count());

    let lazy = service_set(&model, None, true);
    std::thread::scope(|scope| {
        for operation in ["ex#A", "ex#B", "ex#C", "ex#D"] {
            let lazy = &lazy;
            scope.spawn(move || {
                lazy.operation(&id(operation)).unwrap();
                schema(lazy, "ex#Shared");
            });
        }
    });
    assert_eq!(lazy.storage.schema_count(), eager.storage.schema_count());
    assert_matches_eager(&lazy, &eager, Compare::Structure);

    // `materialize_all` racing with lookups builds each schema once.
    let lazy = service_set(&model, None, true);
    std::thread::scope(|scope| {
        let set = &lazy;
        scope.spawn(move || set.materialize_all());
        for operation in ["ex#D", "ex#A"] {
            scope.spawn(move || {
                set.operation(&id(operation)).unwrap();
            });
        }
    });
    lazy.materialize_all();
    assert_eq!(lazy.storage.schema_count(), eager.storage.schema_count());
    assert_matches_eager(&lazy, &eager, Compare::Structure);
}

/// INV-6, INV-7: a thread that reads a root or member slot another thread is publishing,
/// with no operation record between them, sees the complete schema. Each thread looks up
/// schemas directly, in a different order, so the only ordering between the publishing batch
/// and the reader is the slot's own release and acquire. The other thread tests mostly
/// reach schemas through operation records, whose `OnceLock` already orders them, so they
/// pass with relaxed slot accesses; this test, under Miri's schedules, does not.
#[test]
fn readers_race_slot_publication() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let order = ["ex#Shared", "ex#Names", "ex#Map", "ex#AIn$shared", "ex#Out"];
    std::thread::scope(|scope| {
        for thread in 0..3 {
            let lazy = &lazy;
            scope.spawn(move || {
                for i in 0..order.len() {
                    let name = order[(i + thread * 2) % order.len()];
                    let schema = schema(lazy, name);
                    assert_eq!(schema.shape_id().as_str(), name);
                    for member in schema.members() {
                        assert!(member.member_name().is_some());
                    }
                    // A list's member, or a map's value.
                    if let Some(member) = schema.member() {
                        assert!(member.member_name().is_some());
                    }
                }
            });
        }
    });
    let eager = service_set(&model, None, false);
    lazy.materialize_all();
    assert_eq!(lazy.storage.schema_count(), eager.storage.schema_count());
}

/// A model shaped like a small AWS service, without the prelude: `operations` operations,
/// each with its own input (with an `@httpHeader` member) and `@http`, sharing a common
/// output and error and `shared` structures that refer to each other through lists and maps.
/// Under Miri it loads in about a second; the SQS model and the prelude take about 7 minutes.
fn service_like_model(operations: usize, shared: usize) -> Model {
    let targets: Vec<String> = (0..operations)
        .map(|i| format!(r#"{{"target": "ex#Op{i}"}}"#))
        .collect();
    let mut shapes = vec![
        format!(
            r#""ex#Service": {{"type": "service", "operations": [{}]}}"#,
            targets.join(", ")
        ),
        r#""ex#Out": {"type": "structure", "members": {"v": {"target": "ex#String",
            "traits": {"smithy.api#jsonName": "V"}}}}"#
            .into(),
        r#""ex#Err": {"type": "structure", "members": {"message": {"target": "ex#String"}},
            "traits": {"smithy.api#error": "client"}}"#
            .into(),
    ];
    for i in 0..shared {
        let next = (i + 1) % shared;
        shapes.push(format!(
            r#""ex#S{i}": {{"type": "structure", "members": {{
                "name": {{"target": "ex#String"}},
                "items": {{"target": "ex#L{i}"}},
                "byName": {{"target": "ex#M{i}"}}}}}},
            "ex#L{i}": {{"type": "list", "member": {{"target": "ex#S{next}"}}}},
            "ex#M{i}": {{"type": "map", "key": {{"target": "ex#String"}},
                "value": {{"target": "ex#L{next}"}}}}"#
        ));
    }
    for i in 0..operations {
        let (a, b) = (i % shared, (i * 3 + 1) % shared);
        shapes.push(format!(
            r#""ex#Op{i}": {{"type": "operation", "input": {{"target": "ex#In{i}"}},
                "output": {{"target": "ex#Out"}}, "errors": [{{"target": "ex#Err"}}],
                "traits": {{"smithy.api#http": {{"method": "POST", "uri": "/op{i}"}}}}}},
            "ex#In{i}": {{"type": "structure", "members": {{
                "first": {{"target": "ex#S{a}"}},
                "second": {{"target": "ex#S{b}"}},
                "list": {{"target": "ex#L{b}"}},
                "token": {{"target": "ex#String",
                    "traits": {{"smithy.api#httpHeader": "x-token-{i}"}}}}}}}}"#
        ));
    }
    model(&shapes.join(",\n"))
}

/// INV-6, INV-7, INV-8 under real contention: in every round, eight threads start together on
/// a fresh lazy set. Each looks up every root schema directly (so readers load slots that
/// other threads publish, with no operation record ordering them), then every operation and
/// its input members, each thread in a different order, one of them racing `materialize_all`.
/// Every round must end with the eager set's schemas, each built once.
///
/// Natively (and under the sanitizers in `additional-ci`) this uses the SQS model. Under Miri,
/// where parsing SQS and the prelude takes about 7 minutes, it uses `service_like_model`.
#[test]
fn racing_threads_on_a_service() {
    const THREADS: usize = 8;
    let model = if cfg!(miri) {
        service_like_model(8, 4)
    } else {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aws/sdk/aws-models/sqs.json");
        Model::from_json_file(&path).unwrap()
    };
    let service = model.services().next().unwrap().id().clone();
    let build = |materialization| {
        RuntimeSchemaSet::builder(&model)
            .for_service(&service)
            .materialization(materialization)
            .build()
            .unwrap()
    };
    let eager = build(Materialization::Eager);
    let ids: Vec<ShapeId> = eager.operation_ids().cloned().collect();
    let data = model.data();
    let roots: Vec<ShapeId> = eager
        .plan
        .roots
        .iter()
        .map(|root| data.root_at(root.model_position as usize).id().clone())
        .collect();
    let rounds = std::env::var("RUNTIME_SCHEMA_STRESS_ROUNDS")
        .ok()
        .map_or(if cfg!(miri) { 1 } else { 20 }, |rounds| {
            rounds.parse().unwrap()
        });
    for round in 0..rounds {
        let lazy = build(Materialization::Lazy);
        let barrier = std::sync::Barrier::new(THREADS);
        let inputs: Vec<Vec<usize>> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..THREADS)
                .map(|thread| {
                    let (lazy, ids, roots, barrier) = (&lazy, &ids, &roots, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        // Rotate and, on odd threads, reverse, so that threads collide on
                        // the same schemas and operations at different times.
                        let order = |len: usize| {
                            let mut order: Vec<_> = (0..len)
                                .map(|i| (i + thread * len / THREADS) % len)
                                .collect();
                            if thread % 2 == 1 {
                                order.reverse();
                            }
                            order
                        };
                        for i in order(roots.len()) {
                            let root = lazy.schema(&roots[i]).unwrap();
                            assert_eq!(root.shape_id().as_str(), roots[i].as_str());
                            for member in root.members() {
                                assert!(member.member_name().is_some());
                            }
                        }
                        let mut inputs = vec![0; ids.len()];
                        for i in order(ids.len()) {
                            let input = lazy.operation(&ids[i]).unwrap().input();
                            for member in input.members() {
                                let id = ShapeId::new(member.shape_id().as_str()).unwrap();
                                let looked_up = lazy.schema(&id).unwrap();
                                assert_eq!(looked_up.member_name(), member.member_name());
                            }
                            inputs[i] = input as *const Schema<'_> as usize;
                        }
                        // Last, so that it races with the other threads' lookups rather than
                        // building everything before they start.
                        if thread == 0 {
                            lazy.materialize_all();
                        }
                        inputs
                    })
                })
                .collect();
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert!(
            inputs.windows(2).all(|pair| pair[0] == pair[1]),
            "round {round}"
        );
        assert!(fully_materialized(&lazy), "round {round}");
        assert_eq!(
            lazy.storage.schema_count(),
            eager.storage.schema_count(),
            "round {round}"
        );
    }
}

/// INV-1, INV-2, INV-7: a reference stays valid while another thread adds schemas, and a
/// partially materialized set can move between threads without moving its schemas.
#[test]
fn references_survive_appends_and_moves() {
    let model = model(LAZY);
    let lazy = service_set(&model, None, true);
    let b = lazy.operation(&id("ex#B")).unwrap().input();
    let address = b as *const Schema<'_> as usize;
    std::thread::scope(|scope| {
        let lazy = &lazy;
        scope.spawn(move || {
            for operation in ["ex#A", "ex#C", "ex#D"] {
                lazy.operation(&id(operation)).unwrap();
            }
        });
        for _ in 0..10 {
            assert_eq!(b.members()[0].member_name(), Some("shared"));
        }
    });
    assert_eq!(b.shape_id().as_str(), "ex#BIn");

    let lazy = service_set(&model, None, true);
    let b = lazy.operation(&id("ex#B")).unwrap().input() as *const Schema<'_> as usize;
    let lazy = std::thread::spawn(move || {
        let lazy = Box::new(lazy);
        lazy.operation(&id("ex#A")).unwrap();
        lazy
    })
    .join()
    .unwrap();
    let after = lazy.operation(&id("ex#B")).unwrap().input() as *const Schema<'_> as usize;
    assert_eq!(after, b);
    assert_ne!(address, 0);
}

/// INV-3, INV-5: extensions forced before later batches are dropped exactly once.
#[test]
fn extensions_forced_between_batches_drop_once() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    static KEY: SchemaExtensionKey<Sentinel> = SchemaExtensionKey::new(|_| Sentinel(&DROPS));
    let model = model(LAZY);
    let before = DROPS.load(Ordering::SeqCst);
    let lazy = service_set(&model, None, true);
    let mut seen = std::collections::HashSet::new();
    for operation in ["ex#B", "ex#A", "ex#C", "ex#D"] {
        lazy.operation(&id(operation)).unwrap();
        for_each_schema(&lazy, |schema| {
            schema.extension(&KEY);
            seen.insert(schema as *const Schema<'_> as usize);
        });
    }
    assert_eq!(seen.len(), lazy.storage.schema_count());
    drop(lazy);
    assert_eq!(DROPS.load(Ordering::SeqCst) - before, seen.len());
}

/// INV-3, INV-6, INV-8: a panic after any allocation of a lazy operation leaves only complete
/// schemas published and no operation record; the poisoned batch lock is recovered, the same
/// and other operations then materialize correctly, and everything is released once.
#[test]
fn a_panic_during_lazy_materialization_can_be_retried() {
    let model = model(LAZY);
    let eager = service_set(&model, None, false);
    let expected = service_set(&model, Some(&["ex#A", "ex#B"]), false)
        .storage
        .schema_count();
    let allocations = count_allocations(|| {
        service_set(&model, None, true).operation(&id("ex#A"));
    });
    assert!(allocations > 10, "{allocations}");
    for fail_after in 0..allocations {
        let lazy = service_set(&model, None, true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_panic_after(fail_after, || lazy.operation(&id("ex#A")))
        }));
        assert!(result.is_err(), "allocation {fail_after} did not fail");
        assert_eq!(materialized_operations(&lazy), 0);
        // Whatever was published is complete and correct.
        assert_matches_eager(&lazy, &eager, Compare::Structure);
        let a = lazy.operation(&id("ex#A")).unwrap();
        assert_eq!(a.input().http().unwrap().uri(), "/a");
        lazy.operation(&id("ex#B")).unwrap();
        let compare = if fail_after + 1 == allocations {
            Compare::Fingerprint
        } else {
            Compare::Structure
        };
        assert_matches_eager(&lazy, &eager, compare);
        // At most the one allocation that panicked is unused.
        let count = lazy.storage.schema_count();
        assert!(count == expected || count == expected + 1, "{count}");
    }
}

#[test]
fn plans_too_large_to_index_are_resource_limits() {
    let model = model(LAZY);
    let error = RuntimeSchemaSet::builder(&model)
        .max_plan_entries(5)
        .build()
        .unwrap_err();
    let diagnostic = error.diagnostics().iter().next().unwrap();
    assert_eq!(diagnostic.code(), DiagnosticCode::ResourceLimit);
    assert!(
        diagnostic.message().contains("more than the limit of 5"),
        "{}",
        diagnostic.message()
    );
    let filtered = RuntimeSchemaSet::builder(&model)
        .max_plan_entries(14)
        .for_service_operations(&id("ex#Service"), &[id("ex#B")])
        .build();
    // The model has 15 shapes; one operation selects far fewer, but positions in the whole
    // model must be indexable.
    assert!(filtered.is_err(), "the whole model must be indexable");
}

/// A non-recursive summary of the wire-relevant parts of a schema, for comparing large sets.
fn fingerprint(schema: &Schema<'_>) -> String {
    let members: Vec<_> = schema
        .members()
        .iter()
        .map(|member| member.shape_id().as_str())
        .collect();
    format!(
        "{} {:?} {:?} {:?} {members:?} list={:?} key={:?} http={:?} json={:?} xml={:?} \
         header={:?} format={:?} payload={:?} body={} unwrapped={} sensitive={}",
        schema.shape_id().as_str(),
        schema.shape_type(),
        schema.member_name(),
        schema.member_index(),
        schema.member().map(|m| m.shape_id().as_str()),
        schema.key().map(|m| m.shape_id().as_str()),
        schema.http().map(|h| (h.method(), h.uri(), h.code())),
        schema.json_name().map(|t| t.value()),
        schema.xml_name().map(|t| t.value()),
        schema.http_header().map(|t| t.value()),
        schema.timestamp_format().map(|t| t.format()),
        schema.payload_hint(),
        schema.has_body_members(),
        schema.xml_unwrapped_output(),
        schema.sensitive().is_some(),
    )
}

/// Materializing every service operation lazily, in an order unlike the eager one and
/// interleaved with schema lookups, gives the eager set: the same schemas in every slot, the
/// same number of them, and the same sharing.
#[test]
#[cfg_attr(miri, ignore = "loads the AWS model corpus")]
fn lazy_matches_eager_on_the_corpus() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let mut checked = 0;
    for path in paths {
        // Other JSON files there are not models; `tests/corpus.rs` checks that every model
        // loads.
        let Ok(model) = Model::from_json_file(&path) else {
            continue;
        };
        let Some(service) = model.services().next() else {
            continue;
        };
        let name = path.display();
        let build = |lazy: bool| {
            RuntimeSchemaSet::builder(&model)
                .for_service(service.id())
                .materialization(materialization(lazy))
                .build()
                .unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let eager = build(false);
        let prewarmed = build(true);
        prewarmed.materialize_all();
        assert_eq!(
            prewarmed.storage.schema_count(),
            eager.storage.schema_count(),
            "{name}"
        );
        drop(prewarmed);
        let lazy = build(true);
        assert_eq!(lazy.storage.schema_count(), 0, "{name}");
        // Every other operation from the end, a few schemas, then the rest from the start.
        let ids: Vec<ShapeId> = lazy.plan.operations.keys().cloned().collect();
        for id in ids.iter().rev().step_by(2) {
            lazy.operation(id).unwrap();
        }
        let data = model.data();
        for root in lazy.plan.roots.iter().step_by(7) {
            lazy.schema(data.root_at(root.model_position as usize).id())
                .unwrap();
        }
        for operation in lazy.operations() {
            let _ = operation.input();
        }
        assert_eq!(
            lazy.storage.schema_count(),
            eager.storage.schema_count(),
            "{name}"
        );
        for slot in slots(&lazy) {
            let actual = lazy.materialized(slot).unwrap();
            let expected = eager.materialized(slot).unwrap();
            assert_eq!(
                fingerprint(actual),
                fingerprint(expected),
                "{name}: {slot:?}"
            );
            // Roots reference the published member schemas.
            if let Slot::Root(root) = slot {
                let start = lazy.plan.roots[root].member_start as usize;
                for (i, member) in actual.members().iter().enumerate() {
                    let published = lazy.materialized(Slot::Member(start + i)).unwrap();
                    assert!(std::ptr::eq(*member, published), "{name}: {slot:?}");
                }
            }
        }
        for (actual, expected) in lazy.operations().zip(eager.operations()) {
            assert_eq!(actual.id().as_str(), expected.id().as_str());
            for (a, e) in [
                (actual.input(), expected.input()),
                (actual.output(), expected.output()),
            ] {
                assert_eq!(fingerprint(a), fingerprint(e), "{name}");
            }
            let errors = |o: &aws_smithy_schema::OperationSchema<'_>| {
                o.errors()
                    .iter()
                    .map(|e| fingerprint(e))
                    .collect::<Vec<_>>()
            };
            assert_eq!(errors(&actual), errors(&expected), "{name}");
        }
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} models");
}

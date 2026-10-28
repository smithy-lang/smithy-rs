/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Unit tests, and the Miri suite for the ownership component:
//!
//! ```text
//! cargo +nightly miri test -p aws-smithy-lang --features runtime-schema --lib runtime_schema::
//! ```
//!
//! CI runs this, with `-Zmiri-strict-provenance`, from the crate's `additional-ci` script.
//!
//! Models used under Miri disable the prelude, because parsing it is slow there; tests that
//! need it are ignored under Miri.

use super::owner::tests::{count_allocations, with_panic_after, Sentinel};
use super::{RuntimeSchemaSet, SchemaBuildError};
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
    let walked: Vec<_> = super::build::contained_operations(&model, service)
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

/// Visits every schema reachable from the set's public API.
fn for_each_schema<'a>(set: &'a RuntimeSchemaSet, mut f: impl FnMut(&'a Schema<'a>)) {
    let mut ids: Vec<ShapeId> = set.graph.schemas.keys().cloned().collect();
    ids.sort();
    for id in &ids {
        f(set.schema(id).unwrap());
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
        "RuntimeSchemaSet { service: None, roots: 3, schemas: 9, operations: 4, .. }"
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
            .graph
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

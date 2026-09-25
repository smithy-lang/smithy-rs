/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Integration tests for `#[derive(SmithySchema)]`.
//!
//! Run with `cargo test --features derive`. The `@error` shape support is
//! exercised end-to-end in `examples/pokemon-service` (`authz.rs`), which has
//! the `aws-smithy-http-server` dependency the generated error impls require.

#![cfg(feature = "derive")]

use std::collections::HashMap;

use aws_smithy_schema::document::DiscriminatedDocumentExt;
use aws_smithy_schema::{shape_id, ShapeType, SmithySchema, StringTrait};
use aws_smithy_types::{Blob, DateTime, DiscriminatedDocument, Document};

#[derive(Debug, SmithySchema)]
#[smithy(namespace = "smithy.example")]
struct Nested {
    name: String,
}

#[derive(Debug, SmithySchema)]
#[smithy(namespace = "smithy.example", shape_name = "Renamed")]
struct Everything {
    a_boolean: bool,
    an_integer: i32,
    a_long: i64,
    a_double: f64,
    a_string: String,
    a_blob: Blob,
    a_timestamp: DateTime,
    an_optional: Option<String>,
    #[smithy(rename = "wireName", json_name = "jsonName", sensitive)]
    renamed: String,
    a_list: Vec<String>,
    a_map: HashMap<String, String>,
    nested: Nested,
    nested_list: Vec<Nested>,
    #[smithy(skip)]
    #[allow(dead_code)]
    not_on_the_wire: std::time::Instant,
}

fn everything() -> Everything {
    Everything {
        a_boolean: true,
        an_integer: -5,
        a_long: 9_876_543_210,
        a_double: 2.5,
        a_string: "hello".into(),
        a_blob: Blob::new(vec![1, 2, 3]),
        a_timestamp: DateTime::from_secs(1_700_000_000),
        an_optional: None,
        renamed: "secret".into(),
        a_list: vec!["a".into(), "b".into()],
        a_map: HashMap::from([("k".to_string(), "v".to_string())]),
        nested: Nested { name: "inner".into() },
        nested_list: vec![Nested { name: "one".into() }, Nested { name: "two".into() }],
        not_on_the_wire: std::time::Instant::now(),
    }
}

fn members(doc: &DiscriminatedDocument) -> &aws_smithy_types::document::DocumentObject {
    match doc.document() {
        Document::Object(map) => map,
        other => panic!("expected object document, got {other:?}"),
    }
}

#[test]
fn schema_identity_and_members() {
    let schema = Everything::SCHEMA;
    assert_eq!(schema.shape_id().as_str(), "smithy.example#Renamed");

    // Skipped fields are absent; indices are dense over the emitted members.
    assert!(schema.member_schema("not_on_the_wire").is_none());
    let renamed = schema.member_schema("wireName").expect("renamed member");
    assert_eq!(renamed.member_index(), Some(8));
    assert_eq!(renamed.json_name().unwrap().value(), "jsonName");
    assert!(renamed.sensitive().is_some());

    let nested = schema.member_schema("nested").expect("nested member");
    assert_eq!(nested.shape_type(), ShapeType::Structure);
}

#[test]
fn serializes_every_supported_type() {
    let doc = DiscriminatedDocument::from_struct(Everything::SCHEMA, &everything()).unwrap();
    assert_eq!(doc.discriminator(), Some("smithy.example#Renamed"));

    let members = members(&doc);
    assert_eq!(members["a_boolean"], Document::from(true));
    assert_eq!(members["an_integer"], Document::from(-5));
    assert_eq!(members["a_string"], Document::from("hello"));
    // Optional members are omitted when `None`.
    assert!(!members.contains_key("an_optional"));
    // Renamed member serializes under its wire name.
    assert_eq!(members["wireName"], Document::from("secret"));
    assert!(!members.contains_key("renamed"));
    // Skipped member never appears.
    assert!(!members.contains_key("not_on_the_wire"));

    assert_eq!(
        members["a_list"],
        Document::Array(vec![Document::from("a"), Document::from("b")])
    );
    match &members["nested"] {
        Document::Object(nested) => assert_eq!(nested["name"], Document::from("inner")),
        other => panic!("expected nested object, got {other:?}"),
    }
    match &members["nested_list"] {
        Document::Array(items) => assert_eq!(items.len(), 2),
        other => panic!("expected array, got {other:?}"),
    }
    match &members["a_map"] {
        Document::Object(map) => assert_eq!(map["k"], Document::from("v")),
        other => panic!("expected map object, got {other:?}"),
    }
}

#[test]
fn optional_members_serialize_when_present() {
    let mut value = everything();
    value.an_optional = Some("present".into());
    let doc = DiscriminatedDocument::from_struct(Everything::SCHEMA, &value).unwrap();
    assert_eq!(members(&doc)["an_optional"], Document::from("present"));
}

#[derive(Debug, SmithySchema)]
#[smithy(
    namespace = "smithy.example",
    traits(StringTrait::new(shape_id!("smithy.example", "customShapeTrait"), "on-shape"))
)]
struct CustomTraits {
    #[smithy(traits(StringTrait::new(shape_id!("smithy.example", "customMemberTrait"), "on-member")))]
    field: String,
}

#[test]
fn arbitrary_traits_land_in_the_trait_maps() {
    let schema = CustomTraits::SCHEMA;
    let shape_trait = schema
        .traits()
        .and_then(|map| map.get_fqn("smithy.example#customShapeTrait"))
        .expect("shape-level custom trait");
    let shape_trait = shape_trait
        .as_any()
        .downcast_ref::<StringTrait>()
        .expect("StringTrait");
    assert_eq!(shape_trait.value(), "on-shape");

    let member = schema.member_schema("field").unwrap();
    let member_trait = member
        .traits()
        .and_then(|map| map.get_fqn("smithy.example#customMemberTrait"))
        .expect("member-level custom trait");
    let member_trait = member_trait
        .as_any()
        .downcast_ref::<StringTrait>()
        .expect("StringTrait");
    assert_eq!(member_trait.value(), "on-member");
}

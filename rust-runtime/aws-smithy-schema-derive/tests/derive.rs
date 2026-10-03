/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Integration tests for `#[derive(SmithySchema)]`.
//!
//! Run with `cargo test --quiet -p aws-smithy-schema-derive` from `rust-runtime`.
//! These tests expand the macros against the runtime crates directly; no SDK
//! generation or examples build is needed.

use std::collections::HashMap;

use aws_smithy_schema::document::DiscriminatedDocumentExt;
use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::{shape_id, ShapeType, StringTrait};
use aws_smithy_schema_derive::{smithy_namespace, SmithySchema};
use aws_smithy_types::{Blob, DateTime, DiscriminatedDocument, Document};

smithy_namespace! {
    "smithy.example";

    #[derive(Debug, SmithySchema)]
    struct Nested {
        name: String,
    }

    #[derive(Debug, SmithySchema)]
    #[smithy(shape_name = "Renamed")]
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
        nested: Nested {
            name: "inner".into(),
        },
        nested_list: vec![Nested { name: "one".into() }, Nested { name: "two".into() }],
        not_on_the_wire: std::time::Instant::now(),
    }
}

fn members(doc: &DiscriminatedDocument) -> &HashMap<String, Document> {
    match doc.document() {
        Document::Object(map) => map,
        other => panic!("expected object document, got {other:?}"),
    }
}

#[test]
fn schema_identity_and_members() {
    let schema = Everything::SCHEMA;
    let value = everything();
    let erased: &dyn SerializableStruct = &value;
    assert!(std::ptr::eq(erased.schema(), schema));
    let boxed: Box<dyn SerializableStruct> = Box::new(value);
    assert!(std::ptr::eq(boxed.schema(), schema));
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

smithy_namespace! {
    "smithy.section";

    #[derive(Debug, aws_smithy_schema_derive::SmithySchema)]
    pub struct Qualified;

    #[derive(Debug, SmithySchema)]
    #[smithy(shape_name = "Override")]
    #[smithy(namespace = "smithy.explicit")]
    struct Explicit;

    #[derive(Debug, SmithySchema)]
    #[smithy(traits(StringTrait::new(shape_id!("smithy.example", "customShapeTrait"), "wrapped")))]
    struct WrappedTraits;

    struct Ordinary;
    const UNRELATED: Ordinary = Ordinary;
    fn unrelated() -> Ordinary { UNRELATED }

    mod inner {
        #[derive(Debug, aws_smithy_schema_derive::SmithySchema)]
        #[smithy(namespace = "smithy.inner")]
        pub struct Inner;
    }
}

smithy_namespace! {
    "smithy.second";

    #[derive(Debug, SmithySchema)]
    struct Second;
}

#[test]
fn namespace_sections_preserve_overrides_and_other_items() {
    assert_eq!(Nested::SCHEMA.shape_id().as_str(), "smithy.example#Nested");
    assert_eq!(
        Qualified::SCHEMA.shape_id().as_str(),
        "smithy.section#Qualified"
    );
    assert_eq!(
        Explicit::SCHEMA.shape_id().as_str(),
        "smithy.explicit#Override"
    );
    assert_eq!(Second::SCHEMA.shape_id().as_str(), "smithy.second#Second");
    assert_eq!(
        inner::Inner::SCHEMA.shape_id().as_str(),
        "smithy.inner#Inner"
    );
    let _: Ordinary = unrelated();
    let shape_trait = WrappedTraits::SCHEMA
        .traits()
        .unwrap()
        .get_fqn("smithy.example#customShapeTrait")
        .unwrap();
    assert_eq!(
        shape_trait
            .as_any()
            .downcast_ref::<StringTrait>()
            .unwrap()
            .value(),
        "wrapped"
    );
}

// Neither this nested value nor the enclosing shape implements SerializableStruct.
// Metadata-only generation must not introduce serialization bounds or a dependency
// on aws-smithy-http-server, even for modeled errors.
#[derive(Debug)]
struct MetadataChild;

#[derive(Debug, SmithySchema)]
#[smithy(
    namespace = "smithy.metadata",
    target = "server",
    serialize = false,
    error = "client"
)]
struct MetadataOnly {
    #[allow(dead_code)]
    child: MetadataChild,
}

#[test]
fn metadata_only_preserves_schema_without_serialization_requirements() {
    assert_eq!(
        MetadataOnly::SCHEMA.shape_id().as_str(),
        "smithy.metadata#MetadataOnly"
    );
    assert_eq!(
        MetadataOnly::SCHEMA
            .member_schema("child")
            .unwrap()
            .shape_type(),
        ShapeType::Structure
    );
    assert!(MetadataOnly::SCHEMA
        .traits()
        .unwrap()
        .contains_fqn("smithy.api#error"));
}

smithy_namespace! {
    "smithy.client", target = "client", serialize = false;

    #[derive(Debug, SmithySchema)]
    #[smithy(error = "server", serialize = true)]
    struct ClientError {
        message: String,
    }

    #[derive(Debug, aws_smithy_schema_derive::SmithyError)]
    #[smithy(serialize = true)]
    enum ClientErrors {
        Failure(ClientError),
    }

    #[derive(Debug, SmithySchema)]
    #[smithy(target = "server", error = "client")]
    struct ServerMetadataOnly {
        #[allow(dead_code)]
        child: MetadataChild,
    }
}

#[derive(Debug, SmithySchema)]
#[smithy(namespace = "smithy.shared", error = "client")]
struct SharedError;

#[test]
fn client_and_shared_errors_serialize_without_server_dependencies() {
    let error = ClientErrors::Failure(ClientError {
        message: "failed".into(),
    });
    let doc = DiscriminatedDocument::from_struct(error.schema(), &error).unwrap();
    assert_eq!(members(&doc)["message"], Document::from("failed"));
    assert_eq!(
        error.schema().shape_id().as_str(),
        "smithy.client#ClientError"
    );
    let shared = SharedError;
    assert_eq!(
        shared.schema().shape_id().as_str(),
        "smithy.shared#SharedError"
    );
    assert!(ServerMetadataOnly::SCHEMA.member_schema("child").is_some());
}

#[derive(Debug)]
enum Label {
    Ready,
}

impl Label {
    fn as_str(&self) -> &str {
        "ready"
    }
}

smithy_namespace! {
    "smithy.collections";

    #[derive(Debug, SmithySchema)]
    enum Choice {
        #[smithy(rename = "text")]
        Text(String),
        #[smithy(rename = "record")]
        Record(Nested),
    }

    #[derive(Debug, SmithySchema)]
    struct Collections {
        sparse: Vec<Option<String>>,
        matrix: Vec<Vec<i32>>,
        records: HashMap<String, Option<Nested>>,
        mixed: Vec<HashMap<String, Option<Vec<bool>>>>,
        #[smithy(union)]
        choice: Box<Choice>,
        #[smithy(union)]
        choices: HashMap<String, Vec<Option<Choice>>>,
        #[smithy(string_enum)]
        labels: Vec<Option<Label>>,
        shorts: Vec<i16>,
        doubles: Vec<f64>,
        documents: Vec<Document>,
    }

    #[derive(Debug, SmithySchema)]
    #[smithy(serialize = false)]
    #[allow(dead_code)] // Metadata-only fixtures deliberately have no serialization impl.
    struct CollectionMetadata {
        values: HashMap<String, Vec<Option<MetadataChild>>>,
        #[smithy(union)]
        choice: MetadataChild,
    }
}

#[test]
fn nested_collection_metadata_describes_each_level() {
    let schema = Collections::SCHEMA;
    assert_eq!(
        schema
            .member_schema("labels")
            .unwrap()
            .member()
            .unwrap()
            .shape_type(),
        ShapeType::String
    );
    let sparse = schema.member_schema("sparse").unwrap();
    assert_eq!(sparse.shape_type(), ShapeType::List);
    assert_eq!(sparse.member().unwrap().shape_type(), ShapeType::String);
    let matrix = schema.member_schema("matrix").unwrap();
    assert_eq!(matrix.member().unwrap().shape_type(), ShapeType::List);
    assert_eq!(
        matrix.member().unwrap().member().unwrap().shape_type(),
        ShapeType::Integer
    );
    let records = schema.member_schema("records").unwrap();
    assert_eq!(records.key().unwrap().shape_type(), ShapeType::String);
    assert_eq!(records.member().unwrap().shape_type(), ShapeType::Structure);
    assert_eq!(
        schema.member_schema("choice").unwrap().shape_type(),
        ShapeType::Union
    );
    let choices = schema.member_schema("choices").unwrap();
    assert_eq!(choices.member().unwrap().shape_type(), ShapeType::List);
    assert_eq!(
        choices.member().unwrap().member().unwrap().shape_type(),
        ShapeType::Union
    );
    let metadata = CollectionMetadata::SCHEMA.member_schema("values").unwrap();
    assert_eq!(
        metadata.member().unwrap().member().unwrap().shape_type(),
        ShapeType::Structure
    );
    assert_eq!(
        CollectionMetadata::SCHEMA
            .member_schema("choice")
            .unwrap()
            .shape_type(),
        ShapeType::Union
    );
}

#[test]
fn nested_and_sparse_collections_preserve_values_and_nulls() {
    let value = Collections {
        sparse: vec![Some("first".into()), None, Some("last".into())],
        matrix: vec![vec![1, 2], vec![], vec![3]],
        records: HashMap::from([
            (
                "present".into(),
                Some(Nested {
                    name: "record".into(),
                }),
            ),
            ("absent".into(), None),
        ]),
        mixed: vec![HashMap::from([
            ("flags".into(), Some(vec![false, true])),
            ("null".into(), None),
            ("empty".into(), Some(vec![])),
        ])],
        choice: Box::new(Choice::Text("boxed".into())),
        choices: HashMap::from([(
            "items".into(),
            vec![
                None,
                Some(Choice::Record(Nested {
                    name: "union".into(),
                })),
            ],
        )]),
        labels: vec![Some(Label::Ready), None],
        shorts: vec![-1, 2],
        doubles: vec![1.5, -2.5],
        documents: vec![Document::Null, Document::from("document")],
    };
    let doc = DiscriminatedDocument::from_struct(Collections::SCHEMA, &value).unwrap();
    let fields = members(&doc);
    assert_eq!(
        fields["labels"],
        Document::Array(vec![Document::from("ready"), Document::Null])
    );
    assert_eq!(
        fields["sparse"],
        Document::Array(vec![
            Document::from("first"),
            Document::Null,
            Document::from("last")
        ])
    );
    assert_eq!(
        fields["matrix"],
        Document::Array(vec![
            Document::Array(vec![Document::from(1u64), Document::from(2u64)]),
            Document::Array(vec![]),
            Document::Array(vec![Document::from(3u64)])
        ])
    );
    let records = fields["records"].as_object().unwrap();
    assert_eq!(records["absent"], Document::Null);
    assert_eq!(
        records["present"].as_object().unwrap()["name"],
        Document::from("record")
    );
    let mixed = fields["mixed"].as_array().unwrap()[0].as_object().unwrap();
    assert_eq!(
        mixed["flags"],
        Document::Array(vec![Document::Bool(false), Document::Bool(true)])
    );
    assert_eq!(mixed["null"], Document::Null);
    assert_eq!(mixed["empty"], Document::Array(vec![]));
    assert_eq!(
        fields["choice"].as_object().unwrap()["text"],
        Document::from("boxed")
    );
    let choices = fields["choices"].as_object().unwrap()["items"]
        .as_array()
        .unwrap();
    assert_eq!(choices[0], Document::Null);
    assert_eq!(
        choices[1].as_object().unwrap()["record"]
            .as_object()
            .unwrap()["name"],
        Document::from("union")
    );
    assert_eq!(
        fields["shorts"],
        Document::Array(vec![Document::from(-1), Document::from(2u64)])
    );
    assert_eq!(
        fields["doubles"],
        Document::Array(vec![Document::from(1.5), Document::from(-2.5)])
    );
    assert_eq!(
        fields["documents"],
        Document::Array(vec![Document::Null, Document::from("document")])
    );
}

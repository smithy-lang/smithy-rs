/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The provided `write_document_v2`, `write_typed_document`, `read_document_v2`,
//! and `document_union_decode_mode` methods, seen from a third-party crate.
//!
//! The serializer and deserializer below implement exactly the methods that
//! `ShapeSerializer` and `ShapeDeserializer` required before `DocumentV2`
//! existed. That this file compiles is the compatibility check: adding the V2
//! methods must not break an existing implementation. The tests then pin down
//! what such an implementation does when it is handed a `DocumentV2` or a
//! typed document.

use aws_smithy_schema::prelude::DOCUMENT;
use aws_smithy_schema::registry::{RegistryEntry, TypeRegistry};
use aws_smithy_schema::serde::{
    DocumentUnionDecodeMode, SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_types::document::DocumentObject;
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{
    BigDecimal, BigInteger, Blob, DateTime, DiscriminatedDocument, Document, DocumentV2, Number,
};
use std::collections::HashMap;
use std::str::FromStr;

/// A serializer written against the pre-`DocumentV2` trait. It records the
/// documents passed to `write_document` and ignores everything else.
#[derive(Default)]
struct V1Serializer {
    documents: Vec<Document>,
}

impl ShapeSerializer for V1Serializer {
    fn write_struct(
        &mut self,
        _: &Schema<'_>,
        _: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_list(
        &mut self,
        _: &Schema<'_>,
        _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_map(
        &mut self,
        _: &Schema<'_>,
        _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_boolean(&mut self, _: &Schema<'_>, _: bool) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_byte(&mut self, _: &Schema<'_>, _: i8) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_short(&mut self, _: &Schema<'_>, _: i16) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_integer(&mut self, _: &Schema<'_>, _: i32) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_long(&mut self, _: &Schema<'_>, _: i64) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_float(&mut self, _: &Schema<'_>, _: f32) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_double(&mut self, _: &Schema<'_>, _: f64) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_big_integer(&mut self, _: &Schema<'_>, _: &BigInteger) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_big_decimal(&mut self, _: &Schema<'_>, _: &BigDecimal) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_string(&mut self, _: &Schema<'_>, _: &str) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_blob(&mut self, _: &Schema<'_>, _: Blob) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_timestamp(&mut self, _: &Schema<'_>, _: &DateTime) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_document(&mut self, _: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        self.documents.push(value.clone());
        Ok(())
    }
    fn write_null(&mut self, _: &Schema<'_>) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// A deserializer written against the pre-`DocumentV2` trait. `read_document`
/// returns `document`, or an error if there is none.
struct V1Deserializer {
    document: Option<Document>,
}

impl ShapeDeserializer for V1Deserializer {
    fn read_struct(
        &mut self,
        _: &Schema<'_>,
        _: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn read_list(
        &mut self,
        _: &Schema<'_>,
        _: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn read_map(
        &mut self,
        _: &Schema<'_>,
        _: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn read_boolean(&mut self, _: &Schema<'_>) -> Result<bool, SerdeError> {
        Ok(false)
    }
    fn read_byte(&mut self, _: &Schema<'_>) -> Result<i8, SerdeError> {
        Ok(0)
    }
    fn read_short(&mut self, _: &Schema<'_>) -> Result<i16, SerdeError> {
        Ok(0)
    }
    fn read_integer(&mut self, _: &Schema<'_>) -> Result<i32, SerdeError> {
        Ok(0)
    }
    fn read_long(&mut self, _: &Schema<'_>) -> Result<i64, SerdeError> {
        Ok(0)
    }
    fn read_float(&mut self, _: &Schema<'_>) -> Result<f32, SerdeError> {
        Ok(0.0)
    }
    fn read_double(&mut self, _: &Schema<'_>) -> Result<f64, SerdeError> {
        Ok(0.0)
    }
    fn read_big_integer(&mut self, _: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        Err(SerdeError::unsupported("big integer"))
    }
    fn read_big_decimal(&mut self, _: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        Err(SerdeError::unsupported("big decimal"))
    }
    fn read_string(&mut self, _: &Schema<'_>) -> Result<String, SerdeError> {
        Ok(String::new())
    }
    fn read_blob(&mut self, _: &Schema<'_>) -> Result<Blob, SerdeError> {
        Ok(Blob::new(Vec::new()))
    }
    fn read_timestamp(&mut self, _: &Schema<'_>) -> Result<DateTime, SerdeError> {
        Ok(DateTime::from_secs(0))
    }
    fn read_document(&mut self, _: &Schema<'_>) -> Result<Document, SerdeError> {
        self.document
            .take()
            .ok_or_else(|| SerdeError::invalid_input("no document left"))
    }
    fn is_null(&self) -> bool {
        false
    }
    fn container_size(&self) -> Option<usize> {
        None
    }
}

/// A `DocumentV2` that `Document` can represent, using every V1 variant.
fn representable() -> DocumentV2 {
    DocumentV2::Object(DocumentObject::from([
        ("name".to_owned(), DocumentV2::String("widget".to_owned())),
        ("count".to_owned(), DocumentV2::Number(Number::PosInt(3))),
        ("offset".to_owned(), DocumentV2::Number(Number::NegInt(-3))),
        ("ratio".to_owned(), DocumentV2::Number(Number::Float(0.5))),
        ("enabled".to_owned(), DocumentV2::Bool(true)),
        ("missing".to_owned(), DocumentV2::Null),
        (
            "tags".to_owned(),
            DocumentV2::Array(vec![DocumentV2::String("a".to_owned())]),
        ),
    ]))
}

fn representable_as_document() -> Document {
    Document::Object(HashMap::from([
        ("name".to_owned(), Document::String("widget".to_owned())),
        ("count".to_owned(), Document::Number(Number::PosInt(3))),
        ("offset".to_owned(), Document::Number(Number::NegInt(-3))),
        ("ratio".to_owned(), Document::Number(Number::Float(0.5))),
        ("enabled".to_owned(), Document::Bool(true)),
        ("missing".to_owned(), Document::Null),
        (
            "tags".to_owned(),
            Document::Array(vec![Document::String("a".to_owned())]),
        ),
    ]))
}

#[test]
fn default_write_passes_a_representable_value_to_write_document() {
    let mut serializer = V1Serializer::default();
    // Generated code calls through `&mut dyn ShapeSerializer`.
    let dynamic: &mut dyn ShapeSerializer = &mut serializer;
    dynamic
        .write_document_v2(&DOCUMENT, &representable())
        .expect("Document can represent this value");
    assert_eq!(serializer.documents, vec![representable_as_document()]);
}

#[test]
fn default_write_rejects_a_value_document_cannot_represent() {
    let cases = [
        (DocumentV2::Blob(b"bytes".to_vec()), "blob"),
        (DocumentV2::Timestamp(DateTime::from_secs(1)), "timestamp"),
        (
            DocumentV2::BigInteger(BigInteger::from_str("18446744073709551616").unwrap()),
            "bigInteger",
        ),
        (
            DocumentV2::BigDecimal(BigDecimal::from_str("0.1").unwrap()),
            "bigDecimal",
        ),
    ];
    for (leaf, variant) in cases {
        let nested = DocumentV2::Object(DocumentObject::from([(
            "items".to_owned(),
            DocumentV2::Array(vec![DocumentV2::Null, leaf]),
        )]));
        let mut serializer = V1Serializer::default();
        let dynamic: &mut dyn ShapeSerializer = &mut serializer;
        let err = dynamic
            .write_document_v2(&DOCUMENT, &nested)
            .expect_err("Document can't represent this value");
        assert!(
            matches!(err, SerdeError::UnsupportedOperation { .. }),
            "{variant}: {err:?}"
        );
        assert_eq!(
            err.to_string(),
            format!(
                "unsupported operation: this serializer writes DocumentV2 values through \
                 write_document, and Document can't represent the {variant} at $[\"items\"][1]"
            )
        );
        assert!(
            serializer.documents.is_empty(),
            "{variant}: nothing should reach write_document"
        );
    }
}

#[test]
fn default_read_upgrades_the_read_document_result() {
    let mut deserializer = V1Deserializer {
        document: Some(representable_as_document()),
    };
    let dynamic: &mut dyn ShapeDeserializer = &mut deserializer;
    let value = dynamic.read_document_v2(&DOCUMENT).unwrap();
    assert_eq!(value, DocumentV2::from(representable_as_document()));
    // The upgrade is lossless, so a downgrade gives back what `read_document` returned.
    assert_eq!(
        value.try_to_document().unwrap(),
        representable_as_document()
    );
}

#[test]
fn default_read_returns_the_read_document_error() {
    let mut deserializer = V1Deserializer { document: None };
    let dynamic: &mut dyn ShapeDeserializer = &mut deserializer;
    let err = dynamic.read_document_v2(&DOCUMENT).unwrap_err();
    assert_eq!(err.to_string(), "invalid input: no document left");
}

#[test]
fn an_existing_deserializer_reports_the_v1_union_decode_mode() {
    let mut deserializer = V1Deserializer { document: None };
    let dynamic: &mut dyn ShapeDeserializer = &mut deserializer;
    assert_eq!(
        dynamic.document_union_decode_mode(),
        DocumentUnionDecodeMode::V1
    );
    // Selecting a mode doesn't need the deserializer's cooperation.
    let mode = DocumentUnionDecodeMode::V2.read_with(dynamic, |d| d.document_union_decode_mode());
    assert_eq!(mode, DocumentUnionDecodeMode::V2);
}

/// A serializer that writes `DocumentV2` natively, by wrapping the V1 one and
/// overriding `write_document_v2`.
#[derive(Default)]
struct V2Serializer {
    inner: V1Serializer,
    documents_v2: Vec<DocumentV2>,
}

impl ShapeSerializer for V2Serializer {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        self.inner.write_struct(schema, value)
    }
    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.write_list(schema, write)
    }
    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.write_map(schema, write)
    }
    fn write_boolean(&mut self, schema: &Schema<'_>, value: bool) -> Result<(), SerdeError> {
        self.inner.write_boolean(schema, value)
    }
    fn write_byte(&mut self, schema: &Schema<'_>, value: i8) -> Result<(), SerdeError> {
        self.inner.write_byte(schema, value)
    }
    fn write_short(&mut self, schema: &Schema<'_>, value: i16) -> Result<(), SerdeError> {
        self.inner.write_short(schema, value)
    }
    fn write_integer(&mut self, schema: &Schema<'_>, value: i32) -> Result<(), SerdeError> {
        self.inner.write_integer(schema, value)
    }
    fn write_long(&mut self, schema: &Schema<'_>, value: i64) -> Result<(), SerdeError> {
        self.inner.write_long(schema, value)
    }
    fn write_float(&mut self, schema: &Schema<'_>, value: f32) -> Result<(), SerdeError> {
        self.inner.write_float(schema, value)
    }
    fn write_double(&mut self, schema: &Schema<'_>, value: f64) -> Result<(), SerdeError> {
        self.inner.write_double(schema, value)
    }
    fn write_big_integer(
        &mut self,
        schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        self.inner.write_big_integer(schema, value)
    }
    fn write_big_decimal(
        &mut self,
        schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        self.inner.write_big_decimal(schema, value)
    }
    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        self.inner.write_string(schema, value)
    }
    fn write_blob(&mut self, schema: &Schema<'_>, value: Blob) -> Result<(), SerdeError> {
        self.inner.write_blob(schema, value)
    }
    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        self.inner.write_timestamp(schema, value)
    }
    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        self.inner.write_document(schema, value)
    }
    fn write_document_v2(&mut self, _: &Schema<'_>, value: &DocumentV2) -> Result<(), SerdeError> {
        self.documents_v2.push(value.clone());
        Ok(())
    }
    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        self.inner.write_null(schema)
    }
}

#[test]
fn an_override_is_reached_through_a_trait_object() {
    let blob = DocumentV2::Blob(b"bytes".to_vec());
    let mut serializer = V2Serializer::default();
    let dynamic: &mut dyn ShapeSerializer = &mut serializer;
    dynamic.write_document_v2(&DOCUMENT, &blob).unwrap();
    assert_eq!(serializer.documents_v2, vec![blob]);
    assert!(serializer.inner.documents.is_empty());
}

static EMPTY: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.example", "Empty"),
    ShapeType::Structure,
    &[],
);

#[derive(Debug)]
struct Empty;

impl SerializableStruct for Empty {
    fn schema(&self) -> &Schema<'_> {
        &EMPTY
    }

    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

#[test]
fn default_typed_document_write_is_unsupported() {
    let expected = "unsupported operation: this serializer doesn't write typed documents";
    let mut serializer = V1Serializer::default();
    let dynamic: &mut dyn ShapeSerializer = &mut serializer;
    let err = dynamic.write_typed_document(&DOCUMENT, &Empty).unwrap_err();
    assert!(
        matches!(err, SerdeError::UnsupportedOperation { .. }),
        "{err:?}"
    );
    assert_eq!(err.to_string(), expected);

    // The registry passes the error on.
    let registry = TypeRegistry::builder()
        .insert(
            EMPTY.shape_id().clone(),
            RegistryEntry::new(&EMPTY, |_| Ok(TypeErasedBox::new(Empty)))
                .with_serializable::<Empty>(),
        )
        .build();
    let document = DiscriminatedDocument::new_v2(DocumentV2::Object(DocumentObject::new()))
        .try_with_discriminator("smithy.example#Empty")
        .unwrap();
    let err = registry
        .serialize_document(&mut serializer, &DOCUMENT, &document)
        .unwrap_err();
    assert_eq!(err.to_string(), expected);
    assert!(serializer.documents.is_empty());
}

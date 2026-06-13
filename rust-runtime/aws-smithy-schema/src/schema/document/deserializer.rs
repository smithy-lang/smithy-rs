/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentShapeDeserializer`] — a [`ShapeDeserializer`] implementation
//! that walks a [`Document`] tree.
//!
//! Generated `deserialize` methods on Smithy shapes call the
//! [`ShapeDeserializer`] interface to drive structure / list / map
//! consumer dispatch and read scalar leaves. Pointing such generated
//! code at this deserializer reifies a [`Document`] tree as the
//! corresponding typed shape — the inverse of [`Document::from_struct`].
//!
//! The deserializer holds a borrow of the source [`Document`] so reads
//! are zero-copy where possible (e.g. [`String`] reads still clone the
//! payload, but list/map navigation does not allocate).
//!
//! # Member-name resolution
//!
//! For struct reads, member dispatch uses the document's keys (not the
//! schema's member list) so that documents created from a typed shape
//! by [`DocumentShapeSerializer`] round-trip — those use Smithy member
//! names. Documents parsed from a wire format that renames members
//! (`@jsonName`, `@xmlName`) carry an [`Arc<dyn DocumentSettings>`] that
//! resolves wire names back to schema member indices; see
//! [`DocumentSettings::member_index_for`].
//!
//! Members present in the schema but absent from the document are not
//! reported: generated builders default unset optional members to `None`
//! and required-member enforcement is the builder's responsibility, not
//! the deserializer's.

use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime};

use super::{Document, DocumentInner, DocumentSettings};
use crate::serde::{capped_container_size, SerdeError, ShapeDeserializer};
use crate::Schema;

/// Walks a [`Document`] tree via the [`ShapeDeserializer`] interface.
///
/// See the module-level documentation for an overview.
///
/// # Example
///
/// Use [`Document::as_shape`] for the common case of consuming a
/// `Document` via a generated `deserialize` method:
///
/// ```ignore
/// use aws_smithy_schema::document::Document;
///
/// let person: Person = doc.as_shape(|deser| Person::deserialize(deser))?;
/// ```
///
/// Direct construction is useful when consuming a sub-document outside
/// the standard entry point:
///
/// ```ignore
/// use aws_smithy_schema::document::DocumentShapeDeserializer;
/// use aws_smithy_schema::serde::ShapeDeserializer;
///
/// let mut deser = DocumentShapeDeserializer::new(&doc);
/// let s = deser.read_string(&aws_smithy_schema::prelude::STRING)?;
/// ```
#[derive(Debug)]
pub struct DocumentShapeDeserializer<'a> {
    /// The document this deserializer is currently positioned at.
    cursor: &'a Document<'a>,
}

impl<'a> DocumentShapeDeserializer<'a> {
    /// Creates a deserializer positioned at the given document.
    pub fn new(document: &'a Document<'a>) -> Self {
        Self { cursor: document }
    }
}

/// Builds a `TypeMismatch` error message for a read that expected one
/// kind of value and found another.
fn type_mismatch(expected: &str, found: &DocumentInner<'_>) -> SerdeError {
    SerdeError::TypeMismatch {
        message: format!("expected {expected} document, got {}", kind_name(found),),
    }
}

fn kind_name(inner: &DocumentInner<'_>) -> &'static str {
    match inner {
        DocumentInner::Null => "null",
        DocumentInner::Boolean(_) => "boolean",
        DocumentInner::Number(_) => "number",
        DocumentInner::BigInteger(_) => "bigInteger",
        DocumentInner::BigDecimal(_) => "bigDecimal",
        DocumentInner::String(_) => "string",
        DocumentInner::Blob(_) => "blob",
        DocumentInner::Timestamp(_) => "timestamp",
        DocumentInner::List(_) => "list",
        DocumentInner::Map(_) => "map",
    }
}

/// Resolves a wire-level map key to a member of `schema`. If the
/// document has settings attached, those are consulted so that
/// `@jsonName`-style member rename traits resolve correctly. Otherwise
/// falls back to matching against [`Schema::member_name`] directly.
fn resolve_member<'s>(
    schema: &'s Schema<'s>,
    settings: Option<&dyn DocumentSettings>,
    wire_name: &str,
) -> Option<&'s Schema<'s>> {
    let idx = match settings {
        Some(s) => s.member_index_for(schema, wire_name)?,
        None => schema
            .members()
            .iter()
            .position(|m| m.member_name() == Some(wire_name))?,
    };
    schema.member_schema_by_index(idx)
}

impl<'a> ShapeDeserializer for DocumentShapeDeserializer<'a> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let map = self
            .cursor
            .as_map()
            .ok_or_else(|| type_mismatch("struct (map)", self.cursor.inner()))?;
        // Inherit settings from the parent so wire-renamed members
        // resolve correctly; this is `None` for serialize-side documents.
        let settings = self
            .cursor
            .settings()
            .map(|arc| arc.as_ref() as &dyn DocumentSettings);
        for (key, value) in map {
            let Some(member_schema) = resolve_member(schema, settings, key) else {
                // Unknown member — silently ignore. Matches the
                // tolerant "ignore unknown fields" behavior of the
                // JSON deserializer.
                continue;
            };
            let mut sub = Self::new(value);
            consumer(member_schema, &mut sub)?;
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let items = self
            .cursor
            .as_list()
            .ok_or_else(|| type_mismatch("list", self.cursor.inner()))?;
        for item in items {
            let mut sub = Self::new(item);
            consumer(&mut sub)?;
        }
        Ok(())
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let entries = self
            .cursor
            .as_map()
            .ok_or_else(|| type_mismatch("map", self.cursor.inner()))?;
        for (key, value) in entries {
            let mut sub = Self::new(value);
            consumer(key.clone(), &mut sub)?;
        }
        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        self.cursor
            .as_boolean()
            .ok_or_else(|| type_mismatch("boolean", self.cursor.inner()))
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        self.cursor.as_byte()
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        self.cursor.as_short()
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        self.cursor.as_integer()
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        self.cursor.as_long()
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        self.cursor.as_float()
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        self.cursor.as_double()
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        self.cursor.as_big_integer()
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        self.cursor.as_big_decimal()
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        match self.cursor.inner() {
            DocumentInner::String(s) => Ok(s.clone()),
            other => Err(type_mismatch("string", other)),
        }
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        // `Document::as_blob` returns the variant directly for native
        // `Blob` documents and consults `DocumentSettings` to coerce
        // strings (e.g. JSON's base64-encoded blobs).
        let bytes = self.cursor.as_blob()?;
        Ok(Blob::new(bytes.into_owned()))
    }

    fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        self.cursor.as_timestamp()
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document<'_>, SerdeError> {
        Ok(self.cursor.clone())
    }

    fn is_null(&self) -> bool {
        matches!(self.cursor.inner(), DocumentInner::Null)
    }

    fn container_size(&self) -> Option<usize> {
        let raw = match self.cursor.inner() {
            DocumentInner::List(items) => items.len(),
            DocumentInner::Map(entries) => entries.len(),
            _ => return None,
        };
        Some(capped_container_size(raw))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::document::DocumentShapeSerializer;
    use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
    use crate::{prelude, shape_id, Schema, ShapeId, ShapeType};

    // -- Test schemas ----------------------------------------------------

    const PERSON_ID: ShapeId<'static> = shape_id!("smithy.example", "Person");
    const PERSON_NAME_ID: ShapeId<'static> = shape_id!("smithy.example", "Person", "name");
    const PERSON_AGE_ID: ShapeId<'static> = shape_id!("smithy.example", "Person", "age");

    static PERSON_NAME_MEMBER: Schema<'static> =
        Schema::new_member(PERSON_NAME_ID, ShapeType::String, "name", 0);
    static PERSON_AGE_MEMBER: Schema<'static> =
        Schema::new_member(PERSON_AGE_ID, ShapeType::Integer, "age", 1);
    static PERSON_SCHEMA: Schema<'static> = Schema::new_struct(
        PERSON_ID,
        ShapeType::Structure,
        &[&PERSON_NAME_MEMBER, &PERSON_AGE_MEMBER],
    );

    /// Test struct + builder pair to drive `read_struct` consumer
    /// dispatch the same way generated code does.
    #[derive(Debug, Default, PartialEq)]
    struct Person {
        name: Option<String>,
        age: Option<i32>,
    }

    impl SerializableStruct for Person {
        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(n) = &self.name {
                ser.write_string(&PERSON_NAME_MEMBER, n)?;
            }
            if let Some(a) = self.age {
                ser.write_integer(&PERSON_AGE_MEMBER, a)?;
            }
            Ok(())
        }
    }

    fn deserialize_person(deser: &mut dyn ShapeDeserializer) -> Result<Person, SerdeError> {
        let mut out = Person::default();
        deser.read_struct(&PERSON_SCHEMA, &mut |member, sub| {
            match member.member_index() {
                Some(0) => out.name = Some(sub.read_string(member)?),
                Some(1) => out.age = Some(sub.read_integer(member)?),
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }

    // -- Scalars ---------------------------------------------------------

    #[test]
    fn read_string_returns_value() {
        let doc = Document::string("hello");
        let mut deser = DocumentShapeDeserializer::new(&doc);
        assert_eq!(deser.read_string(&prelude::STRING).unwrap(), "hello");
    }

    #[test]
    fn read_string_on_non_string_errors() {
        let doc = Document::integer(1);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let err = deser.read_string(&prelude::STRING).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    #[test]
    fn read_integer_with_coercion() {
        // Integer narrowing follows SEP rules (precision loss ignored).
        let doc = Document::long(42);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        assert_eq!(deser.read_integer(&prelude::INTEGER).unwrap(), 42);
    }

    #[test]
    fn read_integer_overflow_errors() {
        let doc = Document::long(i64::MAX);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let err = deser.read_integer(&prelude::INTEGER).unwrap_err();
        assert!(matches!(err, SerdeError::NumericCoercionOverflow { .. }));
    }

    #[test]
    fn read_boolean_returns_value() {
        let doc = Document::boolean(true);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        assert!(deser.read_boolean(&prelude::BOOLEAN).unwrap());
    }

    #[test]
    fn read_blob_returns_value_for_native_blob() {
        let doc = Document::blob(vec![1, 2, 3]);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let blob = deser.read_blob(&prelude::BLOB).unwrap();
        assert_eq!(blob.as_ref(), &[1u8, 2, 3]);
    }

    #[test]
    fn is_null_for_null_document() {
        let doc = Document::null();
        let deser = DocumentShapeDeserializer::new(&doc);
        assert!(deser.is_null());
    }

    #[test]
    fn is_null_false_for_non_null() {
        let doc = Document::string("not-null");
        let deser = DocumentShapeDeserializer::new(&doc);
        assert!(!deser.is_null());
    }

    // -- Aggregates ------------------------------------------------------

    #[test]
    fn read_list_iterates_elements() {
        let doc = Document::list(vec![
            Document::string("a"),
            Document::string("b"),
            Document::string("c"),
        ]);
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let mut collected = Vec::new();
        deser
            .read_list(&prelude::DOCUMENT, &mut |sub| {
                collected.push(sub.read_string(&prelude::STRING)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(collected, ["a", "b", "c"]);
    }

    #[test]
    fn read_map_iterates_entries() {
        let doc = Document::map(HashMap::from([
            ("k1".to_string(), Document::string("v1")),
            ("k2".to_string(), Document::string("v2")),
        ]));
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let mut collected = HashMap::new();
        deser
            .read_map(&prelude::DOCUMENT, &mut |key, sub| {
                collected.insert(key, sub.read_string(&prelude::STRING)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(collected.get("k1").map(String::as_str), Some("v1"));
        assert_eq!(collected.get("k2").map(String::as_str), Some("v2"));
    }

    #[test]
    fn container_size_on_list() {
        let doc = Document::list(vec![Document::null(); 5]);
        let deser = DocumentShapeDeserializer::new(&doc);
        assert_eq!(deser.container_size(), Some(5));
    }

    #[test]
    fn container_size_on_scalar_is_none() {
        let doc = Document::string("foo");
        let deser = DocumentShapeDeserializer::new(&doc);
        assert!(deser.container_size().is_none());
    }

    // -- Struct round-trip ----------------------------------------------

    #[test]
    fn read_struct_with_consumer_dispatch() {
        let doc = Document::map(HashMap::from([
            ("name".to_string(), Document::string("Alex")),
            ("age".to_string(), Document::integer(30)),
        ]));
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let person = deserialize_person(&mut deser).unwrap();
        assert_eq!(
            person,
            Person {
                name: Some("Alex".into()),
                age: Some(30),
            }
        );
    }

    #[test]
    fn read_struct_with_missing_optional_member() {
        // Document only has `name`; `age` is missing.
        let doc = Document::map(HashMap::from([(
            "name".to_string(),
            Document::string("Sam"),
        )]));
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let person = deserialize_person(&mut deser).unwrap();
        assert_eq!(
            person,
            Person {
                name: Some("Sam".into()),
                age: None,
            }
        );
    }

    #[test]
    fn read_struct_ignores_unknown_members() {
        let doc = Document::map(HashMap::from([
            ("name".to_string(), Document::string("Joe")),
            ("unknown_field".to_string(), Document::string("ignored")),
        ]));
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let person = deserialize_person(&mut deser).unwrap();
        assert_eq!(person.name.as_deref(), Some("Joe"));
    }

    #[test]
    fn read_struct_on_non_map_errors() {
        let doc = Document::string("not-a-struct");
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let err = deserialize_person(&mut deser).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    // -- Round-trip with the serializer ---------------------------------

    #[test]
    // TODO(schema-lifetime): re-enable when Document gains a lifetime
    // parameter — depends on discriminator capture in write_struct.
    #[ignore]
    fn round_trip_through_document() {
        let original = Person {
            name: Some("Iago".into()),
            age: Some(7),
        };
        // serialize → Document
        let mut ser = DocumentShapeSerializer::new();
        ser.write_struct(&PERSON_SCHEMA, &original).unwrap();
        let doc = ser.finish().unwrap();
        // deserialize Document → typed
        let mut deser = DocumentShapeDeserializer::new(&doc);
        let restored = deserialize_person(&mut deser).unwrap();
        assert_eq!(restored, original);
        // discriminator is preserved through the serialize side
        assert_eq!(
            doc.discriminator().map(|id| id.as_str()),
            Some("smithy.example#Person")
        );
    }
}

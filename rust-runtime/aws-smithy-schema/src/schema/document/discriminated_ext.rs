/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Extension methods on [`DiscriminatedDocument`] that need access to
//! [`Schema`], [`SerializableStruct`], and the schema-side
//! [`ShapeSerializer`] / [`ShapeDeserializer`] machinery — types that
//! live in `aws-smithy-schema` and therefore cannot be referenced from
//! the inherent-impl in `aws-smithy-types` where
//! [`DiscriminatedDocument`] is defined.
//!
//! Without this trait the SEP-mandated `Document.of(struct)` and
//! `Document.asShape(deserialize)` entry points would not be reachable
//! on `DiscriminatedDocument` because the dependency direction
//! (`aws-smithy-schema → aws-smithy-types`) and Rust's orphan rule
//! together prevent adding inherent methods to `DiscriminatedDocument`
//! from this crate.
//!
//! Users import this trait to use the methods:
//!
//! ```ignore
//! use aws_smithy_schema::document::DiscriminatedDocumentExt;
//!
//! let doc = DiscriminatedDocument::from_struct(MyShape::SCHEMA, &my_value)?;
//! let restored: MyShape = doc.as_shape(|deser| MyShape::deserialize(deser))?;
//! ```
//!
//! # Transitional limitations
//!
//! The implementation routes through the existing schema-side
//! [`Document<'a>`](super::Document) +
//! [`DocumentShapeSerializer`](super::DocumentShapeSerializer) /
//! [`DocumentShapeDeserializer`](super::DocumentShapeDeserializer)
//! pipeline, then bridges to/from
//! [`aws_smithy_types::Document`] using the existing
//! [`From`] / [`TryFrom`] conversions. Both bridges have known
//! limitations after Phase 1 of the document-type unification work:
//!
//! - The `TryFrom<Document<'_>> for aws_smithy_types::Document`
//!   direction returns
//!   `Err(SerdeError::TypeMismatch { .. })` for the four new variants
//!   (`Blob`, `Timestamp`, `BigInteger`, `BigDecimal`). After Phase 1
//!   added those variants to `aws_smithy_types::Document`, the
//!   pre-existing error arms became technically incorrect — the
//!   variants ARE representable now, but the bridge predates that
//!   change. Consequently, [`DiscriminatedDocument::from_struct`] on
//!   a struct whose schema includes a member of any of those new
//!   variants returns the same `TypeMismatch` error.
//!
//! - The `From<aws_smithy_types::Document> for Document<'static>`
//!   direction has an Option-B transitional `panic!` arm covering the
//!   four new variants (see the bridge's `TODO(document-unification)`
//!   comment in `data.rs`). [`DiscriminatedDocument::as_shape`]
//!   inherits that panic when called on a document whose tree contains
//!   any of those variants.
//!
//! Both limitations resolve in Phase 4 of the unification work, when
//! the schema-side `Document<'a>` type and its bridges are removed and
//! [`DocumentShapeSerializer`] / [`DocumentShapeDeserializer`] are
//! retargeted at `aws_smithy_types::Document` directly. Until then,
//! [`DiscriminatedDocument::from_struct`] and
//! [`DiscriminatedDocument::as_shape`] are functional only for
//! documents whose every node is one of the legacy six variants
//! (`Null`, `Bool`, `Number`, `String`, `Array`, `Object`).

use aws_smithy_types::DiscriminatedDocument;

use super::{Document, DocumentShapeDeserializer, DocumentShapeSerializer};
use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use crate::{Schema, ShapeType};

/// Schema-aware operations on [`DiscriminatedDocument`].
///
/// Implemented for [`DiscriminatedDocument`] in `aws-smithy-schema`
/// because the methods need [`Schema`], [`SerializableStruct`], and
/// the [`ShapeDeserializer`] trait — types that live in this crate.
/// See the [module-level docs](self) for the rationale.
pub trait DiscriminatedDocumentExt {
    /// Constructs a [`DiscriminatedDocument`] from any
    /// [`SerializableStruct`] by driving it through the
    /// [`DocumentShapeSerializer`] pipeline.
    ///
    /// This is the SEP's `Document.of(struct)` entry point. The
    /// resulting wrapper carries `schema.shape_id().as_str()` as its
    /// discriminator, so a downstream type registry can find the right
    /// schema for the reverse conversion.
    ///
    /// Settings are not attached — serialize-side documents have no
    /// protocol context (per plan §2.4).
    ///
    /// # Errors
    ///
    /// Returns [`SerdeError::TypeMismatch`] if the struct contains any
    /// member of the new Smithy variants
    /// (`Blob` / `Timestamp` / `BigInteger` / `BigDecimal`) — see the
    /// "Transitional limitations" section of [the module docs](self).
    /// Resolved in Phase 4 of the document-type unification work.
    fn from_struct(
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<DiscriminatedDocument, SerdeError>;

    /// Reifies this [`DiscriminatedDocument`] as a typed shape by
    /// driving the given `deserialize` callback through a
    /// [`DocumentShapeDeserializer`].
    ///
    /// This is the SEP's `Document.asShape(...)` entry point. The
    /// callback is typically the generated `<Type>::deserialize`
    /// function on a shape's data carrier or builder.
    ///
    /// # Panics
    ///
    /// Panics if `self.document` contains any of the new Smithy
    /// variants (`Blob` / `Timestamp` / `BigInteger` / `BigDecimal`),
    /// because the underlying [`From<aws_smithy_types::Document>`]
    /// bridge has an Option-B transitional `panic!` arm for those
    /// variants. See the "Transitional limitations" section of
    /// [the module docs](self). Resolved in Phase 4.
    fn as_shape<T, F>(&self, deserialize: F) -> Result<T, SerdeError>
    where
        F: FnOnce(&mut dyn ShapeDeserializer) -> Result<T, SerdeError>;

    /// Returns the [`ShapeType`] this document would be reported as if
    /// converted to a typed shape.
    ///
    /// Mirrors [`Document::shape_type`](super::Document::shape_type).
    /// For numeric values, follows the SEP "Reporting `Document`
    /// ambiguous shape types" guidance — picks the narrowest
    /// unambiguous container from the precedence
    /// `Integer → Long → BigInteger → Double → BigDecimal`. `Byte`,
    /// `IntEnum`, `Short`, and `Float` are skipped.
    ///
    /// `Null` is reported as [`ShapeType::Document`] since `Null`
    /// itself is not a Smithy type variant.
    fn shape_type(&self) -> ShapeType;
}

impl DiscriminatedDocumentExt for DiscriminatedDocument {
    fn from_struct(
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<DiscriminatedDocument, SerdeError> {
        // Drive the schema-side serializer pipeline. The inherent
        // `DocumentShapeSerializer<'_>::write_struct` captures the
        // schema's shape ID into the resulting document's
        // discriminator slot.
        let mut ser = DocumentShapeSerializer::default();
        ser.write_struct(schema, value)?;
        let schema_doc: Document<'_> = ser.finish()?;

        // Capture the discriminator BEFORE the move into try_into:
        // schema_doc.discriminator() is a `&ShapeId<'_>`, whose
        // `.as_str()` is borrowed from schema-side storage; we want
        // an owned `String` for the unified DiscriminatedDocument.
        let discriminator = schema_doc.discriminator().map(|id| id.as_str().to_string());

        // Convert schema-side → legacy via the existing TryFrom bridge.
        // For the four new Smithy variants, this currently returns
        // SerdeError::TypeMismatch — see the "Transitional limitations"
        // section of the module docs.
        let unified: aws_smithy_types::Document = schema_doc.try_into()?;

        let mut wrapper = DiscriminatedDocument::new(unified);
        if let Some(d) = discriminator {
            wrapper = wrapper.with_discriminator(d);
        }
        Ok(wrapper)
    }

    fn as_shape<T, F>(&self, deserialize: F) -> Result<T, SerdeError>
    where
        F: FnOnce(&mut dyn ShapeDeserializer) -> Result<T, SerdeError>,
    {
        // Convert legacy → schema-side via the existing From bridge,
        // then drive the schema-side deserializer.
        //
        // The bridge has an Option-B transitional `panic!` for the
        // four new variants (see the bridge's `TODO(document-unification)`
        // comment in `data.rs`). We do NOT pre-check here to convert
        // that panic to a typed Err because:
        //   1. A pre-check is a tree walk — same complexity as the
        //      4-line bridge fix we explicitly chose to defer.
        //   2. The pre-check is itself code that gets deleted in
        //      Phase 4, so it doesn't reduce the total deletable
        //      surface; it just shifts where the deletion happens.
        //   3. The bridge's panic message is loud and self-explanatory
        //      to anyone hitting it during the transition window.
        //
        // We DO clone before converting because the schema-side
        // From bridge is by-value. The clone is unavoidable until the
        // schema-side type goes away; Phase 4's deserializer will read
        // directly from `self.document` without conversion.
        let schema_doc: Document<'static> = self.document.clone().into();
        let mut deser = DocumentShapeDeserializer::new(&schema_doc);
        deserialize(&mut deser)
    }

    fn shape_type(&self) -> ShapeType {
        use aws_smithy_types::Document as Legacy;
        // Inline the variant→ShapeType mapping rather than delegate to
        // `Document::shape_type()` via the bridge, because the bridge
        // is by-value (would require a full clone of the document tree
        // just to read the top-level variant). The number-disambiguation
        // logic delegates to the same private helper used by the
        // schema-side `Document::shape_type`.
        match &self.document {
            Legacy::Null => ShapeType::Document,
            Legacy::Bool(_) => ShapeType::Boolean,
            Legacy::Number(n) => super::data::number_shape_type(n),
            Legacy::Blob(_) => ShapeType::Blob,
            Legacy::Timestamp(_) => ShapeType::Timestamp,
            Legacy::BigInteger(_) => ShapeType::BigInteger,
            Legacy::BigDecimal(_) => ShapeType::BigDecimal,
            Legacy::String(_) => ShapeType::String,
            Legacy::Array(_) => ShapeType::List,
            Legacy::Object(_) => ShapeType::Map,
            // Future variants on the `#[non_exhaustive]` legacy enum
            // fall through to a generic Document type.
            _ => ShapeType::Document,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for [`DiscriminatedDocumentExt`].
    //!
    //! Coverage:
    //! - Round-trip a struct through the legacy 6-variant subset
    //!   (`Person { name: String, age: i32 }`).
    //! - Discriminator capture on `from_struct`.
    //! - `as_shape` reverses the round-trip.
    //! - Shape-type reporting for the new variants (Blob/Timestamp/
    //!   BigInteger/BigDecimal) — these don't exercise the bridges and
    //!   so don't hit the transitional limitations.
    //! - Documented limitation: a struct with a Blob member triggers
    //!   the TryFrom bridge regression and `from_struct` errors.
    //! - Documented limitation: `as_shape` on a top-level Blob document
    //!   panics via the From bridge's Option-B arm.
    //!
    //! All tests use `Schema<'static>` because the schema-crate
    //! prelude and codegen-emitted schemas are `'static`. The trait
    //! method signatures accept any lifetime via `&Schema<'_>`.
    use aws_smithy_types::DateTime;

    use super::*;
    use crate::serde::{SerdeError, ShapeSerializer};
    use crate::{prelude, shape_id, Schema, ShapeId, ShapeType};

    // -- Test schemas + types ------------------------------------------

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

    // -- from_struct happy path ----------------------------------------

    #[test]
    fn from_struct_attaches_fqn_discriminator() {
        let p = Person {
            name: Some("Iago".into()),
            age: Some(7),
        };
        let doc = DiscriminatedDocument::from_struct(&PERSON_SCHEMA, &p).unwrap();
        assert_eq!(doc.discriminator(), Some("smithy.example#Person"));
        assert!(doc.settings().is_none());
        // The inner document is a map with the two members.
        let map = doc.document().as_object().unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("name"));
        assert!(map.contains_key("age"));
    }

    #[test]
    fn from_struct_with_only_optional_members_set() {
        let p = Person {
            name: Some("Sam".into()),
            age: None,
        };
        let doc = DiscriminatedDocument::from_struct(&PERSON_SCHEMA, &p).unwrap();
        let map = doc.document().as_object().unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("name"));
    }

    // -- as_shape happy path -------------------------------------------

    #[test]
    fn as_shape_reverses_from_struct() {
        let original = Person {
            name: Some("Alex".into()),
            age: Some(30),
        };
        let doc = DiscriminatedDocument::from_struct(&PERSON_SCHEMA, &original).unwrap();
        let restored: Person = doc.as_shape(|deser| deserialize_person(deser)).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn as_shape_works_on_legacy_six_variant_document() {
        // Construct a DiscriminatedDocument directly from a legacy
        // map (no schema-side intermediate). Confirms as_shape works
        // on documents that didn't come from `from_struct`.
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(
            "name".to_string(),
            aws_smithy_types::Document::String("Joe".into()),
        );
        map.insert(
            "age".to_string(),
            aws_smithy_types::Document::Number(aws_smithy_types::Number::PosInt(42)),
        );
        let doc = DiscriminatedDocument::new(aws_smithy_types::Document::Object(map));
        let person: Person = doc.as_shape(|deser| deserialize_person(deser)).unwrap();
        assert_eq!(
            person,
            Person {
                name: Some("Joe".into()),
                age: Some(42),
            }
        );
    }

    // -- shape_type reporting ------------------------------------------

    #[test]
    fn shape_type_reports_each_legacy_variant() {
        use aws_smithy_types::{Document as Legacy, Number};
        let cases = [
            (Legacy::Null, ShapeType::Document),
            (Legacy::Bool(true), ShapeType::Boolean),
            (Legacy::String("x".into()), ShapeType::String),
            (Legacy::Number(Number::PosInt(0)), ShapeType::Integer),
            (
                Legacy::Number(Number::PosInt(u64::MAX)),
                ShapeType::BigInteger,
            ),
            (Legacy::Array(vec![]), ShapeType::List),
            (Legacy::Object(Default::default()), ShapeType::Map),
        ];
        for (doc, expected) in cases {
            let wrapped = DiscriminatedDocument::new(doc);
            assert_eq!(wrapped.shape_type(), expected);
        }
    }

    #[test]
    fn shape_type_reports_each_new_variant() {
        // The new variants don't go through any bridge for shape_type
        // — the impl matches them directly.
        use aws_smithy_types::{BigDecimal, BigInteger, Document as Legacy};
        use std::str::FromStr;
        let cases: [(Legacy, ShapeType); 4] = [
            (Legacy::Blob(vec![1, 2, 3]), ShapeType::Blob),
            (
                Legacy::Timestamp(DateTime::from_secs(0)),
                ShapeType::Timestamp,
            ),
            (
                Legacy::BigInteger(BigInteger::from_str("1").unwrap()),
                ShapeType::BigInteger,
            ),
            (
                Legacy::BigDecimal(BigDecimal::from_str("1.0").unwrap()),
                ShapeType::BigDecimal,
            ),
        ];
        for (doc, expected) in cases {
            let wrapped = DiscriminatedDocument::new(doc);
            assert_eq!(wrapped.shape_type(), expected);
        }
    }

    // -- Documented transitional limitations ---------------------------

    #[test]
    fn from_struct_with_blob_member_returns_typemismatch_during_phase3() {
        // A struct whose schema includes a blob member exercises
        // `write_blob` on the schema-side serializer, producing a
        // `Document::Blob`. The TryFrom bridge then errors.

        const BLOBBY_ID: ShapeId<'static> = shape_id!("smithy.example", "Blobby");
        const BLOBBY_DATA_ID: ShapeId<'static> = shape_id!("smithy.example", "Blobby", "data");
        static BLOBBY_DATA_MEMBER: Schema<'static> =
            Schema::new_member(BLOBBY_DATA_ID, ShapeType::Blob, "data", 0);
        static BLOBBY_SCHEMA: Schema<'static> =
            Schema::new_struct(BLOBBY_ID, ShapeType::Structure, &[&BLOBBY_DATA_MEMBER]);

        struct Blobby {
            data: Vec<u8>,
        }
        impl SerializableStruct for Blobby {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_blob(&BLOBBY_DATA_MEMBER, &self.data)
            }
        }

        let result = DiscriminatedDocument::from_struct(
            &BLOBBY_SCHEMA,
            &Blobby {
                data: b"raw".to_vec(),
            },
        );
        let err = result.expect_err(
            "from_struct on a struct with a Blob member should error during \
             Phase 3 (TryFrom bridge regression)",
        );
        assert!(
            matches!(err, SerdeError::TypeMismatch { .. }),
            "expected TypeMismatch, got {err:?}"
        );
    }

    #[test]
    #[should_panic(expected = "transitional panic on the new variant")]
    fn as_shape_on_top_level_blob_panics_during_phase3() {
        // The From bridge's Option-B arm panics with the documented
        // transitional message. We test only that the panic fires
        // with the expected message — the bridge itself is being
        // removed in Phase 4.
        let doc = DiscriminatedDocument::new(aws_smithy_types::Document::Blob(b"x".to_vec()));
        let _ = doc.as_shape(|deser| deser.read_blob(&prelude::BLOB));
    }

    // -- Discriminator preservation across construction paths ----------

    #[test]
    fn from_struct_then_as_shape_preserves_data_but_ignores_discriminator() {
        // The discriminator captured during from_struct is not
        // re-read during as_shape's deserialize callback (the callback
        // receives the document data, not the discriminator). This
        // matches the SEP behavior: callers wanting discriminator-
        // dispatched deserialization use a TypeRegistry, not as_shape.
        let original = Person {
            name: Some("Lee".into()),
            age: Some(25),
        };
        let doc = DiscriminatedDocument::from_struct(&PERSON_SCHEMA, &original).unwrap();
        assert_eq!(doc.discriminator(), Some("smithy.example#Person"));
        let restored: Person = doc.as_shape(|d| deserialize_person(d)).unwrap();
        assert_eq!(restored, original);
    }
}

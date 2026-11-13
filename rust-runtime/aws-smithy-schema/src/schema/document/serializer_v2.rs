/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentV2ShapeSerializer`], a [`ShapeSerializer`] that builds a [`DocumentV2`] tree.

use aws_smithy_types::document::DocumentObject;
use aws_smithy_types::{
    BigDecimal, BigInteger, Blob, DateTime, DiscriminatedDocument, Document, DocumentV2, Number,
};

use crate::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use crate::Schema;

/// Builds a [`DiscriminatedDocument<DocumentV2>`] from a typed shape through the
/// [`ShapeSerializer`] interface.
///
/// It works like [`DocumentShapeSerializer`](super::DocumentShapeSerializer), with a stack of
/// open structures, lists, and maps, but builds a [`DocumentV2`]. The differences:
///
/// - Blobs, timestamps, and arbitrary-precision numbers become the native
///   [`DocumentV2::Blob`], [`DocumentV2::Timestamp`], [`DocumentV2::BigInteger`], and
///   [`DocumentV2::BigDecimal`] variants, rather than strings and numbers.
/// - Structure members and map entries keep the order in which they were written.
/// - A [`Document`] value is converted with the lossless `From<&Document>` upgrade.
/// - [`finish`](DocumentV2ShapeSerializer::finish) attaches the discriminator with
///   [`DiscriminatedDocument::try_with_discriminator`], so it reports an invalid shape ID as an
///   error.
///
/// # Discriminator capture
///
/// As with the `Document` serializer, only the inherent
/// [`write_struct`](DocumentV2ShapeSerializer::write_struct) method records a discriminator, and
/// only for the first, top-level write. Nested structures written through
/// `&mut dyn ShapeSerializer` don't, because only the outer [`DiscriminatedDocument`] has a
/// discriminator.
///
/// # Example
///
/// [`DiscriminatedDocument::from_struct_v2`](crate::document::DiscriminatedDocumentV2Ext::from_struct_v2)
/// covers the common case. Use the serializer directly for other values:
///
/// ```
/// use aws_smithy_schema::document::DocumentV2ShapeSerializer;
/// use aws_smithy_schema::prelude;
/// use aws_smithy_schema::serde::ShapeSerializer;
/// use aws_smithy_types::{Blob, DocumentV2};
///
/// let mut ser = DocumentV2ShapeSerializer::new();
/// ser.write_blob(&prelude::BLOB, Blob::new(b"hello".to_vec())).unwrap();
/// let document = ser.finish().unwrap();
/// assert_eq!(document.document(), &DocumentV2::Blob(b"hello".to_vec()));
/// ```
#[derive(Debug, Default)]
pub struct DocumentV2ShapeSerializer {
    /// Open containers. The last one receives the next value.
    stack: Vec<Frame>,
    /// The root value, once a write has completed with no open container.
    finished: Option<DocumentV2>,
    /// The shape ID of the top-level structure, recorded by the inherent `write_struct`.
    root_discriminator: Option<String>,
}

/// An open container on the serializer's stack.
#[derive(Debug)]
enum Frame {
    Struct(DocumentObject),
    List(Vec<DocumentV2>),
    Map {
        entries: DocumentObject,
        /// The key of the entry whose value comes next, or `None` if the next value is a key.
        pending_key: Option<String>,
    },
}

impl DocumentV2ShapeSerializer {
    /// Creates a serializer with no value written yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the value written, wrapped in a [`DiscriminatedDocument<DocumentV2>`].
    ///
    /// The wrapper's discriminator is the shape ID of the schema passed to the inherent
    /// [`write_struct`](DocumentV2ShapeSerializer::write_struct), if that was the top-level
    /// write. No settings are attached, because a document built from a typed shape doesn't
    /// come from a protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if nothing was written, if a container is still open, or if the
    /// recorded shape ID isn't an absolute shape ID (`namespace#Name`), which happens when the
    /// top-level structure is written with a member schema.
    pub fn finish(self) -> Result<DiscriminatedDocument<DocumentV2>, SerdeError> {
        if !self.stack.is_empty() {
            return Err(SerdeError::custom(format!(
                "DocumentV2ShapeSerializer::finish called with {} unfinished container(s) on the stack",
                self.stack.len()
            )));
        }
        let document = self.finished.ok_or_else(|| {
            SerdeError::custom(
                "DocumentV2ShapeSerializer::finish called before any value was written",
            )
        })?;
        let wrapper = DiscriminatedDocument::new_v2(document);
        Ok(match self.root_discriminator {
            Some(shape_id) => wrapper.try_with_discriminator(shape_id)?,
            None => wrapper,
        })
    }

    /// Writes a structure and, if it's the top-level value, records `schema`'s shape ID as
    /// the discriminator.
    ///
    /// This inherent method takes precedence over [`ShapeSerializer::write_struct`] when the
    /// receiver is a concrete `DocumentV2ShapeSerializer`. Calls through
    /// `&mut dyn ShapeSerializer`, such as nested structures, use the trait method, which
    /// doesn't record a discriminator.
    pub fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        if self.stack.is_empty() && self.finished.is_none() {
            self.root_discriminator = Some(schema.shape_id().as_str().to_string());
        }
        <Self as ShapeSerializer>::write_struct(self, schema, value)
    }

    /// Adds `value` to the open container, or makes it the root value if none is open.
    fn commit_value(&mut self, schema: &Schema<'_>, value: DocumentV2) -> Result<(), SerdeError> {
        match self.stack.last_mut() {
            None => {
                if self.finished.is_some() {
                    return Err(SerdeError::custom(
                        "DocumentV2ShapeSerializer received multiple top-level writes",
                    ));
                }
                self.finished = Some(value);
            }
            Some(Frame::Struct(members)) => {
                let name = schema.member_name().ok_or_else(|| {
                    SerdeError::custom(format!(
                        "writing a struct member requires a member schema with member_name set; got schema for {}",
                        schema.shape_id().as_str()
                    ))
                })?;
                members.insert(name.to_string(), value);
            }
            Some(Frame::List(items)) => items.push(value),
            Some(Frame::Map {
                entries,
                pending_key,
            }) => match pending_key.take() {
                Some(key) => {
                    entries.insert(key, value);
                }
                // Map keys are always strings in the Smithy data model.
                None => match value {
                    DocumentV2::String(key) => *pending_key = Some(key),
                    other => {
                        return Err(SerdeError::custom(format!(
                            "map key must be a String document; got {}",
                            super::deserializer_v2::kind_name(&other),
                        )))
                    }
                },
            },
        }
        Ok(())
    }

    /// Pops the container that `write` filled.
    fn pop(&mut self, kind: &str) -> Result<Frame, SerdeError> {
        self.stack.pop().ok_or_else(|| {
            SerdeError::custom(format!(
                "DocumentV2ShapeSerializer {kind} frame was removed during serialization"
            ))
        })
    }
}

/// The error for a container that a nested write replaced with a different kind.
fn replaced_frame(kind: &str) -> SerdeError {
    SerdeError::custom(format!(
        "DocumentV2ShapeSerializer {kind} frame replaced by a different frame kind during serialization"
    ))
}

impl ShapeSerializer for DocumentV2ShapeSerializer {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::Struct(DocumentObject::new()));
        value.serialize_members(self)?;
        let Frame::Struct(members) = self.pop("struct")? else {
            return Err(replaced_frame("struct"));
        };
        self.commit_value(schema, DocumentV2::Object(members))
    }

    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::List(Vec::new()));
        write_elements(self)?;
        let Frame::List(items) = self.pop("list")? else {
            return Err(replaced_frame("list"));
        };
        self.commit_value(schema, DocumentV2::Array(items))
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::Map {
            entries: DocumentObject::new(),
            pending_key: None,
        });
        write_entries(self)?;
        let Frame::Map {
            entries,
            pending_key,
        } = self.pop("map")?
        else {
            return Err(replaced_frame("map"));
        };
        if pending_key.is_some() {
            return Err(SerdeError::custom(
                "map ended with an unmatched key (key written without a matching value)",
            ));
        }
        self.commit_value(schema, DocumentV2::Object(entries))
    }

    fn write_boolean(&mut self, schema: &Schema<'_>, value: bool) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Bool(value))
    }

    // `From<i64>` stores non-negative values as `Number::PosInt`, like the `Document`
    // serializer does.

    fn write_byte(&mut self, schema: &Schema<'_>, value: i8) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::from(i64::from(value)))
    }

    fn write_short(&mut self, schema: &Schema<'_>, value: i16) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::from(i64::from(value)))
    }

    fn write_integer(&mut self, schema: &Schema<'_>, value: i32) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::from(i64::from(value)))
    }

    fn write_long(&mut self, schema: &Schema<'_>, value: i64) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::from(value))
    }

    fn write_float(&mut self, schema: &Schema<'_>, value: f32) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Number(Number::Float(f64::from(value))))
    }

    fn write_double(&mut self, schema: &Schema<'_>, value: f64) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Number(Number::Float(value)))
    }

    fn write_big_integer(
        &mut self,
        schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::BigInteger(value.clone()))
    }

    fn write_big_decimal(
        &mut self,
        schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::BigDecimal(value.clone()))
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::String(value.to_string()))
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: Blob) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Blob(value.into_inner()))
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Timestamp(*value))
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::from(value))
    }

    fn write_document_v2(
        &mut self,
        schema: &Schema<'_>,
        value: &DocumentV2,
    ) -> Result<(), SerdeError> {
        self.commit_value(schema, value.clone())
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        self.commit_value(schema, DocumentV2::Null)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use super::*;
    use crate::{prelude, shape_id, ShapeId, ShapeType};

    const PERSON_ID: ShapeId<'static> = shape_id!("smithy.example", "Person");
    static PERSON_NAME_MEMBER: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Person", "name"),
        ShapeType::String,
        "name",
        0,
    );
    static PERSON_AGE_MEMBER: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Person", "age"),
        ShapeType::Integer,
        "age",
        1,
    );
    static PERSON_SCHEMA: Schema<'static> = Schema::new_struct(
        PERSON_ID,
        ShapeType::Structure,
        &[&PERSON_NAME_MEMBER, &PERSON_AGE_MEMBER],
    );

    /// Writes `age` before `name`, the reverse of the schema's member order.
    struct Person;

    impl SerializableStruct for Person {
        fn schema(&self) -> &Schema<'_> {
            &PERSON_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_integer(&PERSON_AGE_MEMBER, 7)?;
            ser.write_string(&PERSON_NAME_MEMBER, "Iago")
        }
    }

    fn write(
        write: impl FnOnce(&mut DocumentV2ShapeSerializer) -> Result<(), SerdeError>,
    ) -> DocumentV2 {
        let mut ser = DocumentV2ShapeSerializer::new();
        write(&mut ser).unwrap();
        ser.finish().unwrap().into_document()
    }

    #[test]
    fn scalars_become_their_native_variants() {
        let timestamp = DateTime::from_secs_f64(1_700_000_000.25);
        let big_integer = BigInteger::from_str("123456789012345678901234567890").unwrap();
        let big_decimal = BigDecimal::from_str("0.1000000000000000000001").unwrap();
        let cases = [
            (
                write(|s| s.write_boolean(&prelude::BOOLEAN, true)),
                DocumentV2::Bool(true),
            ),
            (
                write(|s| s.write_byte(&prelude::BYTE, -3)),
                DocumentV2::Number(Number::NegInt(-3)),
            ),
            (
                write(|s| s.write_short(&prelude::SHORT, 3)),
                DocumentV2::Number(Number::PosInt(3)),
            ),
            (
                write(|s| s.write_integer(&prelude::INTEGER, 0)),
                DocumentV2::Number(Number::PosInt(0)),
            ),
            (
                write(|s| s.write_long(&prelude::LONG, i64::MIN)),
                DocumentV2::Number(Number::NegInt(i64::MIN)),
            ),
            (
                write(|s| s.write_float(&prelude::FLOAT, 1.5)),
                DocumentV2::Number(Number::Float(1.5)),
            ),
            (
                write(|s| s.write_double(&prelude::DOUBLE, -0.25)),
                DocumentV2::Number(Number::Float(-0.25)),
            ),
            (
                write(|s| s.write_big_integer(&prelude::BIG_INTEGER, &big_integer)),
                DocumentV2::BigInteger(big_integer.clone()),
            ),
            (
                write(|s| s.write_big_decimal(&prelude::BIG_DECIMAL, &big_decimal)),
                DocumentV2::BigDecimal(big_decimal.clone()),
            ),
            (
                write(|s| s.write_string(&prelude::STRING, "hi")),
                DocumentV2::String("hi".to_string()),
            ),
            (
                write(|s| s.write_blob(&prelude::BLOB, Blob::new(b"raw".to_vec()))),
                DocumentV2::Blob(b"raw".to_vec()),
            ),
            (
                write(|s| s.write_timestamp(&prelude::TIMESTAMP, &timestamp)),
                DocumentV2::Timestamp(timestamp),
            ),
            (write(|s| s.write_null(&prelude::STRING)), DocumentV2::Null),
        ];
        for (actual, expected) in cases {
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn write_struct_keeps_member_order_and_records_the_discriminator() {
        let mut ser = DocumentV2ShapeSerializer::new();
        ser.write_struct(&PERSON_SCHEMA, &Person).unwrap();
        let document = ser.finish().unwrap();
        assert_eq!(document.discriminator(), Some("smithy.example#Person"));
        assert!(document.settings().is_none());
        let members = document.document().as_object().unwrap();
        assert_eq!(members.keys().collect::<Vec<_>>(), ["age", "name"]);
        assert_eq!(members["name"], DocumentV2::String("Iago".to_string()));
    }

    #[test]
    fn struct_written_through_the_trait_has_no_discriminator() {
        let mut ser = DocumentV2ShapeSerializer::new();
        let dyn_ser: &mut dyn ShapeSerializer = &mut ser;
        dyn_ser.write_struct(&PERSON_SCHEMA, &Person).unwrap();
        assert_eq!(ser.finish().unwrap().discriminator(), None);
    }

    #[test]
    fn finish_rejects_a_member_schema_as_the_discriminator() {
        let mut ser = DocumentV2ShapeSerializer::new();
        ser.write_struct(&PERSON_NAME_MEMBER, &Person).unwrap();
        let err = ser.finish().unwrap_err();
        assert!(
            matches!(&err, SerdeError::InvalidInput { message } if message.contains("smithy.example#Person$name")),
            "{err:?}"
        );
    }

    #[test]
    fn maps_and_lists_keep_their_order() {
        let document = write(|s| {
            s.write_map(&prelude::DOCUMENT, &|map| {
                for key in ["z", "a", "m"] {
                    map.write_string(&prelude::STRING, key)?;
                    map.write_list(&prelude::DOCUMENT, &|list| {
                        list.write_string(&prelude::STRING, key)?;
                        list.write_null(&prelude::STRING)
                    })?;
                }
                Ok(())
            })
        });
        let entries = document.as_object().unwrap();
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
        assert_eq!(
            entries["a"],
            DocumentV2::Array(vec![DocumentV2::String("a".to_string()), DocumentV2::Null])
        );
    }

    #[test]
    fn map_errors_match_the_document_serializer() {
        let mut ser = DocumentV2ShapeSerializer::new();
        let err = ser
            .write_map(&prelude::DOCUMENT, &|map| {
                map.write_blob(&prelude::BLOB, Blob::new(b"k".to_vec()))?;
                map.write_string(&prelude::STRING, "value")
            })
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("map key must be a String document; got blob"),
            "{err}"
        );

        let mut ser = DocumentV2ShapeSerializer::new();
        let err = ser
            .write_map(&prelude::DOCUMENT, &|map| {
                map.write_string(&prelude::STRING, "lonely")
            })
            .unwrap_err();
        assert!(err.to_string().contains("unmatched key"), "{err}");
    }

    #[test]
    fn write_document_upgrades_and_write_document_v2_copies() {
        let v1 = Document::Object(HashMap::from([(
            "n".to_string(),
            Document::Number(Number::NegInt(5)),
        )]));
        assert_eq!(
            write(|s| s.write_document(&prelude::DOCUMENT, &v1)),
            DocumentV2::from(&v1)
        );
        let v2 = DocumentV2::Array(vec![DocumentV2::Blob(b"x".to_vec())]);
        assert_eq!(write(|s| s.write_document_v2(&prelude::DOCUMENT, &v2)), v2);
    }

    #[test]
    fn finish_errors_match_the_document_serializer() {
        let err = DocumentV2ShapeSerializer::new().finish().unwrap_err();
        assert!(
            err.to_string().contains("before any value was written"),
            "{err}"
        );

        let mut ser = DocumentV2ShapeSerializer::new();
        ser.write_string(&prelude::STRING, "first").unwrap();
        let err = ser.write_string(&prelude::STRING, "second").unwrap_err();
        assert!(
            err.to_string().contains("multiple top-level writes"),
            "{err}"
        );
    }
}

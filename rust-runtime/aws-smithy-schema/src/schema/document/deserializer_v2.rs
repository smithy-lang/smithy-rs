/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentV2ShapeDeserializer`], a [`ShapeDeserializer`] that walks a [`DocumentV2`] tree.

use std::str::FromStr;
use std::sync::Arc;

use aws_smithy_types::date_time::Format;
use aws_smithy_types::{
    BigDecimal, BigInteger, Blob, DateTime, Document, DocumentSettings, DocumentV2,
};

use super::deserializer::{number_to_timestamp, resolve_member};
use crate::serde::{capped_container_size, SerdeError, ShapeDeserializer};
use crate::Schema;

/// Walks a [`DocumentV2`] tree through the [`ShapeDeserializer`] interface.
///
/// It works like [`DocumentShapeDeserializer`](super::DocumentShapeDeserializer): structure
/// members are matched by Smithy member name, unknown members are skipped, and attached
/// [`DocumentSettings`] decide how strings and numbers become blobs and timestamps. The
/// differences:
///
/// - The native [`DocumentV2::Blob`], [`DocumentV2::Timestamp`], [`DocumentV2::BigInteger`],
///   and [`DocumentV2::BigDecimal`] variants are read as is.
/// - Numbers are converted with `DocumentV2`'s own `coerce_*` methods, such as
///   [`DocumentV2::coerce_integer`]. Every numeric variant converts to every numeric type, a
///   fractional value is truncated toward zero, and an out-of-range value is an error. The
///   `Document` deserializer rejects fractional values instead.
/// - Arbitrary-precision numbers are also read from their text, which is how a [`Document`]
///   holds them, so that an upgraded `Document` reads back.
/// - Structure members and map entries are visited in the document's order.
/// - [`read_document`](ShapeDeserializer::read_document) returns
///   [`DocumentV2::to_document_lossy`], and
///   [`read_document_v2`](ShapeDeserializer::read_document_v2) returns the value unchanged.
///
/// # Example
///
/// [`DiscriminatedDocument::as_shape_v2`](crate::document::DiscriminatedDocumentV2Ext::as_shape_v2)
/// covers the common case. Use the deserializer directly for other values:
///
/// ```
/// use aws_smithy_schema::document::DocumentV2ShapeDeserializer;
/// use aws_smithy_schema::prelude;
/// use aws_smithy_schema::serde::ShapeDeserializer;
/// use aws_smithy_types::{DocumentV2, Number};
///
/// let document = DocumentV2::Number(Number::Float(41.9));
/// let mut deser = DocumentV2ShapeDeserializer::new(&document);
/// assert_eq!(deser.read_integer(&prelude::INTEGER).unwrap(), 41);
/// ```
#[derive(Debug)]
pub struct DocumentV2ShapeDeserializer<'a> {
    /// The value this deserializer reads.
    cursor: &'a DocumentV2,
    /// Settings for converting strings and numbers to blobs and timestamps, if the document
    /// came from a protocol. See [`DocumentV2ShapeDeserializer::new_with_settings`].
    settings: Option<Arc<dyn DocumentSettings>>,
}

impl<'a> DocumentV2ShapeDeserializer<'a> {
    /// Creates a deserializer for `document`, with no settings.
    ///
    /// Without settings, a string is read as a blob by decoding standard base64, and as a
    /// timestamp by parsing RFC 3339 (`date-time`). A number is read as a timestamp in epoch
    /// seconds.
    pub fn new(document: &'a DocumentV2) -> Self {
        Self {
            cursor: document,
            settings: None,
        }
    }

    /// Creates a deserializer for `document` with protocol settings.
    ///
    /// With settings, a string or number that is read as a blob or timestamp is converted by
    /// [`DocumentSettings::coerce_string_to_blob`],
    /// [`DocumentSettings::coerce_string_to_timestamp`], or
    /// [`DocumentSettings::coerce_number_to_timestamp`]. Native blobs and timestamps don't use
    /// the settings.
    pub fn new_with_settings(
        document: &'a DocumentV2,
        settings: Option<Arc<dyn DocumentSettings>>,
    ) -> Self {
        Self {
            cursor: document,
            settings,
        }
    }

    /// A deserializer for a value inside this one's, with the same settings.
    fn child(&self, document: &'a DocumentV2) -> Self {
        Self::new_with_settings(document, self.settings.clone())
    }
}

/// The name of `document`'s variant, for error messages.
pub(super) fn kind_name(document: &DocumentV2) -> &'static str {
    match document {
        DocumentV2::Null => "null",
        DocumentV2::Bool(_) => "boolean",
        DocumentV2::Number(_) => "number",
        DocumentV2::String(_) => "string",
        DocumentV2::Array(_) => "list",
        DocumentV2::Object(_) => "map",
        DocumentV2::Blob(_) => "blob",
        DocumentV2::Timestamp(_) => "timestamp",
        DocumentV2::BigInteger(_) => "bigInteger",
        DocumentV2::BigDecimal(_) => "bigDecimal",
        // `DocumentV2` is `#[non_exhaustive]`.
        _ => "unknown",
    }
}

/// The error for a read that found the wrong kind of value. The message matches the
/// `Document` deserializer's.
fn type_mismatch(expected: &str, found: &DocumentV2) -> SerdeError {
    SerdeError::type_mismatch(format!(
        "expected {expected} document, got {}",
        kind_name(found)
    ))
}

/// Parses a `bigInteger` from its text. Decimal text is truncated toward zero, by the same rule
/// as [`DocumentV2::coerce_big_integer`].
fn big_integer_from_text(text: &str) -> Result<BigInteger, SerdeError> {
    if let Ok(value) = BigInteger::from_str(text) {
        return Ok(value);
    }
    let decimal = BigDecimal::from_str(text).map_err(|e| {
        SerdeError::invalid_input(format!("cannot parse {text:?} as bigInteger: {e}"))
    })?;
    Ok(DocumentV2::BigDecimal(decimal).coerce_big_integer()?)
}

/// Parses a `bigDecimal` from its text.
fn big_decimal_from_text(text: &str) -> Result<BigDecimal, SerdeError> {
    BigDecimal::from_str(text)
        .map_err(|e| SerdeError::invalid_input(format!("cannot parse {text:?} as bigDecimal: {e}")))
}

impl ShapeDeserializer for DocumentV2ShapeDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let members = self
            .cursor
            .as_object()
            .ok_or_else(|| type_mismatch("struct (map)", self.cursor))?;
        for (name, value) in members {
            // Unknown members are skipped, like the JSON deserializer does.
            if let Some(member_schema) = resolve_member(schema, name) {
                consumer(member_schema, &mut self.child(value))?;
            }
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
            .as_array()
            .ok_or_else(|| type_mismatch("list", self.cursor))?;
        for item in items {
            consumer(&mut self.child(item))?;
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
            .as_object()
            .ok_or_else(|| type_mismatch("map", self.cursor))?;
        for (key, value) in entries {
            consumer(key.clone(), &mut self.child(value))?;
        }
        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        self.cursor
            .as_bool()
            .ok_or_else(|| type_mismatch("boolean", self.cursor))
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        Ok(self.cursor.coerce_byte()?)
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        Ok(self.cursor.coerce_short()?)
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        Ok(self.cursor.coerce_integer()?)
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        Ok(self.cursor.coerce_long()?)
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        Ok(self.cursor.coerce_float()?)
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        Ok(self.cursor.coerce_double()?)
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        match self.cursor {
            DocumentV2::String(text) => big_integer_from_text(text),
            other => Ok(other.coerce_big_integer()?),
        }
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        match self.cursor {
            DocumentV2::String(text) => big_decimal_from_text(text),
            other => Ok(other.coerce_big_decimal()?),
        }
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        match self.cursor {
            DocumentV2::String(s) => Ok(s.clone()),
            other => Err(type_mismatch("string", other)),
        }
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        match (self.cursor, &self.settings) {
            (DocumentV2::Blob(bytes), _) => Ok(Blob::new(bytes.clone())),
            (DocumentV2::String(s), Some(settings)) => settings
                .coerce_string_to_blob(s)
                .map(Blob::new)
                .map_err(SerdeError::from),
            (DocumentV2::String(s), None) => aws_smithy_types::base64::decode(s)
                .map(Blob::new)
                .map_err(|e| {
                    SerdeError::custom(format!("cannot base64-decode string as blob: {e}"))
                }),
            (other, _) => Err(type_mismatch("blob", other)),
        }
    }

    fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        match (self.cursor, &self.settings) {
            (DocumentV2::Timestamp(timestamp), _) => Ok(*timestamp),
            (DocumentV2::Number(n), Some(settings)) => settings
                .coerce_number_to_timestamp(n)
                .map_err(SerdeError::from),
            (DocumentV2::Number(n), None) => number_to_timestamp(n),
            (DocumentV2::String(s), Some(settings)) => settings
                .coerce_string_to_timestamp(s)
                .map_err(SerdeError::from),
            (DocumentV2::String(s), None) => DateTime::from_str(s, Format::DateTime).map_err(|e| {
                SerdeError::custom(format!("cannot parse string as a date-time timestamp: {e}"))
            }),
            (other, _) => Err(type_mismatch("timestamp", other)),
        }
    }

    /// Returns [`DocumentV2::to_document_lossy`], the same `Document` that a generated shape's
    /// public `Document` field holds for this value.
    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Ok(self.cursor.to_document_lossy())
    }

    fn read_document_v2(&mut self, _schema: &Schema<'_>) -> Result<DocumentV2, SerdeError> {
        Ok(self.cursor.clone())
    }

    fn is_null(&self) -> bool {
        self.cursor.is_null()
    }

    /// Nothing to advance: each value has its own deserializer, as in the `Document`
    /// deserializer.
    fn skip_value(&mut self) -> Result<(), SerdeError> {
        Ok(())
    }

    fn container_size(&self) -> Option<usize> {
        let size = match self.cursor {
            DocumentV2::Array(items) => items.len(),
            DocumentV2::Object(entries) => entries.len(),
            _ => return None,
        };
        Some(capped_container_size(size))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use aws_smithy_types::document::DocumentObject;
    use aws_smithy_types::{DocumentError, Number};

    use super::*;
    use crate::{prelude, shape_id, ShapeType};

    static M_NAME: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Person", "name"),
        ShapeType::String,
        "name",
        0,
    );
    static M_AGE: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Person", "age"),
        ShapeType::Integer,
        "age",
        1,
    );
    static PERSON_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Person"),
        ShapeType::Structure,
        &[&M_NAME, &M_AGE],
    );

    fn deser(document: &DocumentV2) -> DocumentV2ShapeDeserializer<'_> {
        DocumentV2ShapeDeserializer::new(document)
    }

    fn big_integer(text: &str) -> BigInteger {
        BigInteger::from_str(text).unwrap()
    }

    fn big_decimal(text: &str) -> BigDecimal {
        BigDecimal::from_str(text).unwrap()
    }

    /// Settings whose conversions are recognizable, so tests can tell them from the defaults.
    #[derive(Debug)]
    struct MarkerSettings;

    impl DocumentSettings for MarkerSettings {
        fn protocol_id(&self) -> &str {
            "smithy.example#Marker"
        }

        fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, DocumentError> {
            Ok(s.as_bytes().to_vec())
        }

        fn coerce_string_to_timestamp(&self, _s: &str) -> Result<DateTime, DocumentError> {
            Ok(DateTime::from_secs(1))
        }

        fn coerce_number_to_timestamp(&self, _n: &Number) -> Result<DateTime, DocumentError> {
            Ok(DateTime::from_secs(2))
        }
    }

    fn with_settings(document: &DocumentV2) -> DocumentV2ShapeDeserializer<'_> {
        DocumentV2ShapeDeserializer::new_with_settings(document, Some(Arc::new(MarkerSettings)))
    }

    #[test]
    fn read_struct_visits_known_members_in_document_order() {
        let document = DocumentV2::Object(DocumentObject::from([
            ("age".to_string(), DocumentV2::from(7i64)),
            ("unknown".to_string(), DocumentV2::Null),
            ("name".to_string(), DocumentV2::from("Iago")),
        ]));
        let mut seen = Vec::new();
        deser(&document)
            .read_struct(&PERSON_SCHEMA, &mut |member, sub| {
                let value = match member.member_index() {
                    Some(0) => sub.read_string(member)?,
                    _ => sub.read_integer(member)?.to_string(),
                };
                seen.push((member.member_name().unwrap().to_string(), value));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            seen,
            [
                ("age".to_string(), "7".to_string()),
                ("name".to_string(), "Iago".to_string())
            ]
        );
    }

    #[test]
    fn read_map_and_list_follow_document_order() {
        let document = DocumentV2::Object(DocumentObject::from([
            (
                "z".to_string(),
                DocumentV2::Array(vec![DocumentV2::from(1i64)]),
            ),
            ("a".to_string(), DocumentV2::Array(vec![DocumentV2::Null])),
        ]));
        let mut deserializer = deser(&document);
        assert_eq!(deserializer.container_size(), Some(2));
        let mut seen = Vec::new();
        deserializer
            .read_map(&prelude::DOCUMENT, &mut |key, value| {
                value.read_list(&prelude::DOCUMENT, &mut |item| {
                    seen.push((key.clone(), item.is_null()));
                    Ok(())
                })
            })
            .unwrap();
        assert_eq!(seen, [("z".to_string(), false), ("a".to_string(), true)]);
    }

    #[test]
    fn numbers_use_the_document_v2_accessor_rules() {
        let fraction = DocumentV2::Number(Number::Float(-41.9));
        assert_eq!(deser(&fraction).read_byte(&prelude::BYTE).unwrap(), -41);
        assert_eq!(deser(&fraction).read_long(&prelude::LONG).unwrap(), -41);
        let big = DocumentV2::BigInteger(big_integer("300"));
        assert_eq!(deser(&big).read_short(&prelude::SHORT).unwrap(), 300);
        assert!(matches!(
            deser(&big).read_byte(&prelude::BYTE),
            Err(SerdeError::NumericCoercionOverflow { .. })
        ));
        let decimal = DocumentV2::BigDecimal(big_decimal("1.5e1"));
        assert_eq!(deser(&decimal).read_integer(&prelude::INTEGER).unwrap(), 15);
        assert_eq!(deser(&decimal).read_double(&prelude::DOUBLE).unwrap(), 15.0);
        assert_eq!(deser(&decimal).read_float(&prelude::FLOAT).unwrap(), 15.0);
        assert!(matches!(
            deser(&DocumentV2::Number(Number::Float(f64::NAN))).read_integer(&prelude::INTEGER),
            Err(SerdeError::InvalidInput { .. })
        ));
        assert!(matches!(
            deser(&DocumentV2::from("1")).read_integer(&prelude::INTEGER),
            Err(SerdeError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn arbitrary_precision_reads_native_values_numbers_and_text() {
        let cases = [
            DocumentV2::BigInteger(big_integer("-12345678901234567890123")),
            DocumentV2::BigDecimal(big_decimal("-12345678901234567890123.9")),
            DocumentV2::from("-12345678901234567890123"),
            DocumentV2::from("-1.23456789012345678901239e22"),
        ];
        for document in &cases {
            assert_eq!(
                deser(document)
                    .read_big_integer(&prelude::BIG_INTEGER)
                    .unwrap(),
                big_integer("-12345678901234567890123"),
                "{document:?}"
            );
        }
        let n = DocumentV2::Number(Number::PosInt(7));
        assert_eq!(
            deser(&n).read_big_decimal(&prelude::BIG_DECIMAL).unwrap(),
            big_decimal("7")
        );
        let text = DocumentV2::from("0.1000000000000000000001");
        assert_eq!(
            deser(&text)
                .read_big_decimal(&prelude::BIG_DECIMAL)
                .unwrap(),
            big_decimal("0.1000000000000000000001")
        );
        let not_a_number = DocumentV2::from("twelve");
        assert!(matches!(
            deser(&not_a_number).read_big_integer(&prelude::BIG_INTEGER),
            Err(SerdeError::InvalidInput { .. })
        ));
        assert!(matches!(
            deser(&not_a_number).read_big_decimal(&prelude::BIG_DECIMAL),
            Err(SerdeError::InvalidInput { .. })
        ));
        assert!(matches!(
            deser(&DocumentV2::Bool(true)).read_big_integer(&prelude::BIG_INTEGER),
            Err(SerdeError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn blobs_are_native_or_decoded_strings() {
        let native = DocumentV2::Blob(b"raw".to_vec());
        assert_eq!(
            deser(&native).read_blob(&prelude::BLOB).unwrap().as_ref(),
            b"raw"
        );
        assert_eq!(
            with_settings(&native)
                .read_blob(&prelude::BLOB)
                .unwrap()
                .as_ref(),
            b"raw"
        );
        let text = DocumentV2::from("cmF3");
        assert_eq!(
            deser(&text).read_blob(&prelude::BLOB).unwrap().as_ref(),
            b"raw"
        );
        assert_eq!(
            with_settings(&text)
                .read_blob(&prelude::BLOB)
                .unwrap()
                .as_ref(),
            b"cmF3"
        );
        let err = deser(&DocumentV2::from("not base64!"))
            .read_blob(&prelude::BLOB)
            .unwrap_err();
        assert!(err.to_string().contains("base64"), "{err}");
        assert!(matches!(
            deser(&DocumentV2::Null).read_blob(&prelude::BLOB),
            Err(SerdeError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn timestamps_are_native_or_converted() {
        let timestamp = DateTime::from_secs_f64(1.5);
        let native = DocumentV2::Timestamp(timestamp);
        assert_eq!(
            deser(&native).read_timestamp(&prelude::TIMESTAMP).unwrap(),
            timestamp
        );
        assert_eq!(
            with_settings(&native)
                .read_timestamp(&prelude::TIMESTAMP)
                .unwrap(),
            timestamp
        );
        let number = DocumentV2::Number(Number::Float(1.5));
        assert_eq!(
            deser(&number).read_timestamp(&prelude::TIMESTAMP).unwrap(),
            timestamp
        );
        assert_eq!(
            with_settings(&number)
                .read_timestamp(&prelude::TIMESTAMP)
                .unwrap(),
            DateTime::from_secs(2)
        );
        let text = DocumentV2::from("1970-01-01T00:00:01.5Z");
        assert_eq!(
            deser(&text).read_timestamp(&prelude::TIMESTAMP).unwrap(),
            timestamp
        );
        assert_eq!(
            with_settings(&text)
                .read_timestamp(&prelude::TIMESTAMP)
                .unwrap(),
            DateTime::from_secs(1)
        );
        assert!(matches!(
            deser(&DocumentV2::BigInteger(big_integer("1"))).read_timestamp(&prelude::TIMESTAMP),
            Err(SerdeError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn read_document_downgrades_and_read_document_v2_copies() {
        let document = DocumentV2::Array(vec![
            DocumentV2::Blob(b"raw".to_vec()),
            DocumentV2::BigDecimal(big_decimal("0.5")),
        ]);
        assert_eq!(
            deser(&document).read_document(&prelude::DOCUMENT).unwrap(),
            document.to_document_lossy()
        );
        assert_eq!(
            deser(&document)
                .read_document_v2(&prelude::DOCUMENT)
                .unwrap(),
            document
        );
    }

    #[test]
    fn type_mismatches_name_both_kinds() {
        let document = DocumentV2::Blob(Vec::new());
        let err = deser(&document).read_string(&prelude::STRING).unwrap_err();
        assert_eq!(
            err.to_string(),
            SerdeError::type_mismatch("expected string document, got blob").to_string()
        );
        for result in [
            deser(&document).read_struct(&PERSON_SCHEMA, &mut |_, _| Ok(())),
            deser(&document).read_list(&prelude::DOCUMENT, &mut |_| Ok(())),
            deser(&document).read_map(&prelude::DOCUMENT, &mut |_, _| Ok(())),
            deser(&document).read_boolean(&prelude::BOOLEAN).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(SerdeError::TypeMismatch { .. })),
                "{result:?}"
            );
        }
        assert_eq!(deser(&document).container_size(), None);
        assert!(!deser(&document).is_null());
        assert!(deser(&DocumentV2::Null).is_null());
    }
}

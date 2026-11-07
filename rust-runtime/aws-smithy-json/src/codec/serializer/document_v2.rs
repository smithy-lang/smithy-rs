/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentV2`] support for [`JsonSerializer`].
//!
//! JSON has no native form for four `DocumentV2` variants. Each is written the way the
//! schema-driven writer writes the matching shape, with the codec settings standing in for the
//! member traits a document doesn't have:
//!
//! | Variant | JSON |
//! |---|---|
//! | `Blob` | base64 string, like `write_blob` |
//! | `Timestamp` | the codec's default timestamp format, like `write_timestamp` for a member without `@timestampFormat` |
//! | `BigInteger`, `BigDecimal` | the exact text, like `write_big_integer` and `write_big_decimal` |
//!
//! Object members are written in the order the [`DocumentObject`] holds them.

use super::JsonSerializer;
use crate::serialize::JsonValueWriter;
use aws_smithy_schema::serde::SerdeError;
use aws_smithy_schema::Schema;
use aws_smithy_types::document::DocumentObject;
use aws_smithy_types::{base64, DiscriminatedDocument, DocumentV2};

impl JsonSerializer {
    /// Writes `doc` as one JSON value. The caller writes any separator and key before it.
    pub(super) fn write_json_value_v2(&mut self, doc: &DocumentV2) -> Result<(), SerdeError> {
        match doc {
            DocumentV2::Null => self.output.push_str("null"),
            DocumentV2::Bool(value) => JsonValueWriter::new(&mut self.output).boolean(*value),
            DocumentV2::Number(value) => JsonValueWriter::new(&mut self.output).number(*value),
            DocumentV2::String(value) => JsonValueWriter::new(&mut self.output).string(value),
            // The base64 alphabet has no characters that need escaping.
            DocumentV2::Blob(bytes) => {
                JsonValueWriter::new(&mut self.output).string_unchecked(&base64::encode(bytes))
            }
            DocumentV2::Timestamp(value) => JsonValueWriter::new(&mut self.output)
                .date_time(value, self.settings.default_timestamp_format)
                .map_err(|e| {
                    SerdeError::write_failed(format!("failed to format timestamp: {e}"))
                })?,
            DocumentV2::BigInteger(value) => self.push_arbitrary_precision(value.as_ref())?,
            DocumentV2::BigDecimal(value) => self.push_arbitrary_precision(value.as_ref())?,
            DocumentV2::Array(items) => {
                self.output.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.output.push(',');
                    }
                    self.write_json_value_v2(item)?;
                }
                self.output.push(']');
            }
            DocumentV2::Object(entries) => self.write_json_object_v2(None, entries)?,
            // `DocumentV2` is `#[non_exhaustive]`. A new variant needs a deliberate encoding
            // here, not a guess.
            _ => {
                return Err(SerdeError::unsupported(
                    "the JSON codec doesn't support this DocumentV2 variant",
                ))
            }
        }
        Ok(())
    }

    /// Writes `entries` as a JSON object in order, starting with a `__type` member when
    /// `discriminator` is set.
    fn write_json_object_v2(
        &mut self,
        discriminator: Option<&str>,
        entries: &DocumentObject,
    ) -> Result<(), SerdeError> {
        self.output.push('{');
        let mut first = true;
        if let Some(id) = discriminator {
            self.output.push_str("\"__type\":");
            JsonValueWriter::new(&mut self.output).string(id);
            first = false;
        }
        for (key, value) in entries {
            if !first {
                self.output.push(',');
            }
            first = false;
            JsonValueWriter::new(&mut self.output).string(key);
            self.output.push(':');
            self.write_json_value_v2(value)?;
        }
        self.output.push('}');
        Ok(())
    }

    /// Writes the text of a `DocumentV2` big number with the rules of
    /// [`write_arbitrary_precision`](JsonSerializer::write_arbitrary_precision): a raw JSON
    /// number by default, or a string with `use_string_for_arbitrary_precision`.
    fn push_arbitrary_precision(&mut self, text: &str) -> Result<(), SerdeError> {
        if self.settings.use_string_for_arbitrary_precision() {
            JsonValueWriter::new(&mut self.output).string(text);
        } else {
            let text = self.json_number_text(text)?;
            self.output.push_str(&text);
        }
        Ok(())
    }

    /// Writes a [`DiscriminatedDocument<DocumentV2>`] as JSON.
    ///
    /// When the wrapper has a discriminator and holds an object, the discriminator comes first,
    /// as a `__type` member with the absolute shape ID, followed by the object's members in
    /// order. Otherwise the document is written as
    /// [`write_document_v2`](aws_smithy_schema::serde::ShapeSerializer::write_document_v2) writes
    /// it, and a discriminator on a value that isn't an object is not written, as in
    /// [`write_discriminated_document`](Self::write_discriminated_document).
    ///
    /// Unlike `write_discriminated_document`, this checks the discriminator in every build.
    /// It returns [`SerdeError::InvalidInput`] if the discriminator isn't an absolute shape ID
    /// (`namespace#Name`), or if the object also has a `__type` member, which would put two
    /// `__type` keys in the output. Nothing is written in either case.
    pub fn write_discriminated_document_v2(
        &mut self,
        schema: &Schema<'_>,
        value: &DiscriminatedDocument<DocumentV2>,
    ) -> Result<(), SerdeError> {
        let discriminator = value.discriminator();
        if let Some(id) = discriminator {
            if !crate::codec::is_valid_discriminator(id) {
                return Err(SerdeError::invalid_input(format!(
                    "discriminator {id:?} must be an absolute shape ID (namespace#Name)"
                )));
            }
        }
        match (discriminator, value.document()) {
            (Some(id), DocumentV2::Object(entries)) => {
                if entries.contains_key("__type") {
                    return Err(SerdeError::invalid_input(format!(
                        "the document has the discriminator {id:?} and also a `__type` member, \
                         and JSON writes both as `__type`"
                    )));
                }
                self.prefix(schema);
                self.write_json_object_v2(Some(id), entries)
            }
            (_, document) => {
                self.prefix(schema);
                self.write_json_value_v2(document)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::JsonCodecSettings;
    use aws_smithy_schema::prelude::{BIG_DECIMAL, BIG_INTEGER, BLOB, DOCUMENT, TIMESTAMP};
    use aws_smithy_schema::serde::{SerializableStruct, ShapeSerializer};
    use aws_smithy_schema::{shape_id, ShapeType};
    use aws_smithy_types::date_time::Format;
    use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document, Number};
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    fn serializer(settings: JsonCodecSettings) -> JsonSerializer {
        JsonSerializer::new(Arc::new(settings))
    }

    fn try_write(settings: JsonCodecSettings, doc: &DocumentV2) -> Result<String, SerdeError> {
        let mut ser = serializer(settings);
        ser.write_document_v2(&DOCUMENT, doc)?;
        Ok(String::from_utf8(ser.finish()).unwrap())
    }

    fn write(doc: &DocumentV2) -> String {
        try_write(JsonCodecSettings::default(), doc).unwrap()
    }

    fn object<const N: usize>(entries: [(&str, DocumentV2); N]) -> DocumentV2 {
        DocumentV2::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    #[test]
    fn writes_json_shaped_values_like_write_document() {
        let cases = [
            Document::Null,
            Document::Bool(true),
            Document::Bool(false),
            Document::Number(Number::PosInt(u64::MAX)),
            Document::Number(Number::NegInt(i64::MIN)),
            Document::Number(Number::Float(1.5)),
            Document::Number(Number::Float(-0.0)),
            Document::Number(Number::Float(f64::NAN)),
            Document::Number(Number::Float(f64::INFINITY)),
            Document::Number(Number::Float(f64::NEG_INFINITY)),
            Document::String("quote \" backslash \\ newline \n control \u{1}".into()),
            Document::Array(vec![]),
            Document::Array(vec![Document::Null, Document::Array(vec![1u64.into()])]),
            Document::Object(HashMap::new()),
            // One key per object, so `HashMap` order can't affect the comparison.
            Document::Object(HashMap::from([(
                "key \"quoted\"".to_owned(),
                Document::Object(HashMap::from([("inner".to_owned(), "value".into())])),
            )])),
        ];
        for doc in cases {
            let mut v1 = serializer(JsonCodecSettings::default());
            v1.write_document(&DOCUMENT, &doc).unwrap();
            let v1 = String::from_utf8(v1.finish()).unwrap();
            assert_eq!(write(&DocumentV2::from(&doc)), v1, "{doc:?}");
        }
    }

    #[test]
    fn keeps_object_members_in_order() {
        let doc = object([
            ("zeta", 1i64.into()),
            (
                "alpha",
                DocumentV2::Array(vec![object([("b", 2i64.into()), ("a", 3i64.into())])]),
            ),
            ("mid", DocumentV2::Null),
        ]);
        assert_eq!(
            write(&doc),
            r#"{"zeta":1,"alpha":[{"b":2,"a":3}],"mid":null}"#
        );
    }

    #[test]
    fn writes_blobs_like_write_blob() {
        for bytes in [&b""[..], &b"abcd"[..], &[0xfb, 0xff][..]] {
            let mut typed = serializer(JsonCodecSettings::default());
            typed.write_blob(&BLOB, Blob::new(bytes)).unwrap();
            let typed = String::from_utf8(typed.finish()).unwrap();
            assert_eq!(write(&DocumentV2::Blob(bytes.to_vec())), typed);
        }
        assert_eq!(write(&DocumentV2::Blob(b"abcd".to_vec())), r#""YWJjZA==""#);
        assert_eq!(write(&DocumentV2::Blob(vec![0xfb, 0xff])), r#""+/8=""#);
    }

    #[test]
    fn writes_timestamps_in_the_default_format_like_write_timestamp() {
        let timestamps = [
            DateTime::from_secs(0),
            DateTime::from_secs(1_577_836_800),
            DateTime::from_secs_f64(1.5),
            DateTime::from_secs_f64(-1.5),
        ];
        for format in [Format::EpochSeconds, Format::DateTime, Format::HttpDate] {
            for timestamp in timestamps {
                let settings = || {
                    JsonCodecSettings::builder()
                        .default_timestamp_format(format)
                        .build()
                };
                let mut typed = serializer(settings());
                typed.write_timestamp(&TIMESTAMP, &timestamp).unwrap();
                let typed = String::from_utf8(typed.finish()).unwrap();
                let doc = try_write(settings(), &DocumentV2::Timestamp(timestamp)).unwrap();
                assert_eq!(doc, typed, "{timestamp:?} as {format:?}");
            }
        }

        let epoch = DocumentV2::Timestamp(DateTime::from_secs(0));
        assert_eq!(write(&epoch), "0");
        let date_time = JsonCodecSettings::builder()
            .default_timestamp_format(Format::DateTime)
            .build();
        assert_eq!(
            try_write(date_time, &epoch).unwrap(),
            r#""1970-01-01T00:00:00Z""#
        );
    }

    #[test]
    fn writes_big_numbers_like_the_shape_writers() {
        let settings: [fn() -> JsonCodecSettings; 3] = [
            JsonCodecSettings::default,
            || {
                JsonCodecSettings::builder()
                    .use_string_for_arbitrary_precision(true)
                    .build()
            },
            || {
                JsonCodecSettings::builder()
                    .normalize_arbitrary_precision(true)
                    .build()
            },
        ];
        let integers = ["123456789012345678901234567890", "-1", "00123"];
        let decimals = [
            "1.2345678901234567890123456789e-40",
            "-0.5",
            "+007",
            ".5",
            "1.",
        ];
        for settings in settings {
            for text in integers {
                let value = BigInteger::from_str(text).unwrap();
                let mut typed = serializer(settings());
                let typed = typed
                    .write_big_integer(&BIG_INTEGER, &value)
                    .map(|_| String::from_utf8(typed.finish()).unwrap());
                let doc = try_write(settings(), &DocumentV2::BigInteger(value));
                assert_eq!(format!("{doc:?}"), format!("{typed:?}"), "{text}");
            }
            for text in decimals {
                let value = BigDecimal::from_str(text).unwrap();
                let mut typed = serializer(settings());
                let typed = typed
                    .write_big_decimal(&BIG_DECIMAL, &value)
                    .map(|_| String::from_utf8(typed.finish()).unwrap());
                let doc = try_write(settings(), &DocumentV2::BigDecimal(value));
                assert_eq!(format!("{doc:?}"), format!("{typed:?}"), "{text}");
            }
        }

        let big = DocumentV2::BigInteger(BigInteger::from_str("18446744073709551616").unwrap());
        assert_eq!(write(&big), "18446744073709551616");
        let leading_zeros = DocumentV2::Array(vec![DocumentV2::BigInteger(
            BigInteger::from_str("00123").unwrap(),
        )]);
        let error = try_write(JsonCodecSettings::default(), &leading_zeros).unwrap_err();
        assert!(
            matches!(error, SerdeError::InvalidInput { .. }),
            "{error:?}"
        );
    }

    /// A structure with a document member followed by a string member.
    struct Holder;

    static DOC_MEMBER: Schema<'static> = Schema::new_member(
        shape_id!("test", "Holder", "doc"),
        ShapeType::Document,
        "doc",
        0,
    );
    static NAME_MEMBER: Schema<'static> = Schema::new_member(
        shape_id!("test", "Holder", "name"),
        ShapeType::String,
        "name",
        1,
    );
    static HOLDER: Schema<'static> = Schema::new_struct(
        shape_id!("test", "Holder"),
        ShapeType::Structure,
        &[&DOC_MEMBER, &NAME_MEMBER],
    );

    impl SerializableStruct for Holder {
        fn schema(&self) -> &Schema<'_> {
            &HOLDER
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_document_v2(
                &DOC_MEMBER,
                &object([("b", DocumentV2::Blob(b"a".to_vec()))]),
            )?;
            ser.write_string(&NAME_MEMBER, "x")
        }
    }

    #[test]
    fn writes_documents_in_structures_lists_and_maps() {
        let mut ser = serializer(JsonCodecSettings::default());
        ser.write_struct(&HOLDER, &Holder).unwrap();
        assert_eq!(
            String::from_utf8(ser.finish()).unwrap(),
            r#"{"doc":{"b":"YQ=="},"name":"x"}"#
        );

        let list = Schema::new(shape_id!("test", "List"), ShapeType::List);
        let mut ser = serializer(JsonCodecSettings::default());
        ser.write_list(&list, &|ser| {
            ser.write_document_v2(&DOCUMENT, &DocumentV2::Blob(b"a".to_vec()))?;
            ser.write_document_v2(&DOCUMENT, &object([("k", DocumentV2::Null)]))
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(ser.finish()).unwrap(),
            r#"["YQ==",{"k":null}]"#
        );

        let map = Schema::new(shape_id!("test", "Map"), ShapeType::Map);
        let mut ser = serializer(JsonCodecSettings::default());
        ser.write_map(&map, &|ser| {
            ser.write_string(&DOCUMENT, "first")?;
            ser.write_document_v2(&DOCUMENT, &object([("k", true.into())]))?;
            ser.write_string(&DOCUMENT, "second")?;
            ser.write_document_v2(&DOCUMENT, &DocumentV2::Array(vec![]))
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(ser.finish()).unwrap(),
            r#"{"first":{"k":true},"second":[]}"#
        );
    }

    /// The trait's default would return an error for a blob, which `Document` can't hold.
    #[test]
    fn the_override_is_reached_through_a_trait_object() {
        let mut ser = serializer(JsonCodecSettings::default());
        let dyn_ser: &mut dyn ShapeSerializer = &mut ser;
        dyn_ser
            .write_document_v2(&DOCUMENT, &DocumentV2::Blob(b"abcd".to_vec()))
            .unwrap();
        assert_eq!(String::from_utf8(ser.finish()).unwrap(), r#""YWJjZA==""#);
    }

    fn write_discriminated(
        value: &DiscriminatedDocument<DocumentV2>,
    ) -> Result<String, SerdeError> {
        let mut ser = serializer(JsonCodecSettings::default());
        ser.write_discriminated_document_v2(&DOCUMENT, value)?;
        Ok(String::from_utf8(ser.finish()).unwrap())
    }

    fn discriminated(doc: DocumentV2, id: &str) -> DiscriminatedDocument<DocumentV2> {
        DiscriminatedDocument::new_v2(doc)
            .try_with_discriminator(id)
            .unwrap()
    }

    #[test]
    fn write_discriminated_document_v2_writes_the_discriminator_first() {
        let doc = object([("b", DocumentV2::Blob(b"a".to_vec())), ("a", 1i64.into())]);
        assert_eq!(
            write_discriminated(&discriminated(doc, "com.example#Greeting")).unwrap(),
            r#"{"__type":"com.example#Greeting","b":"YQ==","a":1}"#
        );
        assert_eq!(
            write_discriminated(&discriminated(object([]), "com.example#Empty")).unwrap(),
            r#"{"__type":"com.example#Empty"}"#
        );
    }

    #[test]
    fn write_discriminated_document_v2_without_an_object_discriminator_writes_the_document() {
        let doc = object([("a", DocumentV2::Timestamp(DateTime::from_secs(5)))]);
        assert_eq!(
            write_discriminated(&DiscriminatedDocument::new_v2(doc.clone())).unwrap(),
            write(&doc)
        );
        assert_eq!(
            write_discriminated(&discriminated("text".into(), "com.example#Text")).unwrap(),
            r#""text""#
        );
    }

    /// Checked in every build, unlike the `debug_assert!` in `write_discriminated_document`.
    #[test]
    fn write_discriminated_document_v2_rejects_an_invalid_discriminator() {
        for id in [
            "Greeting",
            "com.example#Greeting$member",
            "com-example#Greeting",
        ] {
            for doc in [object([("a", 1i64.into())]), "text".into()] {
                let mut value = DiscriminatedDocument::new_v2(doc);
                value.set_discriminator_unchecked(id);
                let mut ser = serializer(JsonCodecSettings::default());
                let error = ser
                    .write_discriminated_document_v2(&DOCUMENT, &value)
                    .unwrap_err();
                assert!(
                    matches!(error, SerdeError::InvalidInput { .. }),
                    "{error:?}"
                );
                assert!(error.to_string().contains(id), "{error}");
                assert_eq!(ser.finish(), b"", "{id}");
            }
        }
    }

    #[test]
    fn write_discriminated_document_v2_rejects_a_type_member() {
        let value = discriminated(
            object([("__type", "com.example#Other".into())]),
            "com.example#Greeting",
        );
        let mut ser = serializer(JsonCodecSettings::default());
        let error = ser
            .write_discriminated_document_v2(&DOCUMENT, &value)
            .unwrap_err();
        assert!(
            matches!(error, SerdeError::InvalidInput { .. }),
            "{error:?}"
        );
        assert_eq!(ser.finish(), b"");

        // Without a discriminator, a `__type` member is ordinary data.
        let value = DiscriminatedDocument::new_v2(object([("__type", "x".into())]));
        assert_eq!(write_discriminated(&value).unwrap(), r#"{"__type":"x"}"#);
    }
}

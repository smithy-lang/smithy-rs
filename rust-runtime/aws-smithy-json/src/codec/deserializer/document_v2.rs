/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentV2`] support for [`JsonDeserializer`].
//!
//! The reader has the structure of `read_document`, including its strictness settings, depth
//! limit, and trailing-input check. It differs in two ways: objects keep their members in source
//! order, and numbers that don't fit `Number` read as `BigInteger` or `BigDecimal` instead of
//! being rounded to an `f64`. JSON strings stay strings: JSON has no native blob or timestamp,
//! and the settings attached by
//! [`read_discriminated_document_v2`](JsonDeserializer::read_discriminated_document_v2) coerce
//! strings and numbers on request.

use super::JsonDeserializer;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::document::DocumentObject;
use aws_smithy_types::primitive::Encoder;
use aws_smithy_types::{
    BigDecimal, BigInteger, DiscriminatedDocument, DocumentSettings, DocumentV2, Number,
};
use std::num::IntErrorKind;
use std::str::FromStr;
use std::sync::Arc;

impl<'a> JsonDeserializer<'a> {
    /// Implements [`ShapeDeserializer::read_document_v2`].
    pub(super) fn read_json_value_v2(
        &mut self,
        schema: &Schema<'_>,
    ) -> Result<DocumentV2, SerdeError> {
        self.depth += 1;
        if self.depth > self.settings.max_depth() {
            return Err(SerdeError::custom("maximum nesting depth exceeded"));
        }
        self.skip_whitespace();
        let result = match self.remaining().first() {
            Some(b'"') => Ok(DocumentV2::String(self.read_string(schema)?)),
            Some(b't') | Some(b'f') => Ok(DocumentV2::Bool(self.read_boolean(schema)?)),
            Some(b'n') => {
                if self.remaining().starts_with(b"null") {
                    self.advance_by(4);
                    Ok(DocumentV2::Null)
                } else {
                    Err(SerdeError::invalid_input("unexpected token in document"))
                }
            }
            Some(b'{') => {
                self.advance_by(1);
                let mut entries = DocumentObject::new();
                let mut first = true;
                loop {
                    if self.next_element(first, b'}', "document object")? {
                        break;
                    }
                    first = false;
                    if self.remaining().first() != Some(&b'"') {
                        return Err(SerdeError::invalid_input("expected object key in document"));
                    }
                    let key = self.parse_key()?.into_owned();
                    self.skip_whitespace();
                    if self.remaining().first() != Some(&b':') {
                        return Err(SerdeError::invalid_input(
                            "expected colon in document object",
                        ));
                    }
                    self.advance_by(1);
                    self.skip_whitespace();
                    let value = self.read_json_value_v2(schema)?;
                    // A repeated key keeps its first position and takes the last value.
                    // `read_document` also keeps the last value.
                    entries.insert(key, value);
                }
                Ok(DocumentV2::Object(entries))
            }
            Some(b'[') => {
                self.advance_by(1);
                let mut items = Vec::new();
                let mut first = true;
                loop {
                    if self.next_element(first, b']', "document array")? {
                        break;
                    }
                    first = false;
                    items.push(self.read_json_value_v2(schema)?);
                }
                Ok(DocumentV2::Array(items))
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                // The codec's number lexer sets the token's extent, with the same strictness
                // as `read_document`.
                let start = self.position;
                self.consume_number()?;
                let input = self.input;
                let text = std::str::from_utf8(&input[start..self.position])
                    .map_err(|e| SerdeError::invalid_input(e.to_string()))?;
                document_v2_number(text)
            }
            _ => Err(SerdeError::invalid_input("unexpected token in document")),
        };
        if result.is_ok() {
            self.leave_container()?;
        }
        result
    }

    /// Reads a JSON value as a [`DiscriminatedDocument<DocumentV2>`], with the codec's
    /// [`DocumentSettings`] attached and a top-level `__type` member lifted into the
    /// discriminator.
    ///
    /// This is [`read_discriminated_document`](Self::read_discriminated_document) for
    /// `DocumentV2`. Values are read as
    /// [`read_document_v2`](ShapeDeserializer::read_document_v2) reads them, and `__type` is
    /// lifted by the same rules, with one more check: the resolved shape ID must be one that
    /// [`DiscriminatedDocument::try_with_discriminator`] accepts. That method checks the full
    /// Smithy shape ID grammar, so a value such as `"my-namespace#Bird"`, which
    /// `read_discriminated_document` lifts, stays in the object. Lifting `__type` keeps the
    /// other members in order.
    ///
    /// The settings let [`DiscriminatedDocument::as_blob`] decode a base64 string and
    /// [`DiscriminatedDocument::as_timestamp`] read a timestamp from a string or a number.
    ///
    /// # Example
    ///
    /// ```
    /// use aws_smithy_json::codec::JsonCodec;
    /// use aws_smithy_schema::codec::Codec;
    /// use aws_smithy_types::DocumentV2;
    ///
    /// let codec = JsonCodec::default();
    /// let bytes = br#"{"name":"Iago","__type":"smithy.example#Bird","wingspan":1e400}"#;
    /// let mut deser = codec.create_deserializer(bytes);
    /// let doc = deser.read_discriminated_document_v2().unwrap();
    ///
    /// assert_eq!(doc.discriminator(), Some("smithy.example#Bird"));
    ///
    /// // The other members keep their order, and a number beyond `f64` keeps its text.
    /// let object = doc.document().as_object().unwrap();
    /// let keys: Vec<&str> = object.keys().map(String::as_str).collect();
    /// assert_eq!(keys, ["name", "wingspan"]);
    /// assert!(matches!(&object["wingspan"], DocumentV2::BigDecimal(value) if value.as_ref() == "1e400"));
    /// ```
    pub fn read_discriminated_document_v2(
        &mut self,
    ) -> Result<DiscriminatedDocument<DocumentV2>, SerdeError> {
        let mut doc = self.read_json_value_v2(&aws_smithy_schema::prelude::DOCUMENT)?;
        let mut discriminator = None;
        if let DocumentV2::Object(entries) = &mut doc {
            discriminator = match entries.get("__type") {
                Some(DocumentV2::String(value)) => {
                    Self::resolve_shape_id(value, self.settings.default_namespace())
                }
                _ => None,
            }
            .filter(|id| crate::codec::is_valid_discriminator(id));
            if discriminator.is_some() {
                entries.remove("__type");
            }
        }

        let settings: Arc<dyn DocumentSettings> = self.settings.clone();
        let wrapper = DiscriminatedDocument::new_v2(doc).with_settings(settings);
        match discriminator {
            Some(id) => wrapper
                .try_with_discriminator(id)
                .map_err(|e| SerdeError::invalid_input(e.to_string())),
            None => Ok(wrapper),
        }
    }
}

/// Reads a JSON number token into the first `DocumentV2` variant that holds it exactly.
///
/// - Integer text reads as `Number::PosInt` or `Number::NegInt` when it fits a `u64` or an
///   `i64`, and otherwise as `BigInteger`, keeping the text.
/// - Text with a fraction or an exponent reads as `Number::Float` when the nearest `f64` writes
///   back as the same value, and otherwise as `BigDecimal`, keeping the text. So `0.1` and
///   `1e300` read as floats, while `1e400` (beyond the `f64` range), `1e-400` (closer to zero
///   than any `f64` but zero), and `3.14159265358979323846` (more digits than an `f64` holds)
///   read as big decimals.
///
/// `read_document` reads integers the same way when they fit, and everything else through
/// `f64`.
fn document_v2_number(text: &str) -> Result<DocumentV2, SerdeError> {
    let invalid = |e: &dyn std::fmt::Display| SerdeError::invalid_input(e.to_string());
    if text.contains(['.', 'e', 'E']) {
        let value: f64 = text.parse().map_err(|e| invalid(&e))?;
        let exact = BigDecimal::from_str(text).map_err(|e| invalid(&e))?;
        // `Encoder` writes the shortest text that reads back as `value`, which is what
        // `write_document_v2` writes. `BigDecimal` compares numeric values, so `1e2` and
        // `100.0` are equal.
        let float_holds_it = value.is_finite()
            && BigDecimal::from_str(Encoder::from(value).encode())
                .is_ok_and(|written| written == exact);
        return Ok(if float_holds_it {
            DocumentV2::Number(Number::Float(value))
        } else {
            DocumentV2::BigDecimal(exact)
        });
    }
    let number = if text.starts_with('-') {
        text.parse::<i64>().map(Number::NegInt)
    } else {
        text.parse::<u64>().map(Number::PosInt)
    };
    match number {
        Ok(number) => Ok(DocumentV2::Number(number)),
        Err(e)
            if matches!(
                e.kind(),
                IntErrorKind::PosOverflow | IntErrorKind::NegOverflow
            ) =>
        {
            BigInteger::from_str(text)
                .map(DocumentV2::BigInteger)
                .map_err(|e| invalid(&e))
        }
        Err(e) => Err(invalid(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::JsonCodecSettings;
    use aws_smithy_schema::prelude::DOCUMENT;
    use aws_smithy_schema::{shape_id, ShapeType};
    use aws_smithy_types::Document;

    fn read_with(settings: JsonCodecSettings, input: &str) -> Result<DocumentV2, SerdeError> {
        JsonDeserializer::new(input.as_bytes(), Arc::new(settings)).read_document_v2(&DOCUMENT)
    }

    fn read(input: &str) -> DocumentV2 {
        read_with(JsonCodecSettings::default(), input).unwrap_or_else(|e| panic!("{input}: {e:?}"))
    }

    fn read_v1(settings: JsonCodecSettings, input: &str) -> Result<Document, SerdeError> {
        JsonDeserializer::new(input.as_bytes(), Arc::new(settings)).read_document(&DOCUMENT)
    }

    fn strictness() -> [fn() -> JsonCodecSettings; 2] {
        [JsonCodecSettings::default, || {
            JsonCodecSettings::builder()
                .enforce_strictness(true)
                .build()
        }]
    }

    fn keys(doc: &DocumentV2) -> Vec<&str> {
        doc.as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// For values `Document` holds without loss, both readers agree.
    #[test]
    fn reads_values_that_document_holds_like_read_document() {
        let inputs = [
            "null",
            "true",
            "false",
            "0",
            "-0",
            "42",
            "-123",
            "18446744073709551615",
            "-9223372036854775808",
            "1.5",
            "-0.0",
            "1e-5",
            "1E+2",
            "0.1",
            r#""text \n \u0041 \"quoted\"""#,
            "[]",
            "[1,[2,{}]]",
            "{}",
            r#"{"a":{"b":[null,"c"]}}"#,
            " { \"a\" : [ 1 , 2 ] } ",
        ];
        for settings in strictness() {
            for input in inputs {
                let v1 = read_v1(settings(), input).unwrap();
                let v2 = read_with(settings(), input).unwrap();
                assert_eq!(v2, DocumentV2::from(&v1), "{input}");
            }
        }
        assert!(read("-0.0").as_double().unwrap().is_sign_negative());
    }

    /// Both readers accept and reject the same input, apart from the numbers in
    /// `reads_big_numbers_that_document_rounds`.
    #[test]
    fn accepts_and_rejects_the_same_input_as_read_document() {
        let inputs = [
            "",
            " ",
            "nul",
            "nulls",
            "tru",
            "1e",
            "1e-",
            "1-2",
            "01",
            "1.",
            "1e+-2",
            "--1",
            "-",
            "1e5e5",
            "+1",
            ".5",
            r#"{"a":1}x"#,
            r#"{"a":1} {}"#,
            "[1,]",
            "[,1]",
            "[1 2]",
            r#"{"a":1,,"b":2}"#,
            r#"{"a" 1}"#,
            r#"{a:1}"#,
            r#"{"a":1"#,
            r#""unterminated"#,
            "\"raw\ncontrol\"",
            r#"{"a":[1,{"b":null}],"c":"d"}"#,
        ];
        for settings in strictness() {
            for input in inputs {
                let v1 = read_v1(settings(), input);
                let v2 = read_with(settings(), input);
                assert_eq!(v2.is_ok(), v1.is_ok(), "{input}: {v1:?} {v2:?}");
            }
        }
    }

    #[test]
    fn keeps_object_members_in_source_order() {
        let doc = read(r#"{"zeta":1,"alpha":{"b":2,"a":3},"mid":null}"#);
        assert_eq!(keys(&doc), ["zeta", "alpha", "mid"]);
        assert_eq!(keys(&doc.as_object().unwrap()["alpha"]), ["b", "a"]);

        // A repeated key keeps its first position and its last value.
        let doc = read(r#"{"a":1,"b":2,"a":3}"#);
        assert_eq!(keys(&doc), ["a", "b"]);
        assert_eq!(doc.as_object().unwrap()["a"], DocumentV2::from(3u64));
    }

    /// `read_document` rounds each of these to an `f64`.
    #[test]
    fn reads_big_numbers_that_document_rounds() {
        let integers = [
            "18446744073709551616",
            "-9223372036854775809",
            "99999999999999999999999",
            "-99999999999999999999999",
        ];
        let decimals = [
            "1e400",
            "-1e400",
            "1.7976931348623159e308",
            "1e-400",
            "3.14159265358979323846264338327950288",
            "0.1000000000000000055511151231257827",
            "9007199254740993.0",
        ];
        for settings in strictness() {
            for text in integers {
                match read_with(settings(), text).unwrap() {
                    DocumentV2::BigInteger(value) => assert_eq!(value.as_ref(), text),
                    other => panic!("{text}: expected BigInteger, got {other:?}"),
                }
                assert!(matches!(
                    read_v1(settings(), text).unwrap(),
                    Document::Number(Number::Float(_))
                ));
            }
            for text in decimals {
                match read_with(settings(), text).unwrap() {
                    DocumentV2::BigDecimal(value) => assert_eq!(value.as_ref(), text),
                    other => panic!("{text}: expected BigDecimal, got {other:?}"),
                }
                assert!(matches!(
                    read_v1(settings(), text).unwrap(),
                    Document::Number(Number::Float(_))
                ));
            }
        }

        // Nested values take the same path.
        let doc = read(r#"{"n":[18446744073709551616,1e400]}"#);
        let list = doc.as_object().unwrap()["n"].as_array().unwrap();
        assert!(matches!(list[0], DocumentV2::BigInteger(_)));
        assert!(matches!(list[1], DocumentV2::BigDecimal(_)));

        // Leading zeros, when allowed, stay in the text of a big integer.
        let lenient = || {
            JsonCodecSettings::builder()
                .enforce_strictness(true)
                .allow_leading_zeros(true)
                .build()
        };
        assert_eq!(read_with(lenient(), "007").unwrap(), DocumentV2::from(7u64));
        match read_with(lenient(), "0018446744073709551616").unwrap() {
            DocumentV2::BigInteger(value) => assert_eq!(value.as_ref(), "0018446744073709551616"),
            other => panic!("expected BigInteger, got {other:?}"),
        }
    }

    /// Generated shapes derive their public `Document` fields from the V2 read with
    /// `to_document_lossy`, so for every number it must give what `read_document` and the
    /// token parser that `protocol_serde` uses give.
    #[test]
    fn the_lossy_downgrade_of_a_read_matches_read_document() {
        let inputs = [
            "0",
            "-0",
            "42",
            "-123",
            "18446744073709551615",
            "18446744073709551616",
            "-9223372036854775808",
            "-9223372036854775809",
            "99999999999999999999999",
            "-99999999999999999999999",
            "0.1",
            "-0.0",
            "1e2",
            "0.1234567890123456789",
            "1e400",
            "-1e400",
            "1.7976931348623159e308",
            "1e-400",
            "3.14159265358979323846264338327950288",
            "9007199254740993.0",
            r#"{"a":[18446744073709551616,{"b":1e400}],"c":0.1234567890123456789}"#,
        ];
        for settings in strictness() {
            for input in inputs {
                let v2 = read_with(settings(), input).unwrap();
                assert_eq!(
                    v2.to_document_lossy(),
                    read_v1(settings(), input).unwrap(),
                    "{input}"
                );
            }
        }
        for input in inputs {
            let mut tokens = crate::deserialize::json_token_iter(input.as_bytes()).peekable();
            let legacy = crate::deserialize::token::expect_document(&mut tokens).unwrap();
            assert_eq!(read(input).to_document_lossy(), legacy, "{input}");
        }
    }

    #[test]
    fn reads_decimals_that_f64_holds_as_floats() {
        for (text, expected) in [
            ("0.1", 0.1),
            ("1.5", 1.5),
            ("1.0", 1.0),
            ("100.000", 100.0),
            ("1e2", 100.0),
            ("1E+2", 100.0),
            ("1e300", 1e300),
            ("0.30000000000000004", 0.30000000000000004),
            ("5e-324", 5e-324),
            ("1.7976931348623157e308", f64::MAX),
            ("-2.5e-3", -2.5e-3),
            ("-0.0", -0.0),
            ("0e999", 0.0),
        ] {
            match read(text) {
                DocumentV2::Number(Number::Float(value)) => {
                    assert_eq!(value, expected, "{text}");
                    assert_eq!(
                        value.is_sign_negative(),
                        expected.is_sign_negative(),
                        "{text}"
                    );
                }
                other => panic!("{text}: expected Float, got {other:?}"),
            }
        }
    }

    /// `BigDecimal` limits its exponent, so a number it can't hold is an error. `read_document`
    /// reads this one as infinity.
    #[test]
    fn rejects_an_exponent_big_decimal_cannot_hold() {
        let text = "1e99999999999999999999999999999999999999999";
        let error = read_with(JsonCodecSettings::default(), text).unwrap_err();
        assert!(
            matches!(error, SerdeError::InvalidInput { .. }),
            "{error:?}"
        );
        assert!(read_v1(JsonCodecSettings::default(), text).is_ok());
    }

    /// Every value counts as a level, including scalars, as in `read_document`.
    #[test]
    fn enforces_the_depth_limit_like_read_document() {
        let settings = || JsonCodecSettings::builder().max_depth(3).build();
        for (input, accepted) in [
            ("[[[]]]", true),
            ("[[[[]]]]", false),
            (r#"{"a":{"b":{}}}"#, true),
            (r#"{"a":{"b":{"c":{}}}}"#, false),
            (r#"[{"a":1}]"#, true),
            (r#"[{"a":[1]}]"#, false),
        ] {
            let v2 = read_with(settings(), input);
            assert_eq!(v2.is_ok(), accepted, "{input}: {v2:?}");
            assert_eq!(read_v1(settings(), input).is_ok(), accepted, "{input}");
            if let Err(error) = v2 {
                assert!(
                    error.to_string().contains("maximum nesting depth"),
                    "{error}"
                );
            }
        }
    }

    static DOC_MEMBER: Schema<'static> = Schema::new_member(
        shape_id!("test", "Holder", "doc"),
        ShapeType::Document,
        "doc",
        0,
    );
    static HOLDER: Schema<'static> = Schema::new_struct(
        shape_id!("test", "Holder"),
        ShapeType::Structure,
        &[&DOC_MEMBER],
    );

    #[test]
    fn reads_document_members_of_structures() {
        for (input, expected) in [
            (
                r#"{"doc":{"z":1,"a":18446744073709551616},"other":true}"#,
                Some(DocumentV2::Object(DocumentObject::from([
                    ("z".to_owned(), 1u64.into()),
                    (
                        "a".to_owned(),
                        DocumentV2::BigInteger(
                            BigInteger::from_str("18446744073709551616").unwrap(),
                        ),
                    ),
                ]))),
            ),
            // A null document member is a value, as it is for `read_document`.
            (r#"{"doc":null}"#, Some(DocumentV2::Null)),
            (r#"{"other":null}"#, None),
        ] {
            let mut deser =
                JsonDeserializer::new(input.as_bytes(), Arc::new(JsonCodecSettings::default()));
            let mut actual = None;
            deser
                .read_struct(&HOLDER, &mut |member, reader| {
                    if member.member_index() == Some(0) {
                        actual = Some(reader.read_document_v2(member)?);
                    }
                    Ok(())
                })
                .unwrap();
            assert_eq!(actual, expected, "{input}");
            if let Some(DocumentV2::Object(entries)) = &actual {
                assert_eq!(entries.keys().collect::<Vec<_>>(), ["z", "a"]);
            }
        }
    }

    /// The trait's default would read through `read_document` and round the number.
    #[test]
    fn the_override_is_reached_through_a_trait_object() {
        let mut deser = JsonDeserializer::new(
            b"18446744073709551616",
            Arc::new(JsonCodecSettings::default()),
        );
        let dyn_deser: &mut dyn ShapeDeserializer = &mut deser;
        assert!(matches!(
            dyn_deser.read_document_v2(&DOCUMENT).unwrap(),
            DocumentV2::BigInteger(_)
        ));
    }

    #[test]
    fn round_trips_through_write_document_v2_byte_for_byte() {
        let inputs = [
            r#"{"z":1,"a":[true,null,"s\"q",1.5,-2,-0.0,0.1],"m":{}}"#,
            r#"[18446744073709551616,-9223372036854775809,1e400,1e-400,3.14159265358979323846264338327950288]"#,
            r#"{"b":{"d":[],"c":{"f":1,"e":2}},"a":"x"}"#,
        ];
        for input in inputs {
            let doc = read(input);
            let mut ser = crate::codec::JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
            aws_smithy_schema::serde::ShapeSerializer::write_document_v2(&mut ser, &DOCUMENT, &doc)
                .unwrap();
            assert_eq!(String::from_utf8(ser.finish()).unwrap(), input);
        }
    }

    // --- read_discriminated_document_v2 ------------------------------------------------------

    fn read_discriminated(
        settings: JsonCodecSettings,
        input: &str,
    ) -> DiscriminatedDocument<DocumentV2> {
        JsonDeserializer::new(input.as_bytes(), Arc::new(settings))
            .read_discriminated_document_v2()
            .unwrap_or_else(|e| panic!("{input}: {e:?}"))
    }

    fn with_namespace(namespace: &str) -> JsonCodecSettings {
        JsonCodecSettings::builder()
            .default_namespace(namespace)
            .build()
    }

    #[test]
    fn lifts_an_absolute_type_and_keeps_the_other_members_in_order() {
        let wrapper = read_discriminated(
            JsonCodecSettings::default(),
            r#"{"name":"Iago","__type":"smithy.example#Bird","age":3}"#,
        );
        assert_eq!(wrapper.discriminator(), Some("smithy.example#Bird"));
        assert_eq!(keys(wrapper.document()), ["name", "age"]);
        assert!(wrapper.settings().is_some());

        // An absolute ID wins over the default namespace.
        let wrapper = read_discriminated(
            with_namespace("com.example"),
            r#"{"__type":"smithy.example#Bird"}"#,
        );
        assert_eq!(wrapper.discriminator(), Some("smithy.example#Bird"));
    }

    #[test]
    fn lifts_a_relative_type_only_with_a_default_namespace() {
        let input = r#"{"__type":"Capacity","units":1.0}"#;
        let wrapper = read_discriminated(JsonCodecSettings::default(), input);
        assert_eq!(wrapper.discriminator(), None);
        assert_eq!(keys(wrapper.document()), ["__type", "units"]);

        let wrapper = read_discriminated(with_namespace("com.amazonaws.dynamodb"), input);
        assert_eq!(
            wrapper.discriminator(),
            Some("com.amazonaws.dynamodb#Capacity")
        );
        assert_eq!(keys(wrapper.document()), ["units"]);
    }

    #[test]
    fn leaves_a_type_it_cannot_lift_in_the_object() {
        let cases = [
            // Rejected by `read_discriminated_document` too.
            (JsonCodecSettings::default(), r#""smithy.example#Foo$bar""#),
            (JsonCodecSettings::default(), r#""smithy.example#""#),
            (JsonCodecSettings::default(), r##""#smithy.example""##),
            (JsonCodecSettings::default(), r##""no#hash#twice""##),
            (JsonCodecSettings::default(), "42"),
            (JsonCodecSettings::default(), "null"),
            (JsonCodecSettings::default(), r#"{"nested":true}"#),
            (with_namespace("com.example"), r#""foo bar""#),
            (with_namespace("com.example"), r#""""#),
            // Lifted by `read_discriminated_document`, but not valid Smithy shape IDs.
            (JsonCodecSettings::default(), r#""my-namespace#Bird""#),
            (JsonCodecSettings::default(), r#""smithy.example#1Bird""#),
            (with_namespace("my-namespace"), r#""Bird""#),
        ];
        for (settings, value) in cases {
            let input = format!(r#"{{"__type":{value},"a":1}}"#);
            let wrapper = read_discriminated(settings, &input);
            assert_eq!(wrapper.discriminator(), None, "{input}");
            assert_eq!(keys(wrapper.document()), ["__type", "a"], "{input}");
        }

        // Only the top level has a discriminator slot.
        let wrapper = read_discriminated(
            JsonCodecSettings::default(),
            r#"{"outer":{"__type":"smithy.example#Inner"}}"#,
        );
        assert_eq!(wrapper.discriminator(), None);
        assert_eq!(
            keys(&wrapper.document().as_object().unwrap()["outer"]),
            ["__type"]
        );
    }

    #[test]
    fn attached_settings_coerce_strings_and_numbers() {
        let wrapper = read_discriminated(JsonCodecSettings::default(), r#""aGVsbG8=""#);
        assert_eq!(&*wrapper.as_blob().unwrap(), b"hello");

        let wrapper = read_discriminated(JsonCodecSettings::default(), "1577836800");
        assert_eq!(wrapper.as_timestamp().unwrap().secs(), 1_577_836_800);
    }

    #[test]
    fn round_trips_through_write_discriminated_document_v2() {
        let input = r#"{"__type":"smithy.example#Bird","name":"Iago","span":1e400}"#;
        let wrapper = read_discriminated(JsonCodecSettings::default(), input);
        let mut ser = crate::codec::JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_discriminated_document_v2(&DOCUMENT, &wrapper)
            .unwrap();
        assert_eq!(String::from_utf8(ser.finish()).unwrap(), input);
    }
}

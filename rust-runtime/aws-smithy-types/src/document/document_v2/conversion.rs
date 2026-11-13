/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Conversions between [`Document`] and [`DocumentV2`].
//!
//! `DocumentV2` can hold every `Document`, so the upgrade is a `From` implementation. `Document`
//! has no variant for blobs, timestamps, or arbitrary-precision numbers, so the downgrades are
//! named methods: [`DocumentV2::to_document_lossy`] encodes those values, and
//! [`DocumentV2::try_to_document`] returns an error for them.
//!
//! Each conversion matches every variant explicitly, so a new `DocumentV2` variant doesn't
//! compile until both downgrades handle it.

use super::{signed_number, variant_name, DocumentV2};
use crate::document::DocumentError;
use crate::{DateTime, Document, Number};
use std::collections::HashMap;
use std::fmt;

impl From<Document> for DocumentV2 {
    /// Converts a [`Document`] to a `DocumentV2` without losing information.
    ///
    /// Each variant converts to the `DocumentV2` variant with the same name, recursively.
    /// Numbers are copied unchanged, including a non-negative [`Number::NegInt`] from
    /// `Document`'s `From<i64>`. An object's entries keep the `HashMap`'s iteration order, which
    /// is unspecified.
    fn from(document: Document) -> Self {
        match document {
            Document::Object(map) => DocumentV2::Object(
                map.into_iter()
                    .map(|(key, value)| (key, DocumentV2::from(value)))
                    .collect(),
            ),
            Document::Array(values) => {
                DocumentV2::Array(values.into_iter().map(DocumentV2::from).collect())
            }
            Document::Number(number) => DocumentV2::Number(number),
            Document::String(string) => DocumentV2::String(string),
            Document::Bool(boolean) => DocumentV2::Bool(boolean),
            Document::Null => DocumentV2::Null,
        }
    }
}

impl From<&Document> for DocumentV2 {
    /// Copies a [`Document`] into a `DocumentV2`. The result is the same as
    /// `DocumentV2::from(document.clone())`.
    fn from(document: &Document) -> Self {
        match document {
            Document::Object(map) => DocumentV2::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), DocumentV2::from(value)))
                    .collect(),
            ),
            Document::Array(values) => {
                DocumentV2::Array(values.iter().map(DocumentV2::from).collect())
            }
            Document::Number(number) => DocumentV2::Number(*number),
            Document::String(string) => DocumentV2::String(string.clone()),
            Document::Bool(boolean) => DocumentV2::Bool(*boolean),
            Document::Null => DocumentV2::Null,
        }
    }
}

impl DocumentV2 {
    /// Converts this `DocumentV2` to a [`Document`], encoding the variants that `Document`
    /// lacks.
    ///
    /// This always succeeds. Each JSON-shaped variant converts to the `Document` variant with
    /// the same name, recursively, and the other variants convert as follows:
    ///
    /// | `DocumentV2` | `Document` |
    /// |---|---|
    /// | `Blob` | `String` of the bytes in standard base64, with padding |
    /// | `Timestamp` | `Number` of seconds since the Unix epoch: `PosInt` or `NegInt` for a whole second, and `Float` otherwise |
    /// | `BigInteger`, `BigDecimal` | `Number` read from the number's text: `PosInt` or `NegInt` for an integer that fits in 64 bits, and the nearest `f64` otherwise |
    ///
    /// Each result is the `Document` that `Document`'s JSON parsers read from the value's
    /// default JSON form: base64 text for a blob, epoch seconds for a timestamp, and the number
    /// itself for a big number.
    ///
    /// Without settings, [`DiscriminatedDocument::as_blob`](crate::DiscriminatedDocument::as_blob)
    /// and [`DiscriminatedDocument::as_timestamp`](crate::DiscriminatedDocument::as_timestamp)
    /// read the blob and timestamp encodings back.
    ///
    /// Converting the result back with `From` doesn't always return the original:
    ///
    /// - An encoded value can't be told apart from a string or number with the same text or
    ///   value.
    /// - A fractional timestamp becomes an `f64`, which can lose sub-microsecond precision.
    /// - A big number becomes an `f64` when it doesn't fit in 64 bits or has a fraction or an
    ///   exponent. That rounds digits that an `f64` can't hold, and a magnitude beyond the `f64`
    ///   range becomes infinity.
    /// - Objects lose their entry order, because [`Document::Object`] is a `HashMap`.
    ///
    /// The encodings don't depend on a protocol, and a protocol may encode the same values
    /// differently on the wire. To get an error instead of an encoding, use
    /// [`DocumentV2::try_to_document`].
    ///
    /// # Example
    ///
    /// ```
    /// use aws_smithy_types::{BigDecimal, DateTime, Document, DocumentV2, Number};
    /// use std::str::FromStr;
    ///
    /// let blob = DocumentV2::Blob(b"hello".to_vec());
    /// assert_eq!(blob.to_document_lossy(), Document::String("aGVsbG8=".to_owned()));
    ///
    /// let timestamp = DocumentV2::Timestamp(DateTime::from_secs(1_700_000_000));
    /// assert_eq!(
    ///     timestamp.to_document_lossy(),
    ///     Document::Number(Number::PosInt(1_700_000_000)),
    /// );
    ///
    /// let decimal = DocumentV2::BigDecimal(BigDecimal::from_str("0.1234567890123456789").unwrap());
    /// assert_eq!(
    ///     decimal.to_document_lossy(),
    ///     Document::Number(Number::Float(0.12345678901234568)),
    /// );
    /// ```
    pub fn to_document_lossy(&self) -> Document {
        match self {
            DocumentV2::Object(object) => Document::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_document_lossy()))
                    .collect(),
            ),
            DocumentV2::Array(values) => {
                Document::Array(values.iter().map(DocumentV2::to_document_lossy).collect())
            }
            DocumentV2::Number(number) => Document::Number(*number),
            DocumentV2::String(string) => Document::String(string.clone()),
            DocumentV2::Bool(boolean) => Document::Bool(*boolean),
            DocumentV2::Null => Document::Null,
            DocumentV2::Blob(bytes) => Document::String(crate::base64::encode(bytes)),
            DocumentV2::Timestamp(timestamp) => Document::Number(epoch_seconds(*timestamp)),
            DocumentV2::BigInteger(value) => number_from_text(value.as_ref()),
            DocumentV2::BigDecimal(value) => number_from_text(value.as_ref()),
        }
    }

    /// Converts this `DocumentV2` to a [`Document`], or returns an error if `Document` can't
    /// represent it.
    ///
    /// Returns [`DocumentError::TypeMismatch`] if the tree contains a [`DocumentV2::Blob`],
    /// [`DocumentV2::Timestamp`], [`DocumentV2::BigInteger`], or [`DocumentV2::BigDecimal`].
    /// The error message names the first such value in depth-first order and gives its path,
    /// such as `$["items"][2]`. Otherwise, the result is the same as
    /// [`DocumentV2::to_document_lossy`]'s.
    ///
    /// Objects still lose their entry order, because [`Document::Object`] is a `HashMap`.
    ///
    /// # Example
    ///
    /// ```
    /// use aws_smithy_types::{Document, DocumentV2};
    ///
    /// let string = DocumentV2::String("hello".to_owned());
    /// assert_eq!(string.try_to_document().unwrap(), Document::String("hello".to_owned()));
    ///
    /// let blob = DocumentV2::Array(vec![DocumentV2::Blob(b"hello".to_vec())]);
    /// let error = blob.try_to_document().unwrap_err();
    /// assert!(error.to_string().contains("blob at $[0]"));
    /// ```
    pub fn try_to_document(&self) -> Result<Document, DocumentError> {
        try_downgrade(self).map_err(|e| DocumentError::type_mismatch(e.to_string()))
    }
}

/// Encodes `timestamp` as seconds since the Unix epoch: an integer for a whole second, and a
/// float otherwise.
fn epoch_seconds(timestamp: DateTime) -> Number {
    if timestamp.has_subsec_nanos() {
        Number::Float(timestamp.as_secs_f64())
    } else {
        signed_number(timestamp.secs())
    }
}

/// Reads the text of a [`BigInteger`](crate::BigInteger) or [`BigDecimal`](crate::BigDecimal)
/// as `Document`'s JSON parsers read a JSON number. Text without a fraction or an exponent
/// becomes `PosInt` or `NegInt` when it fits, and everything else becomes the nearest `f64`.
///
/// Zero is `PosInt`, as in `DocumentV2`'s `From<i64>`, even when the text is `-0`. Both types
/// only hold text that `f64` parses, so the `String` fallback keeps the conversion total without
/// being reachable.
fn number_from_text(text: &str) -> Document {
    if !text.contains(['.', 'e', 'E']) {
        if let Ok(value) = text.parse::<u64>() {
            return Document::Number(Number::PosInt(value));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Document::Number(signed_number(value));
        }
    }
    match text.parse::<f64>() {
        Ok(value) => Document::Number(Number::Float(value)),
        Err(_) => Document::String(text.to_owned()),
    }
}

/// The work of [`DocumentV2::try_to_document`], keeping the path to a failure unformatted
/// until the end.
fn try_downgrade(document: &DocumentV2) -> Result<Document, Unrepresentable<'_>> {
    Ok(match document {
        DocumentV2::Object(object) => {
            let mut map = HashMap::with_capacity(object.len());
            for (key, value) in object {
                let value = try_downgrade(value).map_err(|e| e.within(Segment::Key(key)))?;
                map.insert(key.clone(), value);
            }
            Document::Object(map)
        }
        DocumentV2::Array(values) => Document::Array(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    try_downgrade(value).map_err(|e| e.within(Segment::Index(index)))
                })
                .collect::<Result<_, _>>()?,
        ),
        DocumentV2::Number(number) => Document::Number(*number),
        DocumentV2::String(string) => Document::String(string.clone()),
        DocumentV2::Bool(boolean) => Document::Bool(*boolean),
        DocumentV2::Null => Document::Null,
        DocumentV2::Blob(_)
        | DocumentV2::Timestamp(_)
        | DocumentV2::BigInteger(_)
        | DocumentV2::BigDecimal(_) => {
            return Err(Unrepresentable {
                variant: variant_name(document),
                path_from_value: Vec::new(),
            })
        }
    })
}

/// A value that [`Document`] can't represent, and where it is in the tree.
struct Unrepresentable<'a> {
    variant: &'static str,
    /// The path from the value up to the root: the innermost segment comes first.
    path_from_value: Vec<Segment<'a>>,
}

enum Segment<'a> {
    Index(usize),
    Key(&'a str),
}

impl<'a> Unrepresentable<'a> {
    /// Records that the failing value is inside `segment` of the next container up.
    fn within(mut self, segment: Segment<'a>) -> Self {
        self.path_from_value.push(segment);
        self
    }
}

impl fmt::Display for Unrepresentable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Document can't represent the {} at $", self.variant)?;
        for segment in self.path_from_value.iter().rev() {
            match segment {
                Segment::Index(index) => write!(f, "[{index}]")?,
                // `Debug` quotes the key and escapes quotes inside it.
                Segment::Key(key) => write!(f, "[{key:?}]")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::document::DocumentObject;
    use crate::{
        BigDecimal, BigInteger, DateTime, DiscriminatedDocument, Document, DocumentError,
        DocumentV2, Number,
    };
    use std::collections::HashMap;
    use std::str::FromStr;

    fn big_integer(text: &str) -> DocumentV2 {
        DocumentV2::BigInteger(BigInteger::from_str(text).unwrap())
    }

    fn big_decimal(text: &str) -> DocumentV2 {
        DocumentV2::BigDecimal(BigDecimal::from_str(text).unwrap())
    }

    fn string(text: &str) -> Document {
        Document::String(text.to_owned())
    }

    /// A `Document` for every variant, with a nested array and object, and numbers that
    /// `DocumentV2`'s own `From` conversions wouldn't produce.
    fn every_v1_variant() -> Vec<Document> {
        vec![
            Document::Null,
            Document::Bool(true),
            Document::Bool(false),
            Document::Number(Number::PosInt(u64::MAX)),
            Document::Number(Number::NegInt(i64::MIN)),
            // `Document::from(5i64)` produces this non-canonical value.
            Document::Number(Number::NegInt(5)),
            Document::Number(Number::Float(-0.5)),
            Document::Number(Number::Float(f64::INFINITY)),
            string(""),
            string("text"),
            Document::Array(Vec::new()),
            Document::Array(vec![Document::Null, string("a")]),
            Document::Object(HashMap::new()),
            Document::Object(HashMap::from([
                ("a".to_owned(), Document::Bool(true)),
                (
                    "b".to_owned(),
                    Document::Array(vec![Document::Object(HashMap::from([(
                        "c".to_owned(),
                        Document::Null,
                    )]))]),
                ),
            ])),
        ]
    }

    /// Wraps `leaf` in `depth` containers, alternating between a one-element array and a
    /// one-entry object with key `"k"`, starting with an array innermost.
    fn nested_v1(depth: usize, leaf: Document) -> Document {
        (0..depth).fold(leaf, |inner, level| {
            if level % 2 == 0 {
                Document::Array(vec![inner])
            } else {
                Document::Object(HashMap::from([("k".to_owned(), inner)]))
            }
        })
    }

    /// The `DocumentV2` equivalent of [`nested_v1`].
    fn nested_v2(depth: usize, leaf: DocumentV2) -> DocumentV2 {
        (0..depth).fold(leaf, |inner, level| {
            if level % 2 == 0 {
                DocumentV2::Array(vec![inner])
            } else {
                DocumentV2::Object(DocumentObject::from([("k".to_owned(), inner)]))
            }
        })
    }

    const DEPTH: usize = 128;

    fn type_mismatch_message(result: Result<Document, DocumentError>) -> String {
        match result {
            Err(DocumentError::TypeMismatch { message }) => message,
            other => panic!("expected a type mismatch, got {other:?}"),
        }
    }

    #[test]
    fn upgrade_maps_each_variant_to_its_namesake() {
        assert_eq!(DocumentV2::from(Document::Null), DocumentV2::Null);
        assert_eq!(
            DocumentV2::from(Document::Bool(true)),
            DocumentV2::Bool(true)
        );
        assert_eq!(
            DocumentV2::from(Document::Number(Number::Float(1.5))),
            DocumentV2::Number(Number::Float(1.5))
        );
        assert_eq!(
            DocumentV2::from(string("s")),
            DocumentV2::String("s".to_owned())
        );
        assert_eq!(
            DocumentV2::from(Document::Array(vec![Document::Null])),
            DocumentV2::Array(vec![DocumentV2::Null])
        );
        assert_eq!(
            DocumentV2::from(Document::Object(HashMap::from([(
                "k".to_owned(),
                Document::Null
            )]))),
            DocumentV2::Object(DocumentObject::from([("k".to_owned(), DocumentV2::Null)]))
        );
    }

    #[test]
    fn upgrade_copies_numbers_unchanged() {
        // Unlike `DocumentV2::from(5i64)`, the upgrade doesn't canonicalize `NegInt(5)`.
        assert_eq!(
            DocumentV2::from(Document::from(5i64)),
            DocumentV2::Number(Number::NegInt(5))
        );
        assert_eq!(
            DocumentV2::from(5i64),
            DocumentV2::Number(Number::PosInt(5))
        );

        let nan = DocumentV2::from(Document::Number(Number::Float(f64::NAN)));
        assert!(matches!(nan, DocumentV2::Number(Number::Float(value)) if value.is_nan()));
    }

    #[test]
    fn owned_and_borrowed_upgrades_agree() {
        for document in every_v1_variant() {
            assert_eq!(
                DocumentV2::from(&document),
                DocumentV2::from(document.clone())
            );
        }
    }

    #[test]
    fn upgrade_handles_deep_nesting() {
        let document = nested_v1(DEPTH, string("leaf"));
        let expected = nested_v2(DEPTH, DocumentV2::String("leaf".to_owned()));
        assert_eq!(DocumentV2::from(&document), expected);
        assert_eq!(DocumentV2::from(document), expected);
    }

    #[test]
    fn every_v1_document_survives_a_round_trip() {
        for document in every_v1_variant() {
            let upgraded = DocumentV2::from(&document);
            assert_eq!(upgraded.to_document_lossy(), document);
            assert_eq!(upgraded.try_to_document().unwrap(), document);
        }
        let deep = nested_v1(DEPTH, Document::Null);
        assert_eq!(DocumentV2::from(&deep).try_to_document().unwrap(), deep);
    }

    #[test]
    fn lossy_downgrade_keeps_json_shaped_variants() {
        assert_eq!(DocumentV2::Null.to_document_lossy(), Document::Null);
        assert_eq!(
            DocumentV2::Bool(false).to_document_lossy(),
            Document::Bool(false)
        );
        assert_eq!(
            DocumentV2::Number(Number::PosInt(7)).to_document_lossy(),
            Document::Number(Number::PosInt(7))
        );
        assert_eq!(
            DocumentV2::String("s".to_owned()).to_document_lossy(),
            string("s")
        );
    }

    #[test]
    fn lossy_downgrade_encodes_blobs_as_standard_base64() {
        assert_eq!(DocumentV2::Blob(Vec::new()).to_document_lossy(), string(""));
        // `/` and `=` distinguish standard base64 with padding from the URL-safe alphabet.
        assert_eq!(
            DocumentV2::Blob(vec![0xff]).to_document_lossy(),
            string("/w==")
        );
        assert_eq!(
            DocumentV2::Blob(b"hello".to_vec()).to_document_lossy(),
            string("aGVsbG8=")
        );
    }

    #[test]
    fn lossy_downgrade_encodes_timestamps_as_epoch_seconds() {
        let cases = [
            (DateTime::from_secs(0), Number::PosInt(0)),
            (
                DateTime::from_secs(1_700_000_000),
                Number::PosInt(1_700_000_000),
            ),
            (DateTime::from_secs(-1), Number::NegInt(-1)),
            (DateTime::from_secs(i64::MIN), Number::NegInt(i64::MIN)),
            (
                DateTime::from_secs(i64::MAX),
                Number::PosInt(i64::MAX as u64),
            ),
            (
                DateTime::from_secs_and_nanos(1, 500_000_000),
                Number::Float(1.5),
            ),
            // -1.5 seconds is stored as -2 seconds plus 0.5 seconds.
            (DateTime::from_secs_f64(-1.5), Number::Float(-1.5)),
            (
                DateTime::from_secs_and_nanos(-1, 750_000_000),
                Number::Float(-0.25),
            ),
        ];
        for (timestamp, expected) in cases {
            assert_eq!(
                DocumentV2::Timestamp(timestamp).to_document_lossy(),
                Document::Number(expected),
                "{timestamp:?}"
            );
        }
    }

    #[test]
    fn lossy_downgrade_reads_big_numbers_as_numbers() {
        let cases = [
            // Integers that fit in 64 bits, with the zero and sign rules of `From<i64>`.
            (
                big_integer("18446744073709551615"),
                Number::PosInt(u64::MAX),
            ),
            (
                big_integer("-9223372036854775808"),
                Number::NegInt(i64::MIN),
            ),
            (big_integer("00123"), Number::PosInt(123)),
            (big_integer("-0"), Number::PosInt(0)),
            (big_decimal("100"), Number::PosInt(100)),
            (big_decimal("+5"), Number::PosInt(5)),
            (big_decimal("-7"), Number::NegInt(-7)),
            // Integers beyond 64 bits, one past `u64::MAX` and one past `i64::MIN`.
            (
                big_integer("18446744073709551616"),
                Number::Float(18_446_744_073_709_551_616.0),
            ),
            (
                big_integer("-9223372036854775809"),
                Number::Float(-9_223_372_036_854_775_808.0),
            ),
            // Text with a fraction or an exponent, which `f64` rounds.
            (
                big_decimal("0.1234567890123456789"),
                Number::Float(0.123_456_789_012_345_68),
            ),
            (big_decimal("1.5e300"), Number::Float(1.5e300)),
            (big_decimal("1e2"), Number::Float(100.0)),
            (big_decimal("+.5"), Number::Float(0.5)),
            (big_decimal("5."), Number::Float(5.0)),
            (big_decimal("1e400"), Number::Float(f64::INFINITY)),
            (big_decimal("-1e400"), Number::Float(f64::NEG_INFINITY)),
            (big_decimal("1e-400"), Number::Float(0.0)),
        ];
        for (document, expected) in cases {
            assert_eq!(
                document.to_document_lossy(),
                Document::Number(expected),
                "{document:?}"
            );
        }
        let negative_zero = big_decimal("-0.000E-10").to_document_lossy();
        assert!(
            matches!(negative_zero, Document::Number(Number::Float(value)) if value == 0.0 && value.is_sign_negative()),
            "{negative_zero:?}"
        );
    }

    #[test]
    fn lossy_downgrade_keeps_object_entries_but_not_their_order() {
        let object = DocumentObject::from([
            ("z".to_owned(), DocumentV2::Null),
            ("a".to_owned(), DocumentV2::Blob(b"x".to_vec())),
            ("m".to_owned(), DocumentV2::Bool(true)),
        ]);
        let expected = HashMap::from([
            ("z".to_owned(), Document::Null),
            ("a".to_owned(), string("eA==")),
            ("m".to_owned(), Document::Bool(true)),
        ]);
        let downgraded = DocumentV2::Object(object).to_document_lossy();
        assert_eq!(downgraded, Document::Object(expected));
    }

    #[test]
    fn lossy_downgrade_encodes_nested_values() {
        let document = DocumentV2::Array(vec![
            DocumentV2::Object(DocumentObject::from([
                (
                    "when".to_owned(),
                    DocumentV2::Timestamp(DateTime::from_secs(10)),
                ),
                ("size".to_owned(), big_integer("99999999999999999999")),
            ])),
            DocumentV2::Array(vec![big_decimal("0.1")]),
        ]);
        let expected = Document::Array(vec![
            Document::Object(HashMap::from([
                ("when".to_owned(), Document::Number(Number::PosInt(10))),
                ("size".to_owned(), Document::Number(Number::Float(1e20))),
            ])),
            Document::Array(vec![Document::Number(Number::Float(0.1))]),
        ]);
        assert_eq!(document.to_document_lossy(), expected);

        let deep = nested_v2(DEPTH, DocumentV2::Blob(b"x".to_vec()));
        assert_eq!(deep.to_document_lossy(), nested_v1(DEPTH, string("eA==")));
    }

    #[test]
    fn the_v1_wrapper_reads_blob_and_timestamp_encodings_back() {
        let bytes = vec![0, 1, 0xfe, 0xff];
        let encoded = DocumentV2::Blob(bytes.clone()).to_document_lossy();
        let wrapper = DiscriminatedDocument::new(encoded);
        assert_eq!(wrapper.as_blob().unwrap().as_ref(), bytes.as_slice());

        for timestamp in [
            DateTime::from_secs(1_700_000_000),
            DateTime::from_secs(-86_400),
            DateTime::from_secs_and_nanos(1_700_000_000, 500_000_000),
        ] {
            let encoded = DocumentV2::Timestamp(timestamp).to_document_lossy();
            let wrapper = DiscriminatedDocument::new(encoded);
            assert_eq!(wrapper.as_timestamp().unwrap(), timestamp);
        }
    }

    #[test]
    fn try_to_document_rejects_each_v2_only_variant() {
        let cases = [
            (DocumentV2::Blob(Vec::new()), "blob"),
            (DocumentV2::Timestamp(DateTime::from_secs(0)), "timestamp"),
            (big_integer("1"), "bigInteger"),
            (big_decimal("1.5"), "bigDecimal"),
        ];
        for (document, variant) in cases {
            assert_eq!(
                type_mismatch_message(document.try_to_document()),
                format!("Document can't represent the {variant} at $")
            );
        }
    }

    #[test]
    fn try_to_document_reports_the_path_to_the_value() {
        let document = DocumentV2::Object(DocumentObject::from([(
            "items".to_owned(),
            DocumentV2::Array(vec![
                DocumentV2::Null,
                DocumentV2::Null,
                DocumentV2::Object(DocumentObject::from([(
                    "say \"hi\"".to_owned(),
                    DocumentV2::Blob(Vec::new()),
                )])),
            ]),
        )]));
        assert_eq!(
            type_mismatch_message(document.try_to_document()),
            r#"Document can't represent the blob at $["items"][2]["say \"hi\""]"#
        );

        let deep = nested_v2(DEPTH, big_decimal("1.5"));
        let expected_path: String = (0..DEPTH)
            .rev()
            .map(|level| if level % 2 == 0 { "[0]" } else { r#"["k"]"# })
            .collect();
        assert_eq!(
            type_mismatch_message(deep.try_to_document()),
            format!("Document can't represent the bigDecimal at ${expected_path}")
        );
    }

    #[test]
    fn try_to_document_reports_the_first_value_in_depth_first_order() {
        let document = DocumentV2::Object(DocumentObject::from([
            ("first".to_owned(), DocumentV2::Null),
            (
                "second".to_owned(),
                DocumentV2::Array(vec![
                    DocumentV2::Null,
                    DocumentV2::Timestamp(DateTime::from_secs(0)),
                ]),
            ),
            ("third".to_owned(), DocumentV2::Blob(Vec::new())),
        ]));
        assert_eq!(
            type_mismatch_message(document.try_to_document()),
            r#"Document can't represent the timestamp at $["second"][1]"#
        );
    }

    #[test]
    fn try_to_document_matches_the_lossy_downgrade_when_it_succeeds() {
        let document = DocumentV2::Object(DocumentObject::from([
            ("a".to_owned(), DocumentV2::from(-3i64)),
            (
                "b".to_owned(),
                DocumentV2::Array(vec![DocumentV2::from("s"), DocumentV2::Null]),
            ),
        ]));
        assert_eq!(
            document.try_to_document().unwrap(),
            document.to_document_lossy()
        );
    }

    #[test]
    fn the_upgrade_composes_with_option() {
        // `DocumentV2`'s `From<Option<T>>` picks up the new conversions.
        assert_eq!(DocumentV2::from(None::<Document>), DocumentV2::Null);
        assert_eq!(
            DocumentV2::from(Some(&Document::Bool(true))),
            DocumentV2::Bool(true)
        );
    }

    #[test]
    fn upgraded_documents_work_in_the_v2_wrapper() {
        let wrapper = DiscriminatedDocument::new_v2(DocumentV2::from(string("aGk=")));
        assert_eq!(wrapper.as_blob().unwrap().as_ref(), b"hi");
    }
}

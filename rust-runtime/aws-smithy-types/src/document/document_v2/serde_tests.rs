/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tests for `DocumentV2`'s serde support. They need both serde features.

use super::de::cautious_capacity;
use crate::document::{from_document_v2, to_document, to_document_v2, DocumentObject};
use crate::{BigDecimal, BigInteger, Blob, DateTime, Document, DocumentV2, Number};
use serde::de::{DeserializeOwned, IntoDeserializer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;

/// Asserts that `to_document_v2` gives the upgrade of what `to_document` gives, and that
/// `from_document_v2` reads the value back.
fn assert_matches_v1_and_round_trips<T>(value: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let v2 = to_document_v2(&value).unwrap();
    let v1 = to_document(&value).unwrap();
    assert_eq!(v2, DocumentV2::from(v1), "to_document_v2({value:?})");
    let read: T = from_document_v2(v2).unwrap();
    assert_eq!(read, value);
}

fn round_trip<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned,
{
    from_document_v2(to_document_v2(value).unwrap()).unwrap()
}

fn object(entries: &[(&str, DocumentV2)]) -> DocumentV2 {
    DocumentV2::Object(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

fn keys(document: &DocumentV2) -> Vec<&str> {
    document
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

fn big_integer(text: &str) -> DocumentV2 {
    DocumentV2::BigInteger(BigInteger::from_str(text).unwrap())
}

fn big_decimal(text: &str) -> DocumentV2 {
    DocumentV2::BigDecimal(BigDecimal::from_str(text).unwrap())
}

/// Serializes and deserializes as bytes, like `serde_bytes::ByteBuf`.
#[derive(Debug, PartialEq)]
struct ByteBuf(Vec<u8>);

impl Serialize for ByteBuf {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for ByteBuf {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = ByteBuf;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<ByteBuf, E> {
                Ok(ByteBuf(value))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Animal {
    Dog,
    Frog(String, Vec<isize>),
    Cat { age: usize, name: String },
    AntHive(Vec<String>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Inner {
    a: (),
    b: usize,
    c: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Outer {
    inner: Vec<Inner>,
    optional: Option<u32>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Newtype(u32);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Unit;

// -- to_document_v2 and from_document_v2 --------------------------------------------------

#[test]
fn values_that_document_supports_convert_the_same_way() {
    assert_matches_v1_and_round_trips(());
    assert_matches_v1_and_round_trips(true);
    assert_matches_v1_and_round_trips(u8::MAX);
    assert_matches_v1_and_round_trips(u16::MAX);
    assert_matches_v1_and_round_trips(u32::MAX);
    assert_matches_v1_and_round_trips(u64::MAX);
    assert_matches_v1_and_round_trips(i8::MIN);
    assert_matches_v1_and_round_trips(i16::MIN);
    assert_matches_v1_and_round_trips(i32::MIN);
    assert_matches_v1_and_round_trips(i64::MIN);
    assert_matches_v1_and_round_trips(i64::MAX);
    assert_matches_v1_and_round_trips(0i64);
    assert_matches_v1_and_round_trips(-1.5f32);
    assert_matches_v1_and_round_trips(f64::MAX);
    assert_matches_v1_and_round_trips(f64::NEG_INFINITY);
    assert_matches_v1_and_round_trips('\u{1F600}');
    assert_matches_v1_and_round_trips("with\nnewline".to_owned());
    assert_matches_v1_and_round_trips(None::<String>);
    assert_matches_v1_and_round_trips(Some(5u32));
    assert_matches_v1_and_round_trips(Vec::<i32>::new());
    assert_matches_v1_and_round_trips(vec![vec![1u32, 2], vec![], vec![3]]);
    // A `Vec<u8>` serializes as a sequence of numbers, not as bytes.
    assert_matches_v1_and_round_trips(vec![1u8, 2, 3]);
    assert_matches_v1_and_round_trips((1i32, "hello".to_owned(), true));
    assert_matches_v1_and_round_trips(HashMap::from([("a".to_owned(), 1u32)]));
    assert_matches_v1_and_round_trips(HashMap::from([(1u32, "one".to_owned())]));
    assert_matches_v1_and_round_trips(HashMap::from([(
        "outer".to_owned(),
        HashMap::from([("inner".to_owned(), 10u32)]),
    )]));
    assert_matches_v1_and_round_trips(Outer {
        inner: vec![Inner {
            a: (),
            b: 2,
            c: vec!["abc".to_owned(), "xyz".to_owned()],
        }],
        optional: None,
    });
    assert_matches_v1_and_round_trips(Newtype(123));
    assert_matches_v1_and_round_trips(Unit);
    assert_matches_v1_and_round_trips(Animal::Dog);
    assert_matches_v1_and_round_trips(Animal::Frog("Henry".to_owned(), vec![349, -102]));
    assert_matches_v1_and_round_trips(Animal::Cat {
        age: 5,
        name: "Kate".to_owned(),
    });
    assert_matches_v1_and_round_trips(Animal::AntHive(vec!["Bob".to_owned()]));

    let nan = to_document_v2(&f64::NAN).unwrap();
    assert!(matches!(nan, DocumentV2::Number(Number::Float(value)) if value.is_nan()));
}

#[test]
fn objects_keep_the_order_of_serialized_fields() {
    #[derive(Serialize)]
    struct Fields {
        zebra: u8,
        apple: u8,
        mango: u8,
    }
    let document = to_document_v2(&Fields {
        zebra: 1,
        apple: 2,
        mango: 3,
    })
    .unwrap();
    assert_eq!(keys(&document), ["zebra", "apple", "mango"]);

    let cat = to_document_v2(&Animal::Cat {
        age: 1,
        name: "Kate".to_owned(),
    })
    .unwrap();
    assert_eq!(keys(&cat.as_object().unwrap()["Cat"]), ["age", "name"]);
}

#[test]
fn values_serialized_as_bytes_become_blobs() {
    let document = to_document_v2(&ByteBuf(vec![0, 0xff])).unwrap();
    assert_eq!(document, DocumentV2::Blob(vec![0, 0xff]));
    assert_eq!(round_trip(&ByteBuf(Vec::new())), ByteBuf(Vec::new()));

    // `Document` has no blob, so `to_document` produces an array of numbers instead.
    assert!(to_document(&ByteBuf(vec![1])).unwrap().is_array());
}

#[test]
fn wide_integers_become_numbers_when_they_fit_and_big_integers_otherwise() {
    let cases = [
        (
            to_document_v2(&5i128),
            DocumentV2::Number(Number::PosInt(5)),
        ),
        (
            to_document_v2(&-5i128),
            DocumentV2::Number(Number::NegInt(-5)),
        ),
        (
            to_document_v2(&u128::from(u64::MAX)),
            DocumentV2::Number(Number::PosInt(u64::MAX)),
        ),
        (
            to_document_v2(&(u128::from(u64::MAX) + 1)),
            big_integer("18446744073709551616"),
        ),
        (
            to_document_v2(&(i128::from(i64::MIN) - 1)),
            big_integer("-9223372036854775809"),
        ),
        (
            to_document_v2(&u128::MAX),
            big_integer("340282366920938463463374607431768211455"),
        ),
    ];
    for (actual, expected) in cases {
        assert_eq!(actual.unwrap(), expected);
    }

    for value in [i128::MIN, -1, 0, i128::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [0, u128::from(u64::MAX) + 1, u128::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    // `Document` doesn't support 128-bit integers.
    assert!(to_document(&1i128).is_err());
}

#[test]
fn smithy_types_serialize_to_strings_and_read_back() {
    let blob = Blob::new(b"hello".to_vec());
    assert_eq!(
        to_document_v2(&blob).unwrap(),
        DocumentV2::String("aGVsbG8=".to_owned())
    );
    assert_eq!(round_trip(&blob), blob);

    let timestamp = DateTime::from_secs_and_nanos(1_576_540_098, 500_000_000);
    assert_eq!(
        to_document_v2(&timestamp).unwrap(),
        DocumentV2::String("2019-12-16T23:48:18.5Z".to_owned())
    );
    assert_eq!(round_trip(&timestamp), timestamp);

    let integer = BigInteger::from_str("123456789012345678901234567890").unwrap();
    assert_eq!(
        to_document_v2(&integer).unwrap(),
        DocumentV2::String(integer.as_ref().to_owned())
    );
    assert_eq!(round_trip(&integer), integer);

    let decimal = BigDecimal::from_str("1.5e300").unwrap();
    assert_eq!(round_trip(&decimal), decimal);
}

#[test]
fn native_variants_deserialize_into_the_matching_types() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Native {
        blob: Blob,
        bytes: ByteBuf,
        timestamp: DateTime,
        integer: BigInteger,
        wide: u128,
        decimal: BigDecimal,
    }

    let document = object(&[
        ("blob", DocumentV2::Blob(vec![0, 0xff])),
        ("bytes", DocumentV2::Blob(vec![0, 0xff])),
        ("timestamp", DocumentV2::Timestamp(DateTime::from_secs(0))),
        ("integer", big_integer("-123456789012345678901234567890")),
        (
            "wide",
            big_integer("340282366920938463463374607431768211455"),
        ),
        ("decimal", big_decimal("1.5e300")),
    ]);
    let native: Native = from_document_v2(document).unwrap();
    assert_eq!(
        native,
        Native {
            blob: Blob::new(vec![0, 0xff]),
            bytes: ByteBuf(vec![0, 0xff]),
            timestamp: DateTime::from_secs(0),
            integer: BigInteger::from_str("-123456789012345678901234567890").unwrap(),
            wide: u128::MAX,
            decimal: BigDecimal::from_str("1.5e300").unwrap(),
        }
    );
}

#[test]
fn native_variants_deserialize_into_strings_as_their_text() {
    let cases = [
        (DocumentV2::Blob(b"hello".to_vec()), "aGVsbG8="),
        (
            DocumentV2::Timestamp(DateTime::from_secs(1_576_540_098)),
            "2019-12-16T23:48:18Z",
        ),
        (big_integer("00123"), "00123"),
        (big_decimal("-0.5E-10"), "-0.5E-10"),
    ];
    for (document, text) in cases {
        let string: String = from_document_v2(document).unwrap();
        assert_eq!(string, text);
    }

    // A type that asks for bytes gets the blob's bytes, not its text.
    let bytes: ByteBuf = from_document_v2(DocumentV2::Blob(b"hello".to_vec())).unwrap();
    assert_eq!(bytes, ByteBuf(b"hello".to_vec()));
}

#[test]
fn reading_native_variants_into_other_types_fails() {
    // A `Vec<u8>` reads a sequence, not bytes.
    assert!(from_document_v2::<Vec<u8>>(DocumentV2::Blob(vec![1])).is_err());
    // Big numbers read as text, so a 64-bit target can't read them.
    assert!(from_document_v2::<u64>(big_integer("5")).is_err());
    assert!(from_document_v2::<f64>(big_decimal("0.5")).is_err());
    // A 128-bit target reads only big integers in its range.
    assert!(from_document_v2::<u128>(big_integer("-1")).is_err());
    assert!(
        from_document_v2::<i128>(big_integer("170141183460469231731687303715884105728")).is_err()
    );

    let error = from_document_v2::<String>(DocumentV2::Timestamp(DateTime::from_secs(i64::MAX)))
        .unwrap_err();
    assert!(
        error.to_string().starts_with("cannot format timestamp:"),
        "{error}"
    );
}

#[test]
fn a_document_v2_reads_itself_back_like_a_json_round_trip() {
    let unchanged = object(&[
        ("null", DocumentV2::Null),
        ("number", DocumentV2::Number(Number::NegInt(-1))),
        ("array", DocumentV2::Array(vec![DocumentV2::Bool(true)])),
        (
            "nested",
            object(&[("z", DocumentV2::Null), ("a", DocumentV2::Null)]),
        ),
    ]);
    let read: DocumentV2 = from_document_v2(unchanged.clone()).unwrap();
    assert_eq!(read, unchanged);
    assert_eq!(keys(&read), ["null", "number", "array", "nested"]);
    assert_eq!(keys(&read.as_object().unwrap()["nested"]), ["z", "a"]);

    let changed = [
        (DocumentV2::Blob(vec![1, 2]), "AQI="),
        (
            DocumentV2::Timestamp(DateTime::from_secs(0)),
            "1970-01-01T00:00:00Z",
        ),
        (
            big_integer("12345678901234567890123"),
            "12345678901234567890123",
        ),
        (big_decimal("1.5"), "1.5"),
    ];
    for (document, text) in changed {
        let expected = DocumentV2::String(text.to_owned());
        assert_eq!(
            from_document_v2::<DocumentV2>(document.clone()).unwrap(),
            expected
        );
        assert_eq!(to_document_v2(&document).unwrap(), expected);
        let json = serde_json::to_value(&document).unwrap();
        assert_eq!(
            serde_json::from_value::<DocumentV2>(json).unwrap(),
            expected
        );
    }
}

#[test]
fn other_document_types_read_blobs_as_base64() {
    let document = object(&[
        ("s", DocumentV2::String("x".to_owned())),
        ("b", DocumentV2::Blob(b"hi".to_vec())),
    ]);
    let v1: Document = from_document_v2(document.clone()).unwrap();
    assert_eq!(v1, document.to_document_lossy());
    assert_eq!(
        v1.as_object().unwrap()["b"],
        Document::String("aGk=".to_owned())
    );

    let json: serde_json::Value = from_document_v2(document).unwrap();
    assert_eq!(json, serde_json::json!({"s": "x", "b": "aGk="}));
}

#[test]
fn deserialization_errors_match_document() {
    let error = from_document_v2::<bool>(DocumentV2::String("x".to_owned())).unwrap_err();
    assert!(error.to_string().contains("invalid type"), "{error}");

    #[derive(Debug, Deserialize)]
    struct Required {
        #[allow(dead_code)]
        x: u32,
    }
    let error =
        from_document_v2::<Required>(DocumentV2::Object(DocumentObject::new())).unwrap_err();
    assert!(error.to_string().contains("missing field"), "{error}");

    let error = to_document_v2(&HashMap::from([(None::<u32>, 1)])).unwrap_err();
    assert_eq!(error.to_string(), "key must be a string");

    let two_entries = object(&[("Dog", DocumentV2::Null), ("Cat", DocumentV2::Null)]);
    let error = from_document_v2::<Animal>(two_entries).unwrap_err();
    assert_eq!(
        error.to_string(),
        "expected a single-entry map for enum variant"
    );
}

// -- Serialize and Deserialize for DocumentV2 -----------------------------------------------

#[test]
fn json_shaped_variants_serialize_like_document() {
    let json = include_str!("../../../test_data/serialize_document.json");
    let v1: Document = serde_json::from_str(json).unwrap();
    let v2 = DocumentV2::from(&v1);
    assert_eq!(
        serde_json::to_value(&v2).unwrap(),
        serde_json::to_value(&v1).unwrap()
    );

    let parsed: DocumentV2 = serde_json::from_str(json).unwrap();
    assert_eq!(parsed, v2);
}

#[test]
fn json_parsing_keeps_object_order_and_the_last_repeated_value() {
    let parsed: DocumentV2 =
        serde_json::from_str(r#"{"z": 1, "a": {"y": 2, "b": 3}, "z": -4}"#).unwrap();
    assert_eq!(keys(&parsed), ["z", "a"]);
    assert_eq!(
        parsed.as_object().unwrap()["z"],
        DocumentV2::Number(Number::NegInt(-4))
    );
    assert_eq!(keys(&parsed.as_object().unwrap()["a"]), ["y", "b"]);
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#"{"z":-4,"a":{"y":2,"b":3}}"#
    );

    let object: DocumentObject = serde_json::from_str(r#"{"b": null, "a": true}"#).unwrap();
    assert_eq!(
        object.keys().map(String::as_str).collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert!(serde_json::from_str::<DocumentObject>("[]").is_err());
}

#[test]
fn native_variants_come_back_from_json_as_strings() {
    let document = DocumentV2::Array(vec![
        DocumentV2::Blob(b"hi".to_vec()),
        DocumentV2::Timestamp(DateTime::from_secs(0)),
        big_integer("12345678901234567890123"),
        big_decimal("1.5e300"),
    ]);
    let json = serde_json::to_string(&document).unwrap();
    assert_eq!(
        json,
        r#"["aGk=","1970-01-01T00:00:00Z","12345678901234567890123","1.5e300"]"#
    );

    let parsed: DocumentV2 = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed,
        DocumentV2::Array(vec![
            DocumentV2::String("aGk=".to_owned()),
            DocumentV2::String("1970-01-01T00:00:00Z".to_owned()),
            DocumentV2::String("12345678901234567890123".to_owned()),
            DocumentV2::String("1.5e300".to_owned()),
        ])
    );
}

#[test]
fn binary_formats_keep_blobs_and_wide_integers() {
    fn to_cbor(value: &impl Serialize) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(value, &mut bytes).unwrap();
        bytes
    }

    // CBOR isn't human-readable, so a blob is written as a byte string and read back as one.
    let blob = DocumentV2::Blob(vec![0, 0xff]);
    let read: DocumentV2 = ciborium::de::from_reader(to_cbor(&blob).as_slice()).unwrap();
    assert_eq!(read, blob);

    // A CBOR bignum is read as a 128-bit integer.
    let read: DocumentV2 = ciborium::de::from_reader(to_cbor(&u128::MAX).as_slice()).unwrap();
    assert_eq!(read, big_integer("340282366920938463463374607431768211455"));

    // A timestamp is written like `DateTime`: as seconds and nanoseconds.
    let timestamp = DocumentV2::Timestamp(DateTime::from_secs_and_nanos(5, 6));
    let read: DocumentV2 = ciborium::de::from_reader(to_cbor(&timestamp).as_slice()).unwrap();
    assert_eq!(
        read,
        DocumentV2::Array(vec![
            DocumentV2::Number(Number::PosInt(5)),
            DocumentV2::Number(Number::PosInt(6)),
        ])
    );
}

#[test]
fn non_negative_signed_integers_deserialize_as_pos_int() {
    let deserializer: serde::de::value::I64Deserializer<serde::de::value::Error> =
        5i64.into_deserializer();
    assert_eq!(
        DocumentV2::deserialize(deserializer).unwrap(),
        DocumentV2::Number(Number::PosInt(5))
    );
}

#[test]
fn size_hints_from_the_input_are_capped() {
    assert_eq!(cautious_capacity::<u64>(None), 0);
    assert_eq!(cautious_capacity::<u64>(Some(3)), 3);
    assert_eq!(cautious_capacity::<u64>(Some(usize::MAX)), 128 * 1024);
}

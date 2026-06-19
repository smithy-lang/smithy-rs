/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Fuzz target: round-trip correctness for the schema-based XML codec.
//!
//! # Strategy
//!
//! `Arbitrary` interprets libfuzzer bytes as a structured [`FuzzValue`].
//! The harness:
//!
//! 1. Wraps the value in its single-member struct via `wrapper_schema_for`.
//! 2. Serializes through `XmlSerializer`.
//! 3. Deserializes the output through `XmlDeserializer` against the same
//!    wrapper schema, picking the read path from `original`'s variant.
//! 4. Asserts the two `FuzzValue`s compare equal under [`fuzz_values_equal`].
//!
//! Schema matching is guaranteed by design: both sides use the schema
//! returned by `wrapper_schema_for(value)`.
//!
//! # Why we skip some inputs
//!
//! - **XML 1.0 invalid chars** (NUL, most C0 control bytes, etc.). The
//!   serializer's `escape()` emits `&#x0;` for NUL, which is not legal
//!   XML 1.0 — the deserializer (via `xmlparser`) correctly rejects it.
//!   That's not a round-trip bug, it's a fundamental representation gap
//!   and matches the well-known limitation called out in the XML 1.0
//!   spec §2.2. Tested in serialize-validity instead.
//!
//! - **Non-finite floats** (NaN, +/-Infinity). Smithy XML spells these
//!   `NaN` / `Infinity` / `-Infinity` on the wire, but `read_float` /
//!   `read_double` only accept numeric forms. This is a known asymmetry
//!   in the codec — symmetric-format roundtrip is not a goal here, and
//!   the deserialize fuzz target already exercises the parsing side.
//!
//! - **Empty map keys**. `<key></key>` is well-formed XML, but several
//!   downstream XML toolchains collapse empty elements; the JSON harness
//!   makes the same skip for `""` map keys. Better safe than chasing
//!   an environmental false positive.
//!
//! - **Map keys containing `\0` after a passing XML 1.0 check**. Same
//!   reason as XML 1.0 invalid chars; folded into
//!   `contains_xml10_invalid_char`.

#![no_main]
use libfuzzer_sys::fuzz_target;

mod schema_common;

use schema_common::*;

fuzz_target!(|value: FuzzValue| {
    if contains_xml10_invalid_char(&value) {
        return;
    }
    if contains_non_finite_float(&value) {
        return;
    }
    if has_empty_map_key(&value) {
        return;
    }

    let codec = default_codec();
    let bytes = match serialize_value(&value, &codec) {
        Ok(b) => b,
        Err(_) => return,
    };

    let deserialized = match deserialize_value(&bytes, &value, &codec) {
        Ok(v) => v,
        Err(e) => panic!(
            "Deserialization failed!\nOriginal: {:?}\nSerialized: {:?}\nError: {:?}",
            value,
            String::from_utf8_lossy(&bytes),
            e,
        ),
    };

    assert!(
        fuzz_values_equal(&value, &deserialized),
        "Round-trip mismatch!\nOriginal: {:?}\nSerialized: {:?}\nDeserialized: {:?}",
        value,
        String::from_utf8_lossy(&bytes),
        deserialized,
    );
});

fn has_empty_map_key(value: &FuzzValue) -> bool {
    match value {
        FuzzValue::StringStringMap(entries) => entries
            .iter()
            .any(|(k, _)| map_key_invalid_for_roundtrip(k)),
        FuzzValue::StringIntegerMap(entries) => entries
            .iter()
            .any(|(k, _)| map_key_invalid_for_roundtrip(k)),
        FuzzValue::StringLongMap(entries) => entries
            .iter()
            .any(|(k, _)| map_key_invalid_for_roundtrip(k)),
        _ => false,
    }
}

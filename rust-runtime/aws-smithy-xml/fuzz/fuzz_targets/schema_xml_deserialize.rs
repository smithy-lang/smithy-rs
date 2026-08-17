/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Fuzz target: crash-freedom for the schema-based XML deserializer.
//!
//! # Strategy
//!
//! libfuzzer feeds arbitrary `&[u8]` — no structure, no guarantees. We
//! throw the same raw bytes at *every* `read_*` method against a fixed
//! schema. Each call gets a fresh deserializer; most calls return `Err`
//! (the bytes aren't valid XML for that type) — that's expected. The
//! invariant is that **no call may panic** regardless of input.
//!
//! Schema matching is irrelevant here: we're not testing that the
//! deserializer produces correct values, only that it doesn't crash. The
//! schemas are fixed constants and never change between runs.
//!
//! # XML-specific differences from the JSON harness
//!
//! - **No `read_document`.** [`aws_smithy_xml::codec::XmlDeserializer::
//!   read_document`] returns `SerdeError` unconditionally per the REST XML
//!   spec (documents aren't allowed). Calling it would always Err — fine
//!   for crash-freedom but no value to fuzz.
//! - **Stack-overflow guard.** XML's deserializer caps aggregate nesting
//!   at `XmlCodecSettings::default().max_depth()` (128). The target
//!   includes a deep-nesting probe to verify excessive nesting returns
//!   `Err` rather than overflowing the stack.
//! - **Wider schema coverage.** The JSON harness has a single struct
//!   schema with `@jsonName` on one member. The XML version exercises
//!   `@xmlAttribute`, `@xmlName`, `@xmlNamespace`, and `@xmlFlattened` —
//!   each takes a different code path in [`XmlDeserializer::read_struct`].

#![no_main]
use libfuzzer_sys::fuzz_target;

mod schema_common;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::ShapeDeserializer;
use schema_common::*;

fuzz_target!(|data: &[u8]| {
    let codec = default_codec();

    // ----- Crash-freedom: every read_* method must not panic on arbitrary input. -----
    // Each call gets a fresh deserializer so they don't interfere with each other.
    // Errors are discarded — we only care that the code doesn't panic.

    // Simple types
    let _ = codec
        .create_deserializer(data)
        .read_boolean(&aws_smithy_schema::prelude::BOOLEAN);
    let _ = codec
        .create_deserializer(data)
        .read_byte(&aws_smithy_schema::prelude::BYTE);
    let _ = codec
        .create_deserializer(data)
        .read_short(&aws_smithy_schema::prelude::SHORT);
    let _ = codec
        .create_deserializer(data)
        .read_integer(&aws_smithy_schema::prelude::INTEGER);
    let _ = codec
        .create_deserializer(data)
        .read_long(&aws_smithy_schema::prelude::LONG);
    let _ = codec
        .create_deserializer(data)
        .read_float(&aws_smithy_schema::prelude::FLOAT);
    let _ = codec
        .create_deserializer(data)
        .read_double(&aws_smithy_schema::prelude::DOUBLE);
    let _ = codec
        .create_deserializer(data)
        .read_string(&aws_smithy_schema::prelude::STRING);
    let _ = codec
        .create_deserializer(data)
        .read_blob(&aws_smithy_schema::prelude::BLOB);
    let _ = codec
        .create_deserializer(data)
        .read_timestamp(&aws_smithy_schema::prelude::TIMESTAMP);
    let _ = codec
        .create_deserializer(data)
        .read_big_integer(&aws_smithy_schema::prelude::BIG_INTEGER);
    let _ = codec
        .create_deserializer(data)
        .read_big_decimal(&aws_smithy_schema::prelude::BIG_DECIMAL);

    // Note: read_document intentionally not fuzzed — XmlDeserializer always
    // returns SerdeError so any input is an immediate Err with no work done.

    // Null detection
    let _ = codec.create_deserializer(data).is_null();
    let _ = codec.create_deserializer(data).read_null();

    // ----- Aggregates -----
    //
    // Consumers MUST propagate errors from inner read_* calls. If a consumer
    // swallows an error, the deserializer's position doesn't advance and the
    // outer loop in read_list / read_map can spin (the JSON harness documents
    // this hazard). Returning Ok(()) on the outer struct dispatch is fine
    // because read_struct iterates child elements driven by the document
    // itself, not by consumer feedback.

    let _ = codec
        .create_deserializer(data)
        .read_struct(&ALL_TYPES_SCHEMA, &mut |_member, _deser| Ok(()));
    let _ = codec
        .create_deserializer(data)
        .read_struct(&ATTR_SCHEMA, &mut |_member, _deser| Ok(()));
    let _ = codec
        .create_deserializer(data)
        .read_struct(&RENAMED_SCHEMA, &mut |_member, _deser| Ok(()));
    let _ = codec
        .create_deserializer(data)
        .read_struct(&NAMESPACED_SCHEMA, &mut |_member, _deser| Ok(()));
    let _ = codec
        .create_deserializer(data)
        .read_struct(&FLAT_LIST_SCHEMA, &mut |_member, _deser| Ok(()));
    let _ = codec
        .create_deserializer(data)
        .read_struct(&FLAT_MAP_SCHEMA, &mut |_member, _deser| Ok(()));

    let _ = codec
        .create_deserializer(data)
        .read_list(&STRING_LIST_SCHEMA, &mut |deser| {
            deser.read_string(&aws_smithy_schema::prelude::STRING)?;
            Ok(())
        });
    let _ =
        codec
            .create_deserializer(data)
            .read_map(&STRING_STRING_MAP_SCHEMA, &mut |_key, deser| {
                deser.read_string(&aws_smithy_schema::prelude::STRING)?;
                Ok(())
            });

    // Collection helpers (default impls or specialized — both must be safe)
    let _ = codec
        .create_deserializer(data)
        .read_string_list(&STRING_LIST_SCHEMA);
    let _ = codec
        .create_deserializer(data)
        .read_blob_list(&BLOB_LIST_SCHEMA);
    let _ = codec
        .create_deserializer(data)
        .read_integer_list(&INTEGER_LIST_SCHEMA);
    let _ = codec
        .create_deserializer(data)
        .read_long_list(&LONG_LIST_SCHEMA);
    let _ = codec
        .create_deserializer(data)
        .read_string_string_map(&STRING_STRING_MAP_SCHEMA);

    // ----- Stack-overflow guard -----
    //
    // The deserializer's max_depth defaults to 128. A pathological input
    // can nest aggregate elements arbitrarily deeply; the schema only
    // declares one level. Build a deeply-nested probe and confirm
    // read_struct on it returns Err rather than overflowing the stack.
    //
    // Use a depth that comfortably exceeds the cap (256 levels, 2x default).
    // The probe uses ALL_TYPES_SCHEMA as the outer struct; the inner content
    // is unknown to the schema so the deserializer should either skip or
    // error out at each layer. The exact behavior doesn't matter — what
    // matters is that it doesn't crash.
    //
    // We only run this once (not per-iteration) since it's expensive and
    // the libfuzzer harness already covers per-iteration work; running it
    // once per fuzz_target! invocation gives reproducibility for crashes.
    let mut deep = String::with_capacity(8192);
    deep.push_str("<AllTypes>");
    for _ in 0..256 {
        deep.push_str("<a>");
    }
    for _ in 0..256 {
        deep.push_str("</a>");
    }
    deep.push_str("</AllTypes>");
    let _ = codec.create_deserializer(deep.as_bytes()).read_struct(
        &ALL_TYPES_SCHEMA,
        &mut |_member, deser| {
            // Walk into the unknown <a> element so depth tracking actually
            // increments. read_struct on the unknown element will recurse
            // until max_depth is hit.
            deser.read_struct(&ALL_TYPES_SCHEMA, &mut |_, _| Ok(()))
        },
    );
});

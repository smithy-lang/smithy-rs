/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Arbitrary bytes split into two IDL files and a JSON AST document that resolve relative
//! shape IDs against each other.

#![no_main]

mod common;

use libfuzzer_sys::fuzz_target;

/// Declares shapes the fuzzed files can refer to from the same namespace.
const JSON: &str = r#"{"smithy": "2.0", "shapes": {
    "ex#FromJson": {"type": "string"},
    "ex#Resource": {"type": "resource", "identifiers": {"id": {"target": "ex#FromJson"}}},
    "ex#jsonTrait": {"type": "structure", "members": {}, "traits": {"smithy.api#trait": {}}}
}}"#;

fuzz_target!(|data: &[u8]| {
    // Split at the first zero byte, so the fuzzer controls both files.
    let (first, second) = match data.iter().position(|b| *b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    common::assemble(|assembler| {
        assembler
            .add_idl_slice("a.smithy", first.to_vec())
            .add_json_str("b.json", JSON)
            .add_idl_slice("c.smithy", second.to_vec())
    });
});

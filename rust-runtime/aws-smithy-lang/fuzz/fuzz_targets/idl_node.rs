/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Arbitrary bytes as a node value (numbers, quoted strings, text blocks, arrays, objects, and
//! syntactic shape IDs), both in metadata and as a trait value.

#![no_main]

mod common;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = std::str::from_utf8(data) else {
        return;
    };
    let metadata = format!("$version: \"2\"\nmetadata m = {value}\n");
    common::assemble(|assembler| assembler.add_idl_str("metadata.smithy", metadata.clone()));
    let applied = format!("namespace ex\n@tags({value})\n@documentation({value})\nstring S\n");
    common::assemble(|assembler| assembler.add_idl_str("trait.smithy", applied.clone()));
});

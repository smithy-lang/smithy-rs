/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Arbitrary bytes as one IDL file.

#![no_main]

mod common;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    common::assemble(|assembler| assembler.add_idl_slice("fuzz.smithy", data.to_vec()));
});

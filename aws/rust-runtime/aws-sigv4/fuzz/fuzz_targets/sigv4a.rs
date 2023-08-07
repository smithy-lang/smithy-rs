/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![no_main]

use aws_sigv4::sign::v4a::generate_signing_key;
use libfuzzer_sys::fuzz_target;

#[derive(derive_arbitrary::Arbitrary, Debug)]
struct Input {
    access_key_id: String,
    secret_access_key: String,
}

fuzz_target!(|input: Input| {
    // We're looking for panics
    let _ = generate_signing_key(&input.access_key_id, &input.secret_access_key);
});

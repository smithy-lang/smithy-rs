/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Defaults shared between `main.rs` and `/tests`.
pub const DEFAULT_TEST_KEY: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.key");
pub const DEFAULT_TEST_CERT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.crt");
pub const DEFAULT_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 13734;
pub const DEFAULT_DOMAIN: &str = "localhost";

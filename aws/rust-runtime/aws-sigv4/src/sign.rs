/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions to create signing keys and calculate signatures.

/// Support for Sigv4 signing
pub mod v4;
#[cfg(feature = "sigv4a")]
/// Support for Sigv4a signing
pub mod v4a;

pub use v4::{calculate_signature, generate_signing_key};

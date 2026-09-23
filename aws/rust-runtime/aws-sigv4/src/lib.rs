/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
//! Provides functions for calculating Sigv4 signing keys, signatures, and
//! optional utilities for signing HTTP requests and Event Stream messages.
//!
//! # Crypto backends
//!
//! HMAC-SHA256, SHA-256, and (under the `sigv4a` feature) ECDSA-P256 signing are performed by
//! one of two backends, chosen by feature at compile time:
//!
//! | Feature | Implementation | FIPS 140-3 validated |
//! |---|---|---|
//! | `rustcrypto` (default) | the [RustCrypto](https://github.com/RustCrypto) crates | no |
//! | `aws-lc-rs` | [aws-lc-rs](https://github.com/aws/aws-lc-rs) on the standard AWS-LC build | no |
//! | `fips` | aws-lc-rs on the FIPS build of AWS-LC | yes |
//!
//! `fips` takes precedence over `aws-lc-rs`, and either takes precedence over `rustcrypto`, so
//! enabling more than one — which Cargo feature unification does routinely — resolves to the
//! strongest backend rather than failing to build.
//!
//! FIPS is a per-target capability. `fips` builds `aws-lc-fips-sys`, which is only available on
//! CMVP-validated operating environments (Linux, macOS, and Windows) and is unavailable on iOS
//! and WASM, and which needs a C compiler, CMake, and Go at build time.
//!
//! Two RustCrypto crates remain compiled in a `sigv4a` + `fips` build, neither of which performs
//! FIPS-relevant cryptography:
//!
//! - `crypto-bigint`, for the 256-bit integer math in [`sign::v4a::generate_signing_key`]. That
//!   is the key derivation the SigV4a specification defines, not a cryptographic primitive.
//! - `p256`, which the signing path no longer calls. `sigv4a` has to keep declaring it, because
//!   Cargo features are additive and cannot express "only when `rustcrypto` is also enabled".
//!
//! Signing output is unchanged by the choice of backend. SigV4 signatures are deterministic and
//! verified against the shared signing test suite on both; SigV4a signatures are
//! non-deterministic by construction and are verified, not compared.

#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

use std::fmt;

pub mod sign;

mod crypto;
mod date_time;

#[cfg(feature = "sign-eventstream")]
pub mod event_stream;

#[cfg(feature = "sign-http")]
pub mod http_request;

/// The version of the signing algorithm to use
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[non_exhaustive]
pub enum SignatureVersion {
    /// The SigV4 signing algorithm.
    V4,
    /// The SigV4a signing algorithm.
    V4a,
}

impl fmt::Display for SignatureVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureVersion::V4 => write!(f, "SigV4"),
            SignatureVersion::V4a => write!(f, "SigV4a"),
        }
    }
}

/// Container for the signed output and the signature.
///
/// This is returned by signing functions, and the signed output will be
/// different based on what is being signed (for example, an event stream
/// message, or an HTTP request).
#[derive(Debug)]
pub struct SigningOutput<T> {
    output: T,
    signature: String,
}

impl<T> SigningOutput<T> {
    /// Creates a new [`SigningOutput`]
    pub fn new(output: T, signature: String) -> Self {
        Self { output, signature }
    }

    /// Returns the signed output
    pub fn output(&self) -> &T {
        &self.output
    }

    /// Returns the signature as a lowercase hex string
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Decomposes the `SigningOutput` into a tuple of the signed output and the signature
    pub fn into_parts(self) -> (T, String) {
        (self.output, self.signature)
    }
}

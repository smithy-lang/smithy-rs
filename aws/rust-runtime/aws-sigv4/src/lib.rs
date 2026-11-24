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
//! | Selected by | Implementation | FIPS 140-3 validated |
//! |---|---|---|
//! | nothing (the default) | the [RustCrypto](https://github.com/RustCrypto) crates | no |
//! | `aws-lc-rs` | [aws-lc-rs](https://github.com/aws/aws-lc-rs) on the standard AWS-LC build | no |
//! | `aws-lc-rs-fips` | aws-lc-rs on the FIPS build of AWS-LC | yes |
//!
//! `aws-lc-rs-fips` takes precedence over `aws-lc-rs`, and either over RustCrypto, so enabling
//! more than one — which Cargo feature unification does routinely — resolves to the strongest
//! backend rather than failing to build.
//!
//! ## Platform support
//!
//! RustCrypto is the default because it is the only backend that builds everywhere the SDK does.
//! Both AWS-LC backends are per-target; see
//! [aws-lc-rs platform support](https://aws.github.io/aws-lc-rs/platform_support.html) for the
//! authoritative table. `aws-lc-rs` needs a C/C++ compiler and supports no WASM target other than
//! `wasm32-unknown-emscripten`. `aws-lc-rs-fips` additionally needs CMake and Go and covers fewer
//! targets still, notably not i686, WASM, iOS or Android. Enabling either on an unsupported target
//! fails while building `aws-lc-sys` or `aws-lc-fips-sys`, before this crate is reached.
//!
//! Which AWS-LC-FIPS module `aws-lc-rs-fips` binds follows the resolved `aws-lc-rs` version; this
//! crate's `Cargo.toml` documents the declared floor.
//!
//! ## What ends up in the dependency tree
//!
//! Selecting an AWS-LC backend changes which implementation runs, but `hmac` and `sha2` are
//! unconditional dependencies, so they are still compiled. To keep them out of the build as well,
//! set the `aws_sdk_fips_only` cfg, which is what their `[target."cfg(not(aws_sdk_fips_only))"]`
//! declaration responds to:
//!
//! ```sh
//! RUSTFLAGS="--cfg aws_sdk_fips_only" \
//!   cargo build --no-default-features --features sign-http,http1,aws-lc-rs-fips
//! ```
//!
//! A Cargo feature could not do this: features are additive, so "RustCrypto unless AWS-LC" is not
//! expressible, and making these optional would break every build that lists the default features
//! explicitly. `RUSTFLAGS` is global, so one flag also covers `aws-smithy-checksums`.
//!
//! `sigv4a` is unconditional: it declares `p256`, `ecdsa`, `crypto-bigint`, `subtle` and `zeroize`
//! under every backend, with `sha2` arriving under `p256` and `hmac` under `ecdsa`'s RFC 6979 nonce
//! derivation. None of it performs FIPS-relevant cryptography on an AWS-LC backend. `p256` and
//! `ecdsa` are the ECDSA implementation for the RustCrypto backend only, and it does not reach
//! them otherwise; `crypto-bigint` does the 256-bit range check
//! [`sign::v4a::generate_signing_key`] applies to the derived scalar, the derivation itself going
//! through the backend. They stay unconditional because a feature cannot be made conditional on
//! another feature being absent.
//!
//! SigV4 output does not depend on the backend: HMAC-SHA256 is deterministic, and both backends are
//! held to the same known-answer vectors and signing test suite.
//!
//! SigV4a output does. ECDSA leaves the per-signature nonce to the implementation. RustCrypto
//! derives it per RFC 6979, so the same inputs always produce the same signature and
//! [`sign::v4a`]'s tests pin those bytes; AWS-LC randomizes it, so signing one input twice yields
//! two different signatures and those tests verify rather than compare. Both are valid over the
//! same string to sign.

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

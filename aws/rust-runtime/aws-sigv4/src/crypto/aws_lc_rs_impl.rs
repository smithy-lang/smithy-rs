/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [aws-lc-rs](https://github.com/aws/aws-lc-rs) crypto backend.
//!
//! With the `fips` feature, these operations are performed by the FIPS 140-3 validated build of
//! AWS-LC. With `aws-lc-rs` alone they are performed by the same implementations in a build that
//! is not operating in FIPS mode.

use super::SHA256_OUTPUT_SIZE;
use aws_lc_rs::{digest, hmac};
#[cfg(feature = "sigv4a")]
use zeroize::Zeroizing;

/// HMAC-SHA256, computed in one or more `update` steps.
#[derive(Clone)]
pub(crate) struct HmacSha256 {
    context: hmac::Context,
}

impl HmacSha256 {
    pub(crate) fn new(key: &[u8]) -> Self {
        Self {
            context: hmac::Context::with_key(&hmac::Key::new(hmac::HMAC_SHA256, key)),
        }
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        self.context.update(data);
    }

    pub(crate) fn finalize(self) -> [u8; SHA256_OUTPUT_SIZE] {
        fixed_size(self.context.sign().as_ref())
    }
}

pub(crate) fn sha256(data: &[u8]) -> [u8; SHA256_OUTPUT_SIZE] {
    fixed_size(digest::digest(&digest::SHA256, data).as_ref())
}

/// Copies a SHA-256-sized slice into an array.
///
/// AWS-LC returns digests and tags as slices of a runtime length, which for these fixed-output
/// algorithms is always `SHA256_OUTPUT_SIZE`.
fn fixed_size(bytes: &[u8]) -> [u8; SHA256_OUTPUT_SIZE] {
    bytes
        .try_into()
        .expect("SHA-256 digests and HMAC-SHA256 tags are always 32 bytes")
}

/// Signs `message` with `private_key` using ECDSA-P256-SHA256, returning a DER-encoded signature.
///
/// `private_key` is a big-endian P-256 private scalar, as produced by
/// [`crate::sign::v4a::generate_signing_key`].
///
/// # Panics
/// Panics if `private_key` is not a valid P-256 private key.
#[cfg(feature = "sigv4a")]
pub(crate) fn ecdsa_p256_sha256_sign_der(private_key: &[u8], message: &[u8]) -> Vec<u8> {
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    let der = rfc5915_p256_private_key(private_key);
    let key_pair = EcdsaKeyPair::from_private_key_der(&ECDSA_P256_SHA256_ASN1_SIGNING, &der)
        .expect("the scalar is a valid P-256 private key");
    key_pair
        .sign(&SystemRandom::new(), message)
        .expect("signing a SigV4a string to sign cannot fail")
        .as_ref()
        .to_vec()
}

/// Wraps a raw P-256 private scalar in the DER encoding aws-lc-rs accepts.
///
/// aws-lc-rs can only import an ECDSA private key from DER or PKCS#8, never from the raw scalar
/// that SigV4a's key derivation produces, so build the RFC 5915 `ECPrivateKey` around it. Its
/// `publicKey` field is optional and omitted here; AWS-LC derives the public point from the
/// scalar while parsing.
///
/// Every field is fixed size for P-256, so the encoding is a constant prefix and suffix around
/// the scalar rather than a general-purpose DER writer.
///
/// # Panics
/// Panics if `private_key` is not 32 bytes, which is what makes the fixed lengths below correct.
#[cfg(feature = "sigv4a")]
fn rfc5915_p256_private_key(private_key: &[u8]) -> Zeroizing<Vec<u8>> {
    /// Size in bytes of a P-256 private scalar, which the DER lengths below are written for.
    const P256_PRIVATE_KEY_SIZE: usize = 32;
    // SEQUENCE (49 bytes) { INTEGER 1, OCTET STRING (32 bytes) {
    const PREFIX: [u8; 7] = [0x30, 0x31, 0x02, 0x01, 0x01, 0x04, 0x20];
    // } [0] (10 bytes) { OBJECT IDENTIFIER 1.2.840.10045.3.1.7 (prime256v1) } }
    const SUFFIX: [u8; 12] = [
        0xa0, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07,
    ];

    assert_eq!(
        P256_PRIVATE_KEY_SIZE,
        private_key.len(),
        "a P-256 private key must be {P256_PRIVATE_KEY_SIZE} bytes"
    );

    let mut der = Zeroizing::new(Vec::with_capacity(
        PREFIX.len() + private_key.len() + SUFFIX.len(),
    ));
    der.extend_from_slice(&PREFIX);
    der.extend_from_slice(private_key);
    der.extend_from_slice(&SUFFIX);
    der
}

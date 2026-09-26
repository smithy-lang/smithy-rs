/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [RustCrypto](https://github.com/RustCrypto) crypto backend. Not FIPS-validated.

use super::SHA256_OUTPUT_SIZE;
use hmac::{digest::FixedOutput, Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// HMAC-SHA256, computed in one or more `update` steps.
#[derive(Clone)]
pub(crate) struct HmacSha256 {
    mac: Hmac<Sha256>,
}

impl HmacSha256 {
    pub(crate) fn new(key: &[u8]) -> Self {
        Self {
            mac: Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size"),
        }
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        self.mac.update(data);
    }

    pub(crate) fn finalize(self) -> [u8; SHA256_OUTPUT_SIZE] {
        self.mac.finalize_fixed().into()
    }
}

pub(crate) fn sha256(data: &[u8]) -> [u8; SHA256_OUTPUT_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize_fixed().into()
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
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{DerSignature, SigningKey};

    let signing_key = SigningKey::from_slice(private_key).unwrap();
    let signature: DerSignature = signing_key.sign(message);
    signature.as_bytes().to_vec()
}

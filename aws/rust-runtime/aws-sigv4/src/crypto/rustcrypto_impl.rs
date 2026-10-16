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
    use ecdsa::hazmat::sign_prehashed_rfc6979;
    use p256::{NistP256, NonZeroScalar};
    use sha2::Digest as _;
    use zeroize::Zeroizing;

    /// Size in bytes of a P-256 private scalar, which `NonZeroScalar::from_repr` takes a fixed
    /// amount of. The caller is what pads anything shorter.
    const P256_PRIVATE_KEY_SIZE: usize = 32;

    // Sign from the private scalar rather than from a `SigningKey`. Constructing one derives the
    // verifying key, and that costs a P-256 scalar multiplication as expensive as the signature
    // itself while signing never reads it. `NonZeroScalar::from_repr` range-checks the scalar
    // exactly as the `SecretKey` behind `SigningKey` does -- both reject zero and anything at or
    // above the group order.
    //
    // This is what `ecdsa`'s `PrehashSigner for SigningKey` does with that derivation left out,
    // so the signature is unchanged: same RFC 6979 deterministic nonce, same empty additional
    // data.
    //
    // Both copies this function owns, `repr` and `scalar`, are wiped on the way out; the
    // `private_key` slice is the caller's and is left alone. `SigningKey` got the wipe for free:
    // it implements `ZeroizeOnDrop` and its `Drop` zeroizes the secret scalar, whereas
    // `NonZeroScalar` implements `Zeroize` but has no `Drop`, so dropping one leaves the scalar in
    // memory. `Zeroizing` supplies the `Drop` that type is missing. The `FieldBytes` that
    // `from_repr` takes by value is still an unwiped stack copy; that one is not reachable through
    // this API.
    let repr = Zeroizing::new(
        <[u8; P256_PRIVATE_KEY_SIZE]>::try_from(private_key)
            .expect("a P-256 private scalar is 32 bytes"),
    );
    let scalar = Zeroizing::new(
        Option::<NonZeroScalar>::from(NonZeroScalar::from_repr((*repr).into()))
            .expect("signing key is a valid P-256 private scalar"),
    );
    let z = Sha256::digest(message);
    let (signature, _recovery_id) = sign_prehashed_rfc6979::<NistP256, Sha256>(&scalar, &z, &[]);
    signature.to_der().as_bytes().to_vec()
}

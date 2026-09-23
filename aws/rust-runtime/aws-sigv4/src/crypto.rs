/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Internal abstraction over the cryptographic implementations this crate can be built against.
//!
//! Two backends are supported, selected at compile time:
//! - [RustCrypto](https://github.com/RustCrypto) (`rustcrypto`, enabled by default). Not
//!   FIPS-validated.
//! - [aws-lc-rs](https://github.com/aws/aws-lc-rs) (`aws-lc-rs`, or `fips` for the FIPS 140-3
//!   validated build of AWS-LC; `fips` wins if both are enabled).
//!
//! If the features for both backends are enabled, aws-lc-rs is selected.
//!
//! What the backend covers: HMAC-SHA256 and SHA-256 for SigV4, plus ECDSA-P256 signing for
//! SigV4a. The SigV4a key derivation in [`crate::sign::v4a::generate_signing_key`] also does
//! 256-bit integer math with `crypto-bigint`; that is the KDF the signing spec defines rather
//! than a cryptographic primitive, so it stays on `crypto-bigint` in a FIPS build.

/// Size in bytes of a SHA-256 digest, and so of an HMAC-SHA256 tag.
pub(crate) const SHA256_OUTPUT_SIZE: usize = 32;

#[cfg(not(any(feature = "rustcrypto", feature = "__aws-lc-rs")))]
compile_error!(
    "aws-sigv4 requires a crypto backend: enable the `rustcrypto` (default), `aws-lc-rs`, or \
     `fips` feature."
);

#[cfg(feature = "__aws-lc-rs")]
mod aws_lc_rs_impl;
#[cfg(feature = "__aws-lc-rs")]
pub(crate) use aws_lc_rs_impl::HmacSha256;

// The RustCrypto backend is only compiled when it is the selected backend, so that enabling
// `aws-lc-rs` alongside the default features doesn't route signing through RustCrypto.
#[cfg(all(feature = "rustcrypto", not(feature = "__aws-lc-rs")))]
mod rustcrypto_impl;
#[cfg(all(feature = "rustcrypto", not(feature = "__aws-lc-rs")))]
pub(crate) use rustcrypto_impl::HmacSha256;

#[cfg(feature = "sigv4a")]
pub(crate) use imp::ecdsa_p256_sha256_sign_der;
pub(crate) use imp::sha256;

#[cfg(feature = "__aws-lc-rs")]
use aws_lc_rs_impl as imp;
#[cfg(all(feature = "rustcrypto", not(feature = "__aws-lc-rs")))]
use rustcrypto_impl as imp;

#[cfg(test)]
mod tests {
    use super::{sha256, HmacSha256, SHA256_OUTPUT_SIZE};

    // Known-answer tests, so that both backends are held to the same vectors.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    // RFC 4231 test case 2: key "Jefe", data "what do ya want for nothing?".
    const RFC4231_TC2_KEY: &[u8] = b"Jefe";
    const RFC4231_TC2_TAG: &str =
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";

    #[test]
    fn sha256_known_answer() {
        assert_eq!(EMPTY_SHA256, hex::encode(sha256(b"")));
        assert_eq!(
            SHA256_OUTPUT_SIZE,
            sha256(b"the length is fixed regardless of input").len()
        );
    }

    #[test]
    fn hmac_sha256_known_answer() {
        // Split across two `update` calls to exercise the multi-step path the signing key
        // derivation relies on.
        let mut hmac = HmacSha256::new(RFC4231_TC2_KEY);
        hmac.update(b"what do ya want ");
        hmac.update(b"for nothing?");
        assert_eq!(RFC4231_TC2_TAG, hex::encode(hmac.finalize()));
    }

    // SigV4a signatures are non-deterministic, so this checks the signature verifies rather
    // than comparing bytes. `p256` is the verifier on both backends, which makes this a
    // cross-implementation check when aws-lc-rs is the signer.
    #[test]
    #[cfg(feature = "sigv4a")]
    fn ecdsa_p256_sha256_signature_verifies() {
        use p256::ecdsa::signature::Verifier;
        use p256::ecdsa::{DerSignature, SigningKey};

        // An arbitrary valid P-256 scalar, shaped like what `generate_signing_key` derives.
        let private_key = [0x42u8; SHA256_OUTPUT_SIZE];
        let message = b"AWS4-ECDSA-P256-SHA256 string to sign";

        let signature = super::ecdsa_p256_sha256_sign_der(&private_key, message);

        let verifying_key = *SigningKey::from_slice(&private_key)
            .expect("test scalar is a valid P-256 private key")
            .verifying_key();
        verifying_key
            .verify(
                message,
                &DerSignature::try_from(signature.as_slice())
                    .expect("signature must be DER encoded"),
            )
            .expect("signature must verify under the derived public key");
    }

    // Whenever the aws-lc-rs feature is enabled it must win, even if `rustcrypto` is also
    // enabled (which it is by default). The backend modules are cfg'd so that only the
    // selected one is compiled, so the module path of `HmacSha256` is the observable signal.
    #[test]
    fn expected_backend_is_selected() {
        let selected = std::any::type_name::<HmacSha256>();
        if cfg!(feature = "__aws-lc-rs") {
            assert!(
                selected.contains("aws_lc_rs_impl"),
                "expected the aws-lc-rs backend, got {selected}"
            );
        } else {
            assert!(
                selected.contains("rustcrypto_impl"),
                "expected the RustCrypto backend, got {selected}"
            );
        }
    }

    // The `fips` feature is only meaningful if the AWS-LC build it selects is actually the
    // validated one, which is a property of the linked C library rather than of this crate.
    #[test]
    #[cfg(feature = "fips")]
    fn fips_module_is_active() {
        aws_lc_rs::try_fips_mode().expect("the `fips` feature must link the FIPS AWS-LC build");
    }
}

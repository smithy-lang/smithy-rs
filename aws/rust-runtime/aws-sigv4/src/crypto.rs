/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Internal abstraction over the cryptographic implementations this crate can be built against.
//!
//! Covers HMAC-SHA256 and SHA-256 for SigV4, and ECDSA-P256 signing plus the SP 800-108
//! counter-mode key derivation for SigV4a. Two backends, selected at compile time:
//! [RustCrypto](https://github.com/RustCrypto) (the default, not FIPS-validated) and
//! [aws-lc-rs](https://github.com/aws/aws-lc-rs) (`aws-lc-rs`, or `aws-lc-rs-fips` for the
//! FIPS 140-3 validated build of AWS-LC). aws-lc-rs wins whenever it is enabled, and
//! `aws-lc-rs-fips` wins over `aws-lc-rs`.
//!
//! What stays outside: [`crate::sign::v4a::generate_signing_key`] assembles the derivation's
//! fixed-info string and applies the FIPS 186-5 A.2.2 candidate test to the result with
//! `crypto-bigint`. Neither needs a crypto primitive, so on an AWS-LC backend the KDF block runs
//! inside the module while the loop around it stays in Rust.

/// Size in bytes of a SHA-256 digest, and so of an HMAC-SHA256 tag.
pub(crate) const SHA256_OUTPUT_SIZE: usize = 32;

// `--cfg aws_sdk_fips_only` drops the RustCrypto dependencies, so that configuration needs an
// AWS-LC arm to have any crypto at all. Say so here, so the failure names the fix ahead of the
// unresolved `crypto::` imports it also causes at the call sites.
#[cfg(all(aws_sdk_fips_only, not(feature = "__aws-lc-rs")))]
compile_error!(
    "aws-sigv4 was built with `--cfg aws_sdk_fips_only`, which removes the RustCrypto backend, but \
     no AWS-LC backend is enabled. Enable the `aws-lc-rs` or `aws-lc-rs-fips` feature, or drop the \
     cfg."
);

#[cfg(feature = "__aws-lc-rs")]
mod aws_lc_rs_impl;
#[cfg(feature = "__aws-lc-rs")]
pub(crate) use aws_lc_rs_impl::HmacSha256;

// Only the selected backend's module is compiled: the two `HmacSha256` re-exports, and the two
// `imp` aliases below, would otherwise collide.
#[cfg(all(not(aws_sdk_fips_only), not(feature = "__aws-lc-rs")))]
mod rustcrypto_impl;
#[cfg(all(not(aws_sdk_fips_only), not(feature = "__aws-lc-rs")))]
pub(crate) use rustcrypto_impl::HmacSha256;

pub(crate) use imp::sha256;
#[cfg(feature = "sigv4a")]
pub(crate) use imp::{ecdsa_p256_sha256_sign_der, kdf_ctr_hmac_sha256};

#[cfg(feature = "__aws-lc-rs")]
use aws_lc_rs_impl as imp;
#[cfg(all(not(aws_sdk_fips_only), not(feature = "__aws-lc-rs")))]
use rustcrypto_impl as imp;

#[cfg(test)]
mod tests {
    use super::{sha256, HmacSha256, SHA256_OUTPUT_SIZE};

    // Known-answer tests, so whichever backend a build selects is held to the same vectors.
    // `--all-features` legs cover AWS-LC, `additional-ci` the default backend.
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
        // Split across two `update` calls so that incremental feeding is covered too.
        let mut hmac = HmacSha256::new(RFC4231_TC2_KEY);
        hmac.update(b"what do ya want ");
        hmac.update(b"for nothing?");
        assert_eq!(RFC4231_TC2_TAG, hex::encode(hmac.finalize()));
    }

    // Verifies rather than comparing bytes, because AWS-LC randomizes the ECDSA nonce and so has
    // no fixed expected signature. `p256` is the verifier on both backends, which makes this a
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

    // Handing the SigV4a derivation to AWS-LC's KDF rests on one claim: with the counter first, a
    // single block is HMAC(secret, [1]_32 || info). Hold AWS-LC's KDF to its own HMAC to keep that
    // claim honest, since it is what makes the two backends derive the same key.
    //
    // Gated to AWS-LC because on the RustCrypto backend the right-hand side *is* the
    // implementation. The derived key itself is pinned by
    // `sign::v4a::tests::derived_key_matches_the_published_test_vector`, on both backends.
    #[test]
    #[cfg(all(feature = "sigv4a", feature = "__aws-lc-rs"))]
    fn kdf_one_block_matches_its_prf() {
        let secret = b"AWS4AwJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        // Shaped like the fixed-info string the derivation builds: the algorithm label, a zero
        // byte, the access key, the candidate counter, then the output length in bits.
        let info = b"AWS4-ECDSA-P256-SHA256\x00AKIDEXAMPLE\x01\x00\x00\x01\x00";

        let mut hmac = HmacSha256::new(secret);
        hmac.update(&1i32.to_be_bytes());
        hmac.update(info);

        assert_eq!(
            hex::encode(hmac.finalize()),
            hex::encode(super::kdf_ctr_hmac_sha256(secret, info)),
            "AWS-LC's counter-mode KDF no longer agrees with its HMAC over one block"
        );
    }

    // Whenever an aws-lc-rs feature is enabled it must win, even though the RustCrypto crates are
    // still compiled. The module path of `HmacSha256` is the observable signal, since only the
    // selected backend's module compiles.
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

    // `aws-lc-rs-fips` is only meaningful if it links a FIPS build of AWS-LC, which is a property
    // of the linked C library rather than of this crate. Which module that build is, and whether
    // that module is certified, is a version question `FIPS_mode()` does not answer.
    #[test]
    #[cfg(feature = "aws-lc-rs-fips")]
    fn fips_module_is_active() {
        aws_lc_rs::try_fips_mode()
            .expect("the `aws-lc-rs-fips` feature must link the FIPS AWS-LC build");
    }
}

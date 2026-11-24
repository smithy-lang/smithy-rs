/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Internal abstraction over the digest implementations this crate can be built against.
//!
//! Two backends, selected at compile time: [RustCrypto](https://github.com/RustCrypto/hashes)
//! (the default, not FIPS-validated) and [aws-lc-rs](https://github.com/aws/aws-lc-rs)
//! (`aws-lc-rs`, or `aws-lc-rs-fips` for the FIPS 140-3 validated build of AWS-LC). aws-lc-rs wins
//! if both are enabled, and `aws-lc-rs-fips` wins over `aws-lc-rs`.
//!
//! MD5 is RustCrypto-only: aws-lc-rs does not expose it and it is not FIPS-approved. No public API
//! reaches it, since [`crate::ChecksumAlgorithm::Md5`] is deprecated and resolves to CRC-32.

use bytes::Bytes;
use std::fmt::Debug;

/// A streaming message digest.
///
/// This mirrors the shape the [`crate::Checksum`] implementations need: construct with
/// [`Default`], feed bytes with [`Digest::update`], and consume with [`Digest::finalize`].
pub(crate) trait Digest: Default + Debug + Send + Sync {
    /// Update this digest's internal state with `bytes`.
    fn update(&mut self, bytes: &[u8]);

    /// Consume this digest, returning the calculated value.
    fn finalize(self) -> Bytes;

    /// The size, in bytes, of the value returned by [`Digest::finalize`].
    fn output_size() -> u64;
}

// `--cfg aws_sdk_fips_only` drops the RustCrypto dependencies, so that configuration needs an
// AWS-LC arm to have any digests at all. Say so here, so the failure names the fix ahead of the
// unresolved `crypto::` imports it also causes at the call sites.
#[cfg(all(aws_sdk_fips_only, not(feature = "__aws-lc-rs")))]
compile_error!(
    "aws-smithy-checksums was built with `--cfg aws_sdk_fips_only`, which removes the RustCrypto \
     backend, but no AWS-LC backend is enabled. Enable the `aws-lc-rs` or `aws-lc-rs-fips` \
     feature, or drop the cfg."
);

#[cfg(feature = "__aws-lc-rs")]
mod aws_lc_rs_impl;
#[cfg(feature = "__aws-lc-rs")]
pub(crate) use aws_lc_rs_impl::{Sha1, Sha256};

// Only the selected backend's module is compiled: the digest re-exports below, and the two `imp`
// aliases, would otherwise collide.
#[cfg(all(not(aws_sdk_fips_only), not(feature = "__aws-lc-rs")))]
mod rustcrypto_impl;
#[cfg(all(not(aws_sdk_fips_only), not(feature = "__aws-lc-rs")))]
pub(crate) use rustcrypto_impl::{Md5, Sha1, Sha256};

#[cfg(test)]
mod tests {
    use super::{Digest, Sha1, Sha256};

    // Known-answer tests, so whichever backend a build selects is held to the same vectors. Note
    // that CI only ever runs them against AWS-LC, since it tests with `--all-features`. The inputs are
    // split across two `update` calls to exercise the multi-step path.
    const INPUT_HEAD: &[u8] = b"test ";
    const INPUT_TAIL: &[u8] = b"data";
    const SHA1_OF_TEST_DATA: &str = "f48dd853820860816c75d54d0f584dc863327a7c";
    const SHA256_OF_TEST_DATA: &str =
        "916f0027a575074ce72a331777c3478d6513f786a591bd892da1a577bf2335f9";

    fn digest_of<D: Digest>() -> String {
        let mut digest = D::default();
        digest.update(INPUT_HEAD);
        digest.update(INPUT_TAIL);
        hex::encode(digest.finalize())
    }

    #[test]
    fn sha1_known_answer() {
        assert_eq!(SHA1_OF_TEST_DATA, digest_of::<Sha1>());
        assert_eq!(20, Sha1::output_size());
    }

    #[test]
    fn sha256_known_answer() {
        assert_eq!(SHA256_OF_TEST_DATA, digest_of::<Sha256>());
        assert_eq!(32, Sha256::output_size());
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

    // Whenever an aws-lc-rs feature is enabled it must win, even though the RustCrypto crates are
    // still compiled. The module path of `Sha256` is the observable signal, since only the
    // selected backend's module compiles.
    #[test]
    fn expected_backend_is_selected() {
        let selected = std::any::type_name::<Sha256>();
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
}

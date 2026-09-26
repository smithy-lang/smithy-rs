/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Internal abstraction over the cryptographic digest implementations this crate can be
//! built against.
//!
//! Two backends are supported, selected at compile time:
//! - [RustCrypto](https://github.com/RustCrypto/hashes) (`rustcrypto`, enabled by default). Not
//!   FIPS-validated.
//! - [aws-lc-rs](https://github.com/aws/aws-lc-rs) (`aws-lc-rs`, or `aws-lc-rs-fips` for the
//!   FIPS 140-3 validated build of AWS-LC; `aws-lc-rs-fips` wins if both are enabled).
//!
//! If the features for both backends are enabled, aws-lc-rs is selected. MD5 is only available
//! on the RustCrypto backend: aws-lc-rs does not expose it, and it is not a FIPS-approved
//! algorithm. Nothing in this crate reaches MD5 through a public API, since
//! [`crate::ChecksumAlgorithm::Md5`] is deprecated and resolves to CRC-32.

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

#[cfg(feature = "__aws-lc-rs")]
mod aws_lc_rs_impl;
#[cfg(feature = "__aws-lc-rs")]
pub(crate) use aws_lc_rs_impl::{Sha1, Sha256};

// The RustCrypto backend is only compiled when it is the selected backend, so that enabling
// `aws-lc-rs` alongside the default features doesn't pull RustCrypto into the digest path.
#[cfg(not(feature = "__aws-lc-rs"))]
mod rustcrypto_impl;
#[cfg(not(feature = "__aws-lc-rs"))]
pub(crate) use rustcrypto_impl::{Md5, Sha1, Sha256};

#[cfg(test)]
mod tests {
    use super::{Digest, Sha1, Sha256};

    // Known-answer tests, so that both backends are held to the same vectors. The inputs are
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

    // The `aws-lc-rs-fips` feature is only meaningful if the AWS-LC build it selects is actually
    // the validated one, which is a property of the linked C library rather than of this crate.
    #[test]
    #[cfg(feature = "aws-lc-rs-fips")]
    fn fips_module_is_active() {
        aws_lc_rs::try_fips_mode()
            .expect("the `aws-lc-rs-fips` feature must link the FIPS AWS-LC build");
    }

    // Whenever the aws-lc-rs feature is enabled it must win, even if `rustcrypto` is also
    // enabled (which it is by default). The backend modules are cfg'd so that only the
    // selected one is compiled, so the module path of `Sha256` is the observable signal.
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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [aws-lc-rs](https://github.com/aws/aws-lc-rs) digest backend.
//!
//! With the `fips` feature, these digests are computed by the FIPS 140-3 validated build of
//! AWS-LC. With `aws-lc-rs` alone they are computed by the same implementations in a build that
//! is not operating in FIPS mode.

use super::Digest;
use aws_lc_rs::digest;
use bytes::Bytes;
use std::fmt;

/// Implements [`Digest`] by delegating to an aws-lc-rs [`digest::Context`].
///
/// `digest::Context` is neither `Debug` nor `Default`, so both are provided here. `Context::new`
/// panics if AWS-LC fails to initialize the digest, which is the same failure mode as the rest
/// of the aws-lc-rs API and is not recoverable.
macro_rules! aws_lc_rs_digest {
    ($name:ident => $algorithm:expr, $output_len:expr) => {
        pub(crate) struct $name {
            context: digest::Context,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    context: digest::Context::new($algorithm),
                }
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // `digest::Context` holds AWS-LC state that can't be formatted, and a digest's
                // in-progress state isn't something we want to print anyway.
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }

        impl Digest for $name {
            fn update(&mut self, bytes: &[u8]) {
                self.context.update(bytes);
            }

            fn finalize(self) -> Bytes {
                Bytes::copy_from_slice(self.context.finish().as_ref())
            }

            fn output_size() -> u64 {
                $output_len as u64
            }
        }
    };
}

// SHA-1 is not approved for digital signatures under FIPS, but remains available for other
// uses such as the checksums computed here; AWS-LC exposes it under a name that spells out
// that caveat.
aws_lc_rs_digest!(Sha1 => &digest::SHA1_FOR_LEGACY_USE_ONLY, digest::SHA1_OUTPUT_LEN);
aws_lc_rs_digest!(Sha256 => &digest::SHA256, digest::SHA256_OUTPUT_LEN);

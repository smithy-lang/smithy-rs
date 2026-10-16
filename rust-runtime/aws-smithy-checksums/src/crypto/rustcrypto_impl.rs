/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [RustCrypto](https://github.com/RustCrypto/hashes) digest backend. Not FIPS-validated.

use super::Digest;
use bytes::Bytes;

/// Implements [`Digest`] by delegating to a RustCrypto hasher.
///
/// `$rc_digest` is the `Digest` trait as re-exported by the hasher's own crate, which is how
/// the RustCrypto hashers expose `update`/`finalize`/`output_size`.
macro_rules! rustcrypto_digest {
    ($(#[$attrs:meta])* $name:ident => $hasher:ty, $rc_digest:path) => {
        $(#[$attrs])*
        #[derive(Debug, Default)]
        pub(crate) struct $name {
            hasher: $hasher,
        }

        impl Digest for $name {
            fn update(&mut self, bytes: &[u8]) {
                use $rc_digest as _;
                self.hasher.update(bytes);
            }

            fn finalize(self) -> Bytes {
                use $rc_digest as _;
                Bytes::copy_from_slice(self.hasher.finalize().as_ref())
            }

            fn output_size() -> u64 {
                use $rc_digest as _;
                <$hasher>::output_size() as u64
            }
        }
    };
}

rustcrypto_digest!(Sha1 => sha1::Sha1, sha1::Digest);
rustcrypto_digest!(Sha256 => sha2::Sha256, sha2::Digest);
rustcrypto_digest!(
    /// MD5 is only available on this backend; see the [module docs](super) for why.
    Md5 => md5::Md5, md5::Digest
);

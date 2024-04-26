/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    // missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

//! Compression-related code.

use crate::error::UnknownCompressionAlgorithmError;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use std::io::Write;
use std::str::FromStr;

pub mod body;
pub mod error;
mod gzip;
pub mod http;

// Valid compression algorithm names
pub const GZIP_NAME: &str = "gzip";

/// Types implementing this trait can compress data.
///
/// Compression algorithms are used reduce the size of data. This trait
/// requires Send + Sync because trait implementors are often used in an
/// async context.
pub trait Compression: Send + Sync {
    /// Given a slice of bytes, and a [Write] implementor, compress and write
    /// bytes to the writer until done.
    // I wanted to use `impl Write` but that's not object-safe
    fn compress_bytes(&mut self, bytes: &[u8], writer: &mut dyn Write) -> Result<(), BoxError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompressionOptions {
    /// Valid values are 0-9 with lower values configuring less (but faster) compression
    level: u32,
    min_compression_size_bytes: u32,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            level: 6,
            min_compression_size_bytes: 10240,
        }
    }
}

impl CompressionOptions {
    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn min_compression_size_bytes(&self) -> u32 {
        self.min_compression_size_bytes
    }

    pub fn with_level(self, level: u32) -> Result<Self, BoxError> {
        Self::validate_level(level)?;
        Ok(Self { level, ..self })
    }

    pub fn with_min_compression_size_bytes(
        self,
        min_compression_size_bytes: u32,
    ) -> Result<Self, BoxError> {
        Self::validate_min_compression_size_bytes(min_compression_size_bytes)?;
        Ok(Self {
            min_compression_size_bytes,
            ..self
        })
    }

    fn validate_level(level: u32) -> Result<(), BoxError> {
        if level > 9 {
            return Err(format!(
                "compression level `{}` is invalid, valid values are 0-9",
                level
            )
            .into());
        };
        Ok(())
    }

    fn validate_min_compression_size_bytes(
        min_compression_size_bytes: u32,
    ) -> Result<(), BoxError> {
        if min_compression_size_bytes > 10485760 {
            return Err(format!(
                "min compression size `{}` is invalid, valid values are 0-10485760",
                min_compression_size_bytes
            )
            .into());
        };
        Ok(())
    }
}

impl Storable for CompressionOptions {
    type Storer = StoreReplace<Self>;
}

/// We only support compression calculation and validation for these compression algorithms.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    Gzip,
}

impl FromStr for CompressionAlgorithm {
    type Err = UnknownCompressionAlgorithmError;

    /// Create a new `CompressionAlgorithm` from an algorithm name.
    ///
    /// Valid algorithm names are:
    /// - "gzip"
    ///
    /// Passing an invalid name will return an error.
    fn from_str(compression_algorithm: &str) -> Result<Self, Self::Err> {
        if compression_algorithm.eq_ignore_ascii_case(GZIP_NAME) {
            Ok(Self::Gzip)
        } else {
            Err(UnknownCompressionAlgorithmError::new(compression_algorithm))
        }
    }
}

impl CompressionAlgorithm {
    /// Return the `HttpChecksum` implementor for this algorithm
    pub fn into_impl(self, options: &CompressionOptions) -> Box<dyn http::RequestCompressor> {
        match self {
            Self::Gzip => Box::new(gzip::Gzip::from(options)),
        }
    }

    /// Return the name of this algorithm in string form
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gzip { .. } => GZIP_NAME,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::CompressionAlgorithm;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_compression_algorithm_from_str_unknown() {
        let error = "some unknown compression algorithm"
            .parse::<CompressionAlgorithm>()
            .expect_err("it should error");
        assert_eq!(
            "some unknown compression algorithm",
            error.compression_algorithm()
        );
    }

    #[test]
    fn test_compression_algorithm_from_str_gzip() {
        let algo = "gzip".parse::<CompressionAlgorithm>().unwrap();
        assert_eq!("gzip", algo.as_str());
    }
}

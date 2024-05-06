/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors that can occur when using this crate.

use std::error::Error;
use std::fmt;

/// A compression algorithm was unknown
#[derive(Debug)]
pub struct UnknownCompressionAlgorithmError {
    compression_algorithm: String,
}

impl UnknownCompressionAlgorithmError {
    pub(crate) fn new(compression_algorithm: impl Into<String>) -> Self {
        Self {
            compression_algorithm: compression_algorithm.into(),
        }
    }

    /// The compression algorithm that is unknown
    pub fn compression_algorithm(&self) -> &str {
        &self.compression_algorithm
    }
}

impl fmt::Display for UnknownCompressionAlgorithmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            r#"unknown compression algorithm "{}", please pass a known algorithm name ("gzip")"#,
            self.compression_algorithm
        )
    }
}

impl Error for UnknownCompressionAlgorithmError {}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//TODO(runtimeCratesVersioningCleanup): Keep the following deprecated type aliases for at least
// one release since 0.56.1 and then remove this module.

//! ByteStream Abstractions

/// Non-contiguous Binary Data Storage
#[deprecated(note = "Moved to `aws_smithy_types::byte_stream::AggregatedBytes`.")]
pub type AggregatedBytes = aws_smithy_types::byte_stream::AggregatedBytes;

/// Stream of binary data
#[deprecated(note = "Moved to `aws_smithy_types::byte_stream::ByteStream`.")]
pub type ByteStream = aws_smithy_types::byte_stream::ByteStream;

/// Errors related to bytestreams.
pub mod error {
    /// An error occurred in the byte stream
    #[deprecated(note = "Moved to `aws_smithy_types::byte_stream::error::Error`.")]
    pub type Error = aws_smithy_types::byte_stream::error::Error;
}

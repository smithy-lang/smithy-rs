/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! ByteStream Abstractions

//TODO(runtimeCratesVersioningCleanup): Re-point those who use the following reexports to
// `aws_smithy_types` and remove the `byte_stream` module.

pub use aws_smithy_types::byte_stream::{AggregatedBytes, ByteStream};

/// Errors related to bytestreams.
pub mod error {
    pub use aws_smithy_types::byte_stream::error::Error;
}

#[cfg(feature = "rt-tokio")]
pub use aws_smithy_types::byte_stream::FsBuilder;

#[cfg(feature = "rt-tokio")]
pub use aws_smithy_types::byte_stream::Length;

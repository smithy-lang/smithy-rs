/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//TODO(runtimeCratesVersioningCleanup): Keep the following deprecated type aliases for at least
// one release since 0.56.1 and then remove this module.

//! Types for representing the body of an HTTP request or response

/// A boxed generic HTTP body that, when consumed, will result in [`Bytes`](bytes::Bytes) or an [`Error`](aws_smithy_types::body::Error).
#[deprecated(note = "Moved to `aws_smithy_types::body::BoxBody`.")]
pub type BoxBody = aws_smithy_types::body::BoxBody;

/// A generic, boxed error that's `Send` and `Sync`
#[deprecated(note = "`Moved to `aws_smithy_types::body::Error`.")]
pub type Error = aws_smithy_types::body::Error;

/// SdkBody type
#[deprecated(note = "Moved to `aws_smithy_types::body::SdkBody`.")]
pub type SdkBody = aws_smithy_types::body::SdkBody;

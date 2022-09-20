/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::header::ToStrError;
use thiserror::Error;

/// An AWS JSON routing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Relative URI was not "/".
    #[error("relative URI is not \"/\"")]
    NotRootUrl,
    /// Method was not `POST`.
    #[error("method not POST")]
    MethodNotAllowed,
    /// Missing the `x-amz-target` header.
    #[error("missing the \"x-amz-target\" header")]
    MissingHeader,
    /// Unable to parse header into UTF-8.
    #[error("failed to parse header: {0}")]
    InvalidHeader(ToStrError),
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
}

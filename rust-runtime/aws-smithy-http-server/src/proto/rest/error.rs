/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use thiserror::Error;

/// An AWS REST routing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
    /// Method was not allowed.
    #[error("method was not allowed")]
    MethodNotAllowed,
}

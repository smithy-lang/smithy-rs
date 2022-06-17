/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python error definition.

use thiserror::Error;

/// Python error that implements foreign errors.
#[derive(Error, Debug)]
pub enum Error {
    /// Custom error.
    #[error("{0}")]
    Custom(String),
    /// Errors coming from `pyo3::PyErr`.
    #[error("PyO3 error: {0}")]
    PyO3(#[from] pyo3::PyErr),
    /// Error coming from `tokio::task::JoinError`.
    #[error("Tokio task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}

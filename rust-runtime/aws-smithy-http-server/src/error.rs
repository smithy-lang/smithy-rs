/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Error definition.

// This file is a copy of Axum error.rs (https://github.com/tokio-rs/axum/blob/2507463706d0cea90007b5959c579a32d4b24cc4/axum/src/error.rs#L1)
// Axum original license can be found inside the file named LICENSE.mit.
use crate::BoxError;
use std::{error::Error as StdError, fmt};

/// Errors that can happen when using `aws-smithy-http-server`.
#[derive(Debug)]
pub struct Error {
    pub inner: BoxError,
}

impl Error {
    pub fn new(error: impl Into<BoxError>) -> Self {
        Self { inner: error.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.inner)
    }
}

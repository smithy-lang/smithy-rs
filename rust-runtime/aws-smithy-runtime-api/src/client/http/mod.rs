/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Http Request / Response Types

/// Http Request Types
pub mod request;

/// Http Response Types
pub mod response;

use crate::box_error::BoxError;
pub use request::Request;
use std::error::{Error as StdError, Error};
use std::fmt::{Display, Formatter};
use std::ops::Deref;

#[derive(Debug)]
/// An error occurred constructing an Http Request.
///
/// This is normally due to configuration issues, internal SDK bugs, or other user error.
pub struct HttpError(BoxError);

impl HttpError {
    fn new<E: Into<Box<dyn Error + Send + Sync + 'static>>>(err: E) -> Self {
        HttpError(err.into())
    }

    fn invalid_header_value(err: impl Into<BoxError>) -> Self {
        // TODO: better error internals
        Self(err.into())
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "An error occurred creating an HTTP Request")
    }
}

impl Deref for HttpError {
    type Target = dyn StdError + Send + Sync + 'static;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl AsRef<dyn StdError + Send + Sync> for HttpError {
    fn as_ref(&self) -> &(dyn StdError + Send + Sync + 'static) {
        &**self
    }
}

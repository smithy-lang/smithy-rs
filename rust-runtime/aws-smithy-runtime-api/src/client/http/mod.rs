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
use http::header::{InvalidHeaderName, InvalidHeaderValue, ToStrError};
use http::uri::InvalidUri;
pub use request::Request;
use std::error::{Error as StdError, Error};
use std::fmt::{Display, Formatter};

#[derive(Debug)]
/// An error occurred constructing an Http Request.
///
/// This is normally due to configuration issues, internal SDK bugs, or other user error.
pub struct HttpError(BoxError);

impl HttpError {
    // TODO(httpRefactor): Add better error internals
    fn new<E: Into<Box<dyn Error + Send + Sync + 'static>>>(err: E) -> Self {
        HttpError(err.into())
    }

    fn invalid_header_value(err: InvalidHeaderValue) -> Self {
        Self(err.into())
    }

    fn header_was_not_a_string(err: ToStrError) -> Self {
        Self(err.into())
    }

    fn invalid_header_name(err: InvalidHeaderName) -> Self {
        Self(err.into())
    }

    fn invalid_uri(err: InvalidUri) -> Self {
        Self(err.into())
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "An error occurred creating an HTTP Request")
    }
}

impl Error for HttpError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.0.as_ref())
    }
}

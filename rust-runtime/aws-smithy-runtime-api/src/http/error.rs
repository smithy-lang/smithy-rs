/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Error types for HTTP requests/responses.

use crate::box_error::BoxError;
use http_02x::header::{InvalidHeaderName, InvalidHeaderValue};
use http_02x::uri::InvalidUri;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::str::Utf8Error;

#[derive(Debug)]
/// An error occurred constructing an Http Request.
///
/// This is normally due to configuration issues, internal SDK bugs, or other user error.
pub struct HttpError {
    kind: Kind,
    source: Option<BoxError>,
}

#[derive(Debug)]
enum Kind {
    InvalidExtensions,
    InvalidHeaderName,
    InvalidHeaderValue,
    InvalidStatusCode,
    InvalidUri,
    InvalidUriParts,
    MissingAuthority,
    MissingScheme,
    NotUtf8(Vec<u8>),
}

impl HttpError {
    pub(super) fn invalid_extensions() -> Self {
        Self {
            kind: Kind::InvalidExtensions,
            source: None,
        }
    }

    pub(super) fn invalid_header_name(err: InvalidHeaderName) -> Self {
        Self {
            kind: Kind::InvalidHeaderName,
            source: Some(Box::new(err)),
        }
    }

    pub(super) fn invalid_header_value(err: InvalidHeaderValue) -> Self {
        Self {
            kind: Kind::InvalidHeaderValue,
            source: Some(Box::new(err)),
        }
    }

    pub(super) fn invalid_status_code() -> Self {
        Self {
            kind: Kind::InvalidStatusCode,
            source: None,
        }
    }

    pub(super) fn invalid_uri(err: InvalidUri) -> Self {
        Self {
            kind: Kind::InvalidUri,
            source: Some(Box::new(err)),
        }
    }

    pub(super) fn invalid_uri_parts(err: http_02x::Error) -> Self {
        Self {
            kind: Kind::InvalidUriParts,
            source: Some(Box::new(err)),
        }
    }

    pub(super) fn missing_authority() -> Self {
        Self {
            kind: Kind::MissingAuthority,
            source: None,
        }
    }

    pub(super) fn missing_scheme() -> Self {
        Self {
            kind: Kind::MissingScheme,
            source: None,
        }
    }

    pub(super) fn not_utf8(header_value_bytes: &[u8]) -> Self {
        Self {
            kind: Kind::NotUtf8(header_value_bytes.to_owned()),
            source: None,
        }
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use Kind::*;
        match &self.kind {
            InvalidExtensions => write!(f, "Extensions were provided during initialization. This prevents the request format from being converted."),
            InvalidHeaderName => write!(f, "invalid header name"),
            InvalidHeaderValue => write!(f, "invalid header value"),
            InvalidStatusCode => write!(f, "invalid HTTP status code"),
            InvalidUri => write!(f, "endpoint is not a valid URI"),
            InvalidUriParts => write!(f, "endpoint parts are not valid"),
            MissingAuthority => write!(f, "endpoint must contain authority"),
            MissingScheme => write!(f, "endpoint must contain scheme"),
            NotUtf8(hv) => {
                let utf8_err: Utf8Error = std::str::from_utf8(hv).expect_err("we've already proven the value is not UTF-8");
                let hv = String::from_utf8_lossy(hv);
                let problem_char = utf8_err.valid_up_to();
                write!(f, "header value `{hv}` contains non-UTF8 octet at index {problem_char}")
            },
        }
    }
}

impl Error for HttpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|err| err.as_ref() as _)
    }
}

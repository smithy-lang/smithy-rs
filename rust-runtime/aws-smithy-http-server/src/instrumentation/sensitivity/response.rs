/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A builder whose methods allow for configuration of [`MakeFmt`] implementations over parts of [`http::Response`].

use std::fmt::{Debug, Error, Formatter};

use http;

use crate::http::{header::HeaderName, HeaderMap};

use crate::instrumentation::{MakeFmt, MakeIdentity};

use super::{
    headers::{HeaderMarker, MakeHeaders},
    MakeSensitive,
};

/// Allows the modification the responses status code [`Display`](std::fmt::Display) and headers
/// [`Debug`] to accommodate sensitivity.
///
/// This enjoys [`MakeFmt`] for [`&HeaderMap`](HeaderMap) and [`StatusCode`](http::StatusCode).
#[derive(Clone)]
pub struct ResponseFmt<Headers, StatusCode> {
    headers: Headers,
    status_code: StatusCode,
}

impl<Headers, StatusCode> Debug for ResponseFmt<Headers, StatusCode> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_struct("ResponseFmt").finish_non_exhaustive()
    }
}

/// Default [`ResponseFmt`].
pub type DefaultResponseFmt = ResponseFmt<MakeIdentity, MakeIdentity>;

impl Default for DefaultResponseFmt {
    fn default() -> Self {
        Self {
            headers: MakeIdentity,
            status_code: MakeIdentity,
        }
    }
}

impl DefaultResponseFmt {
    /// Constructs a new [`ResponseFmt`] with no redactions.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<Header, StatusCode> ResponseFmt<Header, StatusCode> {
    /// Marks headers as sensitive using a closure.
    ///
    /// See [`SensitiveHeaders`](super::headers::SensitiveHeaders) for more info.
    pub fn header<F>(self, header: F) -> ResponseFmt<MakeHeaders<F>, StatusCode>
    where
        F: Fn(&HeaderName) -> HeaderMarker,
    {
        ResponseFmt {
            headers: MakeHeaders(header),
            status_code: self.status_code,
        }
    }

    /// Marks request status code as sensitive.
    pub fn status_code(self) -> ResponseFmt<Header, MakeSensitive> {
        ResponseFmt {
            headers: self.headers,
            status_code: MakeSensitive,
        }
    }
}

impl<'a, Headers, StatusCode> MakeFmt<&'a HeaderMap> for ResponseFmt<Headers, StatusCode>
where
    Headers: MakeFmt<&'a HeaderMap>,
{
    type Target = Headers::Target;

    fn make(&self, source: &'a HeaderMap) -> Self::Target {
        self.headers.make(source)
    }
}

impl<Headers, StatusCode> MakeFmt<http::StatusCode> for ResponseFmt<Headers, StatusCode>
where
    StatusCode: MakeFmt<http::StatusCode>,
{
    type Target = StatusCode::Target;

    fn make(&self, source: http::StatusCode) -> Self::Target {
        self.status_code.make(source)
    }
}

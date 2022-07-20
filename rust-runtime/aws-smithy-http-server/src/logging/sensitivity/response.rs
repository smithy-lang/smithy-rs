/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Debug, Error, Formatter};

use http::{header::HeaderName, HeaderMap};

use crate::logging::{MakeFmt, MakeIdentity};

use super::{
    headers::{HeaderMarker, MakeHeaders},
    MakeSensitive,
};

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

impl Default for ResponseFmt<MakeIdentity, MakeIdentity> {
    fn default() -> Self {
        Self {
            headers: MakeIdentity,
            status_code: MakeIdentity,
        }
    }
}

impl ResponseFmt<MakeIdentity, MakeIdentity> {
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

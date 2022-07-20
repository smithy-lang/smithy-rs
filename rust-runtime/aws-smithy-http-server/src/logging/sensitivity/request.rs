/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Debug, Error, Formatter};

use http::{header::HeaderName, HeaderMap};

use crate::logging::{MakeFmt, MakeIdentity};

use super::{
    headers::{HeaderMarker, MakeHeaders},
    uri::{MakeGreedyLabel, MakeLabel, MakeQuery, MakeUri, QueryMarker},
};

/// Allows the modification the requests URIs [`Display`](std::fmt::Display) and headers
/// [`Debug`](std::fmt::Debug) to accommodate sensitivity.
#[derive(Clone)]
pub struct RequestFmt<Headers, Uri> {
    headers: Headers,
    uri: Uri,
}

impl<Headers, Uri> Debug for RequestFmt<Headers, Uri> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_struct("RequestFmt").finish_non_exhaustive()
    }
}

impl Default for RequestFmt<MakeIdentity, MakeUri<MakeIdentity, MakeIdentity>> {
    fn default() -> Self {
        Self {
            headers: MakeIdentity,
            uri: MakeUri::default(),
        }
    }
}

impl RequestFmt<MakeIdentity, MakeUri<MakeIdentity, MakeIdentity>> {
    /// Constructs a new [`RequestFmt`] with no redactions.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<Header, Uri> RequestFmt<Header, Uri> {
    /// Marks parts of headers as sensitive using a closure.
    ///
    /// See [`SensitiveHeaders`](super::headers::SensitiveHeaders) for more info.
    pub fn header<F>(self, headers: F) -> RequestFmt<MakeHeaders<F>, Uri>
    where
        F: Fn(&HeaderName) -> HeaderMarker,
    {
        RequestFmt {
            headers: MakeHeaders(headers),
            uri: self.uri,
        }
    }
}

impl<Header, P, Q> RequestFmt<Header, MakeUri<P, Q>> {
    /// Marks parts of the URI as sensitive.
    ///
    /// See [`Label`](super::uri::Label) for more info.
    pub fn label<F>(self, label: F) -> RequestFmt<Header, MakeUri<MakeLabel<F>, Q>>
    where
        F: Fn(usize) -> bool,
    {
        RequestFmt {
            headers: self.headers,
            uri: MakeUri {
                make_path: MakeLabel(label),
                make_query: self.uri.make_query,
            },
        }
    }

    /// Marks parts of the URI as sensitive.
    ///
    /// See [`GreedyLabel`](super::uri::GreedyLabel) for more info.
    pub fn greedy_label(self, position: usize) -> RequestFmt<Header, MakeUri<MakeGreedyLabel, Q>> {
        RequestFmt {
            headers: self.headers,
            uri: MakeUri {
                make_path: MakeGreedyLabel(position),
                make_query: self.uri.make_query,
            },
        }
    }

    /// Marks parts of the query as sensitive.
    ///
    /// See [`Query`](super::uri::Query) for more info.
    pub fn query<F>(self, query: F) -> RequestFmt<Header, MakeUri<P, MakeQuery<F>>>
    where
        F: Fn(&str) -> QueryMarker,
    {
        RequestFmt {
            headers: self.headers,
            uri: MakeUri {
                make_path: self.uri.make_path,
                make_query: MakeQuery(query),
            },
        }
    }
}

impl<'a, Headers, Uri> MakeFmt<&'a HeaderMap> for RequestFmt<Headers, Uri>
where
    Headers: MakeFmt<&'a HeaderMap>,
{
    type Target = Headers::Target;

    fn make(&self, source: &'a HeaderMap) -> Self::Target {
        self.headers.make(source)
    }
}

impl<'a, Headers, Uri> MakeFmt<&'a http::Uri> for RequestFmt<Headers, Uri>
where
    Uri: MakeFmt<&'a http::Uri>,
{
    type Target = Uri::Target;

    fn make(&self, source: &'a http::Uri) -> Self::Target {
        self.uri.make(source)
    }
}

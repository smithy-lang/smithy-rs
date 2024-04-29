/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checksum support for HTTP requests and responses.

use crate::Compression;
use http::header::{HeaderMap, HeaderName, HeaderValue};

/// Implementors of this trait can be used to compress HTTP requests.
pub trait RequestCompressor: Compression + Send + Sync {
    /// Return a map of headers that must be included when a request is compressed.
    fn headers(self: Box<Self>) -> HeaderMap<HeaderValue> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        header_map
    }

    /// Return the header name for the content-encoding header.
    fn header_name(&self) -> HeaderName {
        HeaderName::from_static("content-encoding")
    }

    /// Return the header value for the content-encoding header.
    fn header_value(self: Box<Self>) -> HeaderValue;
}

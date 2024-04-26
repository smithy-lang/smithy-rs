/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checksum support for HTTP requests and responses.

use crate::Compression;
use http::header::{HeaderMap, HeaderName, HeaderValue};

pub trait RequestCompressor: Compression + Send + Sync {
    fn headers(self: Box<Self>) -> HeaderMap<HeaderValue> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        header_map
    }

    fn header_name(&self) -> HeaderName {
        HeaderName::from_static("content-encoding")
    }

    fn header_value(self: Box<Self>) -> HeaderValue;
}

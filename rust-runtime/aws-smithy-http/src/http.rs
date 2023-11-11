/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types for abstracting over HTTP requests and responses.

use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::http::Headers;

/// Trait for accessing HTTP headers.
///
/// Useful for generic impls so that they can access headers via trait bounds.
pub trait HttpHeaders {
    /// Returns a reference to the associated header map.
    fn http_headers(&self) -> &Headers;

    /// Returns a mutable reference to the associated header map.
    fn http_headers_mut(&mut self) -> &mut Headers;
}

impl HttpHeaders for HttpResponse {
    fn http_headers(&self) -> &Headers {
        self.headers()
    }

    fn http_headers_mut(&mut self) -> &mut Headers {
        self.headers_mut()
    }
}

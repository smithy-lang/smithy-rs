/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2022 Tower Contributors
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

use async_trait::async_trait;
use http::{Extensions, HeaderMap, Request, Uri};

use crate::response::IntoResponse;

/// Trait for extracting information from requests.
///
/// A type implementing the `FromRequest` trait is used to handle and extract information from an async handler taking in a `RequestParts` as input.
#[async_trait]
pub trait FromRequest<B>: Sized {
    /// If the extractor fails it'll use this "rejection" type. A rejection is
    /// a kind of error that can be converted into a response.
    type Rejection: IntoResponse;

    /// Perform the extraction.
    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection>;
}

#[doc(hidden)]
#[derive(Debug)]
pub struct RequestParts<B> {
    uri: Uri,
    headers: Option<HeaderMap>,
    extensions: Option<Extensions>,
    body: Option<B>,
}

impl<B> RequestParts<B> {
    /// Create a new `RequestParts`.
    ///
    /// You generally shouldn't need to construct this type yourself, unless
    /// using extractors outside of axum for example to implement a
    /// [`tower::Service`].
    ///
    /// [`tower::Service`]: https://docs.rs/tower/lastest/tower/trait.Service.html
    #[doc(hidden)]
    pub fn new(req: Request<B>) -> Self {
        let (
            http::request::Parts {
                uri,
                headers,
                extensions,
                ..
            },
            body,
        ) = req.into_parts();

        RequestParts {
            uri,
            headers: Some(headers),
            extensions: Some(extensions),
            body: Some(body),
        }
    }

    /// Gets a reference to the request headers.
    ///
    /// Returns `None` if the headers has been taken by another extractor.
    #[doc(hidden)]
    pub fn headers(&self) -> Option<&HeaderMap> {
        self.headers.as_ref()
    }

    /// Takes the body out of the request, leaving a `None` in its place.
    #[doc(hidden)]
    pub fn take_body(&mut self) -> Option<B> {
        self.body.take()
    }

    /// Gets a reference the request URI.
    #[doc(hidden)]
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Gets a reference to the request extensions.
    ///
    /// Returns `None` if the extensions has been taken by another extractor.
    #[doc(hidden)]
    pub fn extensions(&self) -> Option<&Extensions> {
        self.extensions.as_ref()
    }
}

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

use std::{
    convert::Infallible,
    future::{ready, Future, Ready},
};

use futures_util::{
    future::{try_join, MapErr, MapOk, TryJoin},
    TryFutureExt,
};
use http::{request::Parts, Extensions, HeaderMap, Request, Uri};

use crate::{rejection::EitherRejection, response::IntoResponse};

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
            Parts {
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

/// Provides a protocol aware extraction from a [`Request`]. This borrows the
/// [`Parts`], in contrast to [`FromRequest`].
pub trait FromParts<Protocol>: Sized {
    type Rejection: IntoResponse<Protocol>;

    /// Extracts `self` from a [`Parts`] synchronously.
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}

impl<P> FromParts<P> for () {
    type Rejection = Infallible;

    fn from_parts(_parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(())
    }
}

impl<P, T> FromParts<P> for (T,)
where
    T: FromParts<P>,
{
    type Rejection = T::Rejection;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok((T::from_parts(parts)?,))
    }
}

impl<P, T1, T2> FromParts<P> for (T1, T2)
where
    T1: FromParts<P>,
    T2: FromParts<P>,
{
    type Rejection = EitherRejection<T1::Rejection, T2::Rejection>;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let t1 = T1::from_parts(parts).map_err(EitherRejection::Left)?;
        let t2 = T2::from_parts(parts).map_err(EitherRejection::Right)?;
        Ok((t1, t2))
    }
}

/// Provides a protocol aware extraction from a [`Request`]. This consumes the
/// [`Request`], in contrast to [`FromParts`].
pub trait FromRequest<Protocol, B>: Sized {
    type Rejection: IntoResponse<Protocol>;
    type Future: Future<Output = Result<Self, Self::Rejection>>;

    /// Extracts `self` from a [`Request`] asynchronously.
    fn from_request(request: Request<B>) -> Self::Future;
}

impl<P, B, T1> FromRequest<P, B> for (T1,)
where
    T1: FromRequest<P, B>,
{
    type Rejection = T1::Rejection;
    type Future = MapOk<T1::Future, fn(T1) -> (T1,)>;

    fn from_request(request: Request<B>) -> Self::Future {
        T1::from_request(request).map_ok(|t1| (t1,))
    }
}

impl<P, B, T1, T2> FromRequest<P, B> for (T1, T2)
where
    T1: FromRequest<P, B>,
    T2: FromParts<P>,
{
    type Rejection = EitherRejection<T1::Rejection, T2::Rejection>;
    type Future = TryJoin<MapErr<T1::Future, fn(T1::Rejection) -> Self::Rejection>, Ready<Result<T2, Self::Rejection>>>;

    fn from_request(request: Request<B>) -> Self::Future {
        let (mut parts, body) = request.into_parts();
        let t2_result = T2::from_parts(&mut parts).map_err(EitherRejection::Right);
        try_join(
            T1::from_request(Request::from_parts(parts, body)).map_err(EitherRejection::Left),
            ready(t2_result),
        )
    }
}

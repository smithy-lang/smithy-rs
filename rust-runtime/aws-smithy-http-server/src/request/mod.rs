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

//! Types and traits for extracting data from requests.
//!
//! See [Accessing Un-modelled data](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/from_parts.md)
//! a comprehensive overview.
//!
//! The following implementations exist:
//! * Tuples up to size 8, extracting each component.
//! * `Option<T>`: `Some(T)` if extracting `T` is successful, `None` otherwise.
//! * `Result<T, T::Rejection>`: `Ok(T)` if extracting `T` is successful, `Err(T::Rejection)` otherwise.
//!
//! when `T: FromParts`.
//!

use std::{
    convert::Infallible,
    future::{ready, Future, Ready},
};

use futures_util::{
    future::{try_join, MapErr, MapOk, TryJoin},
    TryFutureExt,
};

use http;

use crate::http::{request::Parts, Request, StatusCode};

use crate::{
    body::{empty, BoxBody},
    rejection::any_rejections,
    response::IntoResponse,
};

pub mod connect_info;
pub mod extension;
#[cfg(feature = "aws-lambda")]
#[cfg_attr(docsrs, doc(cfg(feature = "aws-lambda")))]
pub mod lambda;
#[cfg(feature = "request-id")]
#[cfg_attr(docsrs, doc(cfg(feature = "request-id")))]
pub mod request_id;

fn internal_server_error() -> http::Response<BoxBody> {
    let mut response = http::Response::new(empty());
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}

/// Provides a protocol aware extraction from a [`Request`]. This borrows the [`Parts`], in contrast to
/// [`FromRequest`] which consumes the entire [`http::Request`] including the body.
pub trait FromParts<Protocol>: Sized {
    /// The type of the extraction failures.
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

macro_rules! impl_from_parts {
    ($error_name:ident, $($var:ident),+) => (
        impl<P, $($var,)*> FromParts<P> for ($($var),*)
        where
            $($var: FromParts<P>,)*
        {
            type Rejection = any_rejections::$error_name<$($var::Rejection),*>;

            fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
                let tuple = (
                    $($var::from_parts(parts).map_err(any_rejections::$error_name::$var)?,)*
                );
                Ok(tuple)
            }
        }
    )
}

impl_from_parts!(Two, A, B);
impl_from_parts!(Three, A, B, C);
impl_from_parts!(Four, A, B, C, D);
impl_from_parts!(Five, A, B, C, D, E);
impl_from_parts!(Six, A, B, C, D, E, F);
impl_from_parts!(Seven, A, B, C, D, E, F, G);
impl_from_parts!(Eight, A, B, C, D, E, F, G, H);

/// Provides a protocol aware extraction from a [`Request`]. This consumes the
/// [`Request`], including the body, in contrast to [`FromParts`] which borrows the [`Parts`].
///
/// This should not be implemented by hand. Code generation should implement this for your operations input. To extract
/// items from a HTTP request [`FromParts`] should be used.
pub trait FromRequest<Protocol, B>: Sized {
    /// The type of the extraction failures.
    type Rejection: IntoResponse<Protocol>;
    /// The type of the extraction [`Future`].
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
    T1::Rejection: std::fmt::Display,
    T2::Rejection: std::fmt::Display,
{
    type Rejection = any_rejections::Two<T1::Rejection, T2::Rejection>;
    type Future = TryJoin<MapErr<T1::Future, fn(T1::Rejection) -> Self::Rejection>, Ready<Result<T2, Self::Rejection>>>;

    fn from_request(request: Request<B>) -> Self::Future {
        let (mut parts, body) = request.into_parts();
        let t2_result: Result<T2, any_rejections::Two<T1::Rejection, T2::Rejection>> = T2::from_parts(&mut parts)
            .map_err(|e| {
                // The error is likely caused by a failure to construct a parameter from the
                // `Request` required by the user handler. This typically occurs when the
                // user handler expects a specific type, such as `Extension<State>`, but
                // either the `ExtensionLayer` has not been added, or it adds a different
                // type to the extension bag, such as `Extension<Arc<State>>`.
                tracing::error!(
                    error = %e,
                    "additional parameter for the handler function could not be constructed");
                any_rejections::Two::B(e)
            });
        try_join(
            T1::from_request(Request::from_parts(parts, body)).map_err(|e| {
                // `T1`, the first parameter of a handler function, represents the input parameter
                // defined in the Smithy model. An error at this stage suggests that `T1` could not
                // be constructed from the `Request`.
                tracing::debug!(error = %e, "failed to deserialize request into operation's input");
                any_rejections::Two::A(e)
            }),
            ready(t2_result),
        )
    }
}

impl<P, T> FromParts<P> for Option<T>
where
    T: FromParts<P>,
{
    type Rejection = Infallible;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(T::from_parts(parts).ok())
    }
}

impl<P, T> FromParts<P> for Result<T, T::Rejection>
where
    T: FromParts<P>,
{
    type Rejection = Infallible;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(T::from_parts(parts))
    }
}

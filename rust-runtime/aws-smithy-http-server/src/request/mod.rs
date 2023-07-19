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
//! See [Accessing Un-modelled data](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/from_parts.md)
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
use http::{request::Parts, Request, StatusCode};

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
pub trait FromParts<Ser, Op>: Sized {
    /// The type of the extraction failures.
    type Rejection: IntoResponse<Ser, Op>;

    /// Extracts `self` from a [`Parts`] synchronously.
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}

impl<Ser, Op> FromParts<Ser, Op> for () {
    type Rejection = Infallible;

    fn from_parts(_parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(())
    }
}

impl<Ser, Op, T> FromParts<Ser, Op> for (T,)
where
    T: FromParts<Ser, Op>,
{
    type Rejection = T::Rejection;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok((T::from_parts(parts)?,))
    }
}

macro_rules! impl_from_parts {
    ($error_name:ident, $($var:ident),+) => (
        impl<Ser, Op, $($var,)*> FromParts<Ser, Op> for ($($var),*)
        where
            $($var: FromParts<Ser, Op>,)*
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
pub trait FromRequest<Ser, Op, B>: Sized {
    /// The type of the extraction failures.
    type Rejection: IntoResponse<Ser, Op>;
    /// The type of the extraction [`Future`].
    type Future: Future<Output = Result<Self, Self::Rejection>>;

    /// Extracts `self` from a [`Request`] asynchronously.
    fn from_request(request: Request<B>) -> Self::Future;
}

macro_rules! impl_from_request {
    ($($var:ident),*) => (
        #[allow(unused_parens)]
        impl<Ser, Op, B, T, $($var),*> FromRequest<Ser, Op, B> for (T, $($var),*)
        where
            T: FromRequest<Ser, Op, B>,
            $(
                $var: FromParts<Ser, Op>
            ),*
        {
            type Rejection = any_rejections::Two<
                T::Rejection,
                <($($var),*) as FromParts<Ser, Op>>::Rejection
            >;

            type Future = MapOk<
                TryJoin<
                    MapErr<T::Future, fn(T::Rejection) -> Self::Rejection>,
                    Ready<Result<($($var),*), Self::Rejection>>
                >,
                fn((T, ($($var),*))) -> Self
            >;

            #[allow(non_snake_case)]
            fn from_request(request: Request<B>) -> Self::Future {
                let (mut parts, body) = request.into_parts();
                let vars = <($($var),*) as FromParts<Ser, Op>>::from_parts(&mut parts).map_err(any_rejections::Two::B);

                let request = Request::from_parts(parts, body);
                // This is required to coerce the closure
                let map_err: fn(_) -> _ = |err| any_rejections::Two::A(err);
                let t = T::from_request(request).map_err(map_err);
                let closure: fn((T, ($($var),*))) -> (T, $($var),*) = |x| {
                    let (t, ($($var),*)) = x;
                    (t, $($var),*)
                };
                try_join(
                    t,
                    ready(vars)
                ).map_ok(closure)
            }
        }
    )
}

impl_from_request!();
impl_from_request!(T0);
impl_from_request!(T0, T1);
impl_from_request!(T0, T1, T2);
impl_from_request!(T0, T1, T2, T3);
impl_from_request!(T0, T1, T2, T3, T4);
impl_from_request!(T0, T1, T2, T3, T4, T5);
impl_from_request!(T0, T1, T2, T3, T4, T5, T6);
impl_from_request!(T0, T1, T2, T3, T4, T5, T6, T7);

impl<Ser, Op, T> FromParts<Ser, Op> for Option<T>
where
    T: FromParts<Ser, Op>,
{
    type Rejection = Infallible;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(T::from_parts(parts).ok())
    }
}

impl<Ser, Op, T> FromParts<Ser, Op> for Result<T, T::Rejection>
where
    T: FromParts<Ser, Op>,
{
    type Rejection = Infallible;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(T::from_parts(parts))
    }
}

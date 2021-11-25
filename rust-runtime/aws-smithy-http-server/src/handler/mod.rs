/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2021 Tower Contributors
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

//! Async functions that can be used to handle requests.
//!
//! For a function to be used as a handler it must implement the [`Handler`] trait.
//! We provide blanket implementations for functions that:
//!
//! - Are `async fn`s.
//! - Take one argument that can be converted (via [`std::convert::Into`]) into a type that
//!   implements [`FromRequest`].
//! - Returns a type that can be converted (via [`std::convert::Into`]) into a type that implements
//!   [`IntoResponse`].
//! - If a closure is used it must implement `Clone + Send` and be
//!   `'static`.
//! - Returns a future that is `Send`. The most common way to accidentally make a
//!   future `!Send` is to hold a `!Send` type across an await.

use async_trait::async_trait;
use axum::extract::{FromRequest, RequestParts};
use axum::response::IntoResponse;
use http::{Request, Response};
use std::future::Future;

use crate::body::{box_body, BoxBody};

#[async_trait]
pub trait HandlerMarker<B, T, Fut>: Clone + Send + Sized + 'static {
    async fn call(self, req: Request<B>) -> Response<BoxBody>;
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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

//! Future types.
use crate::body::BoxBody;
use futures_util::future::Either;
use http::{Request, Response};
use std::{convert::Infallible, future::ready};
use tower::util::Oneshot;

pub use super::{into_make_service::IntoMakeService, route::RouteFuture};

type OneshotRoute<B> = Oneshot<super::Route<B>, Request<B>>;
type ReadyResponse = std::future::Ready<Result<Response<BoxBody>, Infallible>>;

opaque_future! {
    /// Response future for [`Router`](super::Router).
    pub type RouterFuture<B> =
        futures_util::future::Either<OneshotRoute<B>, ReadyResponse>;
}

impl<B> RouterFuture<B> {
    pub(super) fn from_oneshot(future: Oneshot<super::Route<B>, Request<B>>) -> Self {
        Self::new(Either::Left(future))
    }

    pub(super) fn from_response(response: Response<BoxBody>) -> Self {
        Self::new(Either::Right(ready(Ok(response))))
    }
}

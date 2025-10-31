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

use crate::body::BoxBody;


use crate::http::{Request, Response};
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{
    util::{BoxCloneService, Oneshot},
    Service, ServiceExt,
};

/// A HTTP [`Service`] representing a single route.
///
/// The construction of [`Route`] from a named HTTP [`Service`] `S`, erases the type of `S`.
pub struct Route<B = hyper::body::Incoming> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}

impl<B> Route<B> {
    /// Constructs a new [`Route`] from a well-formed HTTP service which is cloneable.
    pub fn new<T>(svc: T) -> Self
    where
        T: Service<Request<B>, Response = Response<BoxBody>, Error = Infallible> + Clone + Send + 'static,
        T::Future: Send + 'static,
    {
        Self {
            service: BoxCloneService::new(svc),
        }
    }
}

impl<ReqBody> Clone for Route<ReqBody> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
        }
    }
}

impl<ReqBody> fmt::Debug for Route<ReqBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route").finish()
    }
}

impl<B> Service<Request<B>> for Route<B> {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = RouteFuture<B>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: Request<B>) -> Self::Future {
        RouteFuture::new(self.service.clone().oneshot(req))
    }
}

pin_project_lite::pin_project! {
    /// Response future for [`Route`].
    pub struct RouteFuture<B> {
        #[pin]
        future: Oneshot<BoxCloneService<Request<B>, Response<BoxBody>, Infallible>, Request<B>>,
    }
}

impl<B> RouteFuture<B> {
    pub(crate) fn new(future: Oneshot<BoxCloneService<Request<B>, Response<BoxBody>, Infallible>, Request<B>>) -> Self {
        RouteFuture { future }
    }
}

impl<B> Future for RouteFuture<B> {
    type Output = Result<Response<BoxBody>, Infallible>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().future.poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traits() {
        use crate::test_helpers::*;

        assert_send::<Route<()>>();
    }
}

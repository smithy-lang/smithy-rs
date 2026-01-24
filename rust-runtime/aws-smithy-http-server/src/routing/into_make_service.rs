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

use std::{
    convert::Infallible,
    future::ready,
    task::{Context, Poll},
};
use tower::Service;

/// A [`MakeService`] that produces router services.
///
/// [`MakeService`]: tower::make::MakeService
#[derive(Debug, Clone)]
pub struct IntoMakeService<S> {
    service: S,
}

impl<S> IntoMakeService<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S, T> Service<T> for IntoMakeService<S>
where
    S: Clone,
{
    type Response = S;
    type Error = Infallible;
    type Future = MakeRouteServiceFuture<S>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: T) -> Self::Future {
        println!("[TRACE 5] File: aws-smithy-http-server/src/routing/into_make_service.rs");
        println!("[TRACE 5] Type: IntoMakeService<S>");
        println!("[TRACE 5] Function: Service::call() - Cloning router service for new connection");
        MakeRouteServiceFuture::new(ready(Ok(self.service.clone())))
    }
}

opaque_future! {
    /// Response future for [`IntoMakeService`] services.
    pub type MakeRouteServiceFuture<S> =
        std::future::Ready<Result<S, Infallible>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traits() {
        use crate::test_helpers::*;

        assert_send::<IntoMakeService<()>>();
        assert_sync::<IntoMakeService<()>>();
    }
}

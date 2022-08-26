/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    future::{ready, Ready},
    task::{Context, Poll},
};

use tower::Service;

/// A basic [`MakeService`](tower::MakeService) which [`Clone`]s the inner service.
pub struct IntoMakeService<S> {
    svc: S,
}

impl<S> IntoMakeService<S> {
    /// Constructs a new [`IntoMakeService`].
    pub fn new(svc: S) -> Self {
        Self { svc }
    }
}

impl<S, T> Service<T> for IntoMakeService<S>
where
    S: Clone,
{
    type Response = S;
    type Error = Infallible;
    type Future = Ready<Result<S, Infallible>>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: T) -> Self::Future {
        ready(Ok(self.svc.clone()))
    }
}

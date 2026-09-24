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
use http::{Request, Response};
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{
    util::{BoxCloneService, BoxCloneSyncService, Oneshot},
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

/// A `Sync` HTTP [`Service`] representing a single route, used by the schema-driven router
/// ([`SchemaRoutingService`](super::SchemaRoutingService)).
///
/// Like [`Route`], constructing it erases the type of the wrapped service. Unlike [`Route`], it is
/// `Sync`, so the schema router can share one handler set across threads instead of copying it for
/// every request; the wrapped service must therefore be `Send + Sync`. The protocol routers used
/// by the legacy (non-schema) code path keep using [`Route`], which does not require `Sync`.
pub struct SyncRoute<B = hyper::body::Incoming> {
    service: BoxCloneSyncService<Request<B>, Response<BoxBody>, Infallible>,
}

impl<B> SyncRoute<B> {
    /// Constructs a new [`SyncRoute`] from a well-formed HTTP service which is cloneable.
    ///
    /// A service that is already a `SyncRoute` (for example, one passed through
    /// [`Identity`](tower::layer::util::Identity)) is returned as is rather than boxed again, so
    /// cloning it stays a single boxed clone.
    pub fn new<T>(svc: T) -> Self
    where
        B: 'static,
        T: Service<Request<B>, Response = Response<BoxBody>, Error = Infallible> + Clone + Send + Sync + 'static,
        T::Future: Send + 'static,
    {
        match try_downcast::<Self, T>(svc) {
            Ok(route) => route,
            Err(svc) => Self {
                service: BoxCloneSyncService::new(svc),
            },
        }
    }

    /// Calls an owned route without the extra clone [`Service::call`] makes through `&mut self`.
    pub(crate) fn call_owned(self, req: Request<B>) -> SyncRouteFuture<B> {
        SyncRouteFuture {
            future: self.service.oneshot(req),
        }
    }
}

/// Moves `value` out as a `T` when it is one, without allocating.
fn try_downcast<T: 'static, K: 'static>(value: K) -> Result<T, K> {
    let mut slot = Some(value);
    if let Some(slot) = (&mut slot as &mut dyn std::any::Any).downcast_mut::<Option<T>>() {
        return Ok(slot.take().expect("slot is filled"));
    }
    Err(slot.expect("slot is filled"))
}

impl<ReqBody> Clone for SyncRoute<ReqBody> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
        }
    }
}

impl<ReqBody> fmt::Debug for SyncRoute<ReqBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyncRoute").finish()
    }
}

impl<B> Service<Request<B>> for SyncRoute<B> {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = SyncRouteFuture<B>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: Request<B>) -> Self::Future {
        self.clone().call_owned(req)
    }
}

pin_project_lite::pin_project! {
    /// Response future for [`SyncRoute`].
    pub struct SyncRouteFuture<B> {
        #[pin]
        future: Oneshot<BoxCloneSyncService<Request<B>, Response<BoxBody>, Infallible>, Request<B>>,
    }
}

impl<B> Future for SyncRouteFuture<B> {
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
        assert_send::<SyncRoute<()>>();
        assert_sync::<SyncRoute<()>>();
    }

    /// `Route` must not require `Sync`: legacy handlers and layers may hold `!Sync` state.
    #[test]
    fn route_accepts_a_non_sync_service() {
        let hits = std::cell::Cell::new(0u32); // Send + Clone, not Sync
        let _route: Route<()> = Route::new(tower::service_fn(move |_: Request<()>| {
            hits.set(hits.get() + 1);
            async { Ok::<_, Infallible>(Response::new(crate::body::empty())) }
        }));
    }

    #[test]
    fn try_downcast_moves_out_only_the_matching_type() {
        assert_eq!(
            try_downcast::<String, _>(String::from("route")),
            Ok(String::from("route"))
        );
        assert_eq!(try_downcast::<String, _>(7u8), Err(7u8));
    }

    #[tokio::test]
    async fn wrapping_a_sync_route_keeps_serving_it() {
        let route: SyncRoute<()> = SyncRoute::new(tower::service_fn(|_: Request<()>| async {
            Ok::<_, Infallible>(Response::builder().status(204).body(crate::body::empty()).unwrap())
        }));
        let rewrapped = SyncRoute::new(tower::Layer::layer(&tower::layer::util::Identity::new(), route));
        let response = rewrapped.oneshot(Request::new(())).await.unwrap();
        assert_eq!(response.status(), 204);
    }
}

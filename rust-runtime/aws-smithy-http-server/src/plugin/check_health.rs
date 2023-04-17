/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Middleware for handling health check requests.

use std::marker::PhantomData;
use std::task::{Context, Poll};

use futures_util::Future;
use hyper::{Body, Request, Response};
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Layer, Service, ServiceExt};

use crate::body::BoxBody;

use super::Either;

/// A [`tower::Layer`] used to apply [`CheckHealthService`].
#[derive(Clone, Debug)]
pub struct CheckHealthLayer<PingHandler> {
    health_check_uri: &'static str,
    ping_handler: PingHandler,
}

impl CheckHealthLayer<()> {
    pub fn new<HandlerFuture: Future<Output = Response<BoxBody>>, H: Fn(Request<Body>) -> HandlerFuture>(
        health_check_uri: &'static str,
        ping_handler: H,
    ) -> CheckHealthLayer<H> {
        CheckHealthLayer {
            health_check_uri,
            ping_handler,
        }
    }
}

impl<S, H: Clone> Layer<S> for CheckHealthLayer<H> {
    type Service = CheckHealthService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        CheckHealthService {
            inner,
            layer: self.clone(),
        }
    }
}

/// A middleware [`Service`] responsible for handling health check requests.
#[derive(Clone, Debug)]
pub struct CheckHealthService<H, S> {
    inner: S,
    layer: CheckHealthLayer<H>,
}

pin_project! {
    /// A future that converts `F` into a compatible [`Service::Future`].
    pub struct MappedHandlerFuture<R, S, F> {
        #[pin]
        inner: F,
        pd: PhantomData<fn(R) -> S>,
    }
}

impl<R, S, F> MappedHandlerFuture<R, S, F> {
    fn new(inner: F) -> MappedHandlerFuture<R, S, F> {
        Self { inner, pd: PhantomData }
    }
}

impl<R, S: Service<R>, F: Future<Output = S::Response>> Future for MappedHandlerFuture<R, S, F> {
    type Output = Result<S::Response, S::Error>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(Ok)
    }
}

impl<H, HandlerFuture, S> Service<Request<Body>> for CheckHealthService<H, S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>> + Clone,
    S::Future: std::marker::Send + 'static,
    HandlerFuture: Future<Output = Response<BoxBody>>,
    H: Fn(Request<Body>) -> HandlerFuture,
{
    type Response = S::Response;

    type Error = S::Error;

    type Future = CheckHealthFuture<S, HandlerFuture>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the service is ready is done by `Oneshot` below.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if req.uri() == self.layer.health_check_uri {
            let handler_future = (self.layer.ping_handler)(req);

            CheckHealthFuture::handler_future(handler_future)
        } else {
            let clone = self.inner.clone();
            let service = std::mem::replace(&mut self.inner, clone);
            let service_future = service.oneshot(req);

            CheckHealthFuture::service_future(service_future)
        }
    }
}

type CheckHealthFutureInner<S, HandlerFuture> =
    Either<MappedHandlerFuture<Request<Body>, S, HandlerFuture>, Oneshot<S, Request<Body>>>;

pin_project! {
    /// Future for [`CheckHealthService`].
    pub struct CheckHealthFuture<S: Service<Request<Body>>, HandlerFuture: Future<Output = S::Response>> {
        #[pin]
        inner: CheckHealthFutureInner<S, HandlerFuture>
    }
}

impl<S: Service<Request<Body>>, HandlerFuture: Future<Output = S::Response>> CheckHealthFuture<S, HandlerFuture> {
    fn handler_future(handler_future: HandlerFuture) -> Self {
        Self {
            inner: Either::Left {
                value: MappedHandlerFuture::new(handler_future),
            },
        }
    }

    fn service_future(service_future: Oneshot<S, Request<Body>>) -> Self {
        Self {
            inner: Either::Right { value: service_future },
        }
    }
}

impl<S: Service<Request<Body>>, HandlerFuture: Future<Output = S::Response>> Future
    for CheckHealthFuture<S, HandlerFuture>
{
    type Output = Result<S::Response, S::Error>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

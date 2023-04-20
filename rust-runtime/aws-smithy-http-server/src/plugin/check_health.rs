/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Middleware for handling health check requests.
//!
//! # Example
//!
//! ```no_run
//! # use aws_smithy_http_server::{body, plugin::{PluginPipeline, check_health::CheckHealthLayer}};
//! # use hyper::{Body, Response, StatusCode};
//! let plugins = PluginPipeline::new()
//!     // Handle all `/ping` health check requests by returning a `200 OK`.
//!     .http_layer(CheckHealthLayer::new("/ping", |_req| async {
//!         Response::builder()
//!             .status(StatusCode::OK)
//!             .body(body::boxed(Body::empty()))
//!             .expect("Couldn't construct response")
//!     }));
//!
//! ```

use std::task::{Context, Poll};

use futures_util::Future;
use hyper::{Body, Request, Response};
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Layer, Service, ServiceExt};

use crate::body::BoxBody;

use super::either::EitherProj;
use super::Either;

/// A [`tower::Layer`] used to apply [`CheckHealthService`].
#[derive(Clone, Debug)]
pub struct CheckHealthLayer<'a, HealthCheckHandler> {
    health_check_uri: &'a str,
    health_check_handler: HealthCheckHandler,
}

impl<'a> CheckHealthLayer<'a, ()> {
    pub fn new<HandlerFuture: Future<Output = Response<BoxBody>>, H: Fn(Request<Body>) -> HandlerFuture>(
        health_check_uri: &'static str,
        health_check_handler: H,
    ) -> CheckHealthLayer<H> {
        CheckHealthLayer {
            health_check_uri,
            health_check_handler,
        }
    }
}

impl<'a, S, H: Clone> Layer<S> for CheckHealthLayer<'a, H> {
    type Service = CheckHealthService<'a, H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        CheckHealthService {
            inner,
            layer: self.clone(),
        }
    }
}

/// A middleware [`Service`] responsible for handling health check requests.
#[derive(Clone, Debug)]
pub struct CheckHealthService<'a, H, S> {
    inner: S,
    layer: CheckHealthLayer<'a, H>,
}

impl<'a, H, HandlerFuture, S> Service<Request<Body>> for CheckHealthService<'a, H, S>
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
            let handler_future = (self.layer.health_check_handler)(req);

            CheckHealthFuture::handler_future(handler_future)
        } else {
            let clone = self.inner.clone();
            let service = std::mem::replace(&mut self.inner, clone);
            let service_future = service.oneshot(req);

            CheckHealthFuture::service_future(service_future)
        }
    }
}

type CheckHealthFutureInner<S, HandlerFuture> = Either<HandlerFuture, Oneshot<S, Request<Body>>>;

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
            inner: Either::Left { value: handler_future },
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
        let either_proj = self.project().inner.project();

        match either_proj {
            EitherProj::Left { value } => value.poll(cx).map(Ok),
            EitherProj::Right { value } => value.poll(cx),
        }
    }
}

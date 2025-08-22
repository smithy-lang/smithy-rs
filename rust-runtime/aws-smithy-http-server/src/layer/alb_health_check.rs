/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Middleware for handling [ALB health
//! checks](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/target-group-health-checks.html).
//!
//! # Example
//!
//! ```no_run
//! use aws_smithy_http_server::layer::alb_health_check::AlbHealthCheckLayer;
//! use hyper::StatusCode;
//! use tower::Layer;
//!
//! // Handle all `/ping` health check requests by returning a `200 OK`.
//! let ping_layer = AlbHealthCheckLayer::from_handler("/ping", |_req| async {
//!     StatusCode::OK
//! });
//! # async fn handle() { }
//! let app = tower::service_fn(handle);
//! let app = ping_layer.layer(app);
//! ```

use std::borrow::Cow;
use std::convert::Infallible;
use std::task::{Context, Poll};

use futures_util::{Future, FutureExt};
#[cfg(feature = "http-02x")]
use http::StatusCode;
#[cfg(feature = "http-02x")]
use hyper::{Body, Request, Response};
use pin_project_lite::pin_project;
use tower::{service_fn, util::Oneshot, Layer, Service, ServiceExt};

#[cfg(feature = "http-02x")]
use crate::body::BoxBody;

use crate::plugin::either::Either;
use crate::plugin::either::EitherProj;

/// A [`tower::Layer`] used to apply [`AlbHealthCheckService`].
#[cfg(feature = "http-02x")]
#[derive(Clone, Debug)]
pub struct AlbHealthCheckLayer<HealthCheckHandler> {
    health_check_uri: Cow<'static, str>,
    health_check_handler: HealthCheckHandler,
}

#[cfg(feature = "http-02x")]
impl AlbHealthCheckLayer<()> {
    /// Handle health check requests at `health_check_uri` with the specified handler.
    #[cfg(feature = "http-02x")]
    pub fn from_handler<HandlerFuture: Future<Output = StatusCode>, H: Fn(Request<Body>) -> HandlerFuture + Clone>(
        health_check_uri: impl Into<Cow<'static, str>>,
        health_check_handler: H,
    ) -> AlbHealthCheckLayer<
        impl Service<
                Request<Body>,
                Response = StatusCode,
                Error = Infallible,
                Future = impl Future<Output = Result<StatusCode, Infallible>>,
            > + Clone,
    > {
        let service = service_fn(move |req| health_check_handler(req).map(Ok));

        AlbHealthCheckLayer::new(health_check_uri, service)
    }

    /// Handle health check requests at `health_check_uri` with the specified service.
    #[cfg(feature = "http-02x")]
    pub fn new<H: Service<Request<Body>, Response = StatusCode>>(
        health_check_uri: impl Into<Cow<'static, str>>,
        health_check_handler: H,
    ) -> AlbHealthCheckLayer<H> {
        AlbHealthCheckLayer {
            health_check_uri: health_check_uri.into(),
            health_check_handler,
        }
    }
}

#[cfg(feature = "http-02x")]
impl<S, H: Clone> Layer<S> for AlbHealthCheckLayer<H> {
    type Service = AlbHealthCheckService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        AlbHealthCheckService {
            inner,
            layer: self.clone(),
        }
    }
}

/// A middleware [`Service`] responsible for handling health check requests.
#[cfg(feature = "http-02x")]
#[derive(Clone, Debug)]
pub struct AlbHealthCheckService<H, S> {
    inner: S,
    layer: AlbHealthCheckLayer<H>,
}

#[cfg(feature = "http-02x")]
impl<H, S> Service<Request<Body>> for AlbHealthCheckService<H, S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>> + Clone,
    S::Future: Send + 'static,
    H: Service<Request<Body>, Response = StatusCode, Error = Infallible> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = AlbHealthCheckFuture<H, S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the service is ready is done by `Oneshot` below.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if req.uri() == self.layer.health_check_uri.as_ref() {
            let clone = self.layer.health_check_handler.clone();
            let service = std::mem::replace(&mut self.layer.health_check_handler, clone);
            let handler_future = service.oneshot(req);

            AlbHealthCheckFuture::handler_future(handler_future)
        } else {
            let clone = self.inner.clone();
            let service = std::mem::replace(&mut self.inner, clone);
            let service_future = service.oneshot(req);

            AlbHealthCheckFuture::service_future(service_future)
        }
    }
}

#[cfg(feature = "http-02x")]
type HealthCheckFutureInner<H, S> = Either<Oneshot<H, Request<Body>>, Oneshot<S, Request<Body>>>;

#[cfg(feature = "http-02x")]
pin_project! {
    /// Future for [`AlbHealthCheckService`].
    pub struct AlbHealthCheckFuture<H: Service<Request<Body>, Response = StatusCode>, S: Service<Request<Body>>> {
        #[pin]
        inner: HealthCheckFutureInner<H, S>
    }
}

#[cfg(feature = "http-02x")]
impl<H, S> AlbHealthCheckFuture<H, S>
where
    H: Service<Request<Body>, Response = StatusCode>,
    S: Service<Request<Body>>,
{
    fn handler_future(handler_future: Oneshot<H, Request<Body>>) -> Self {
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

#[cfg(feature = "http-02x")]
impl<H, S> Future for AlbHealthCheckFuture<H, S>
where
    H: Service<Request<Body>, Response = StatusCode, Error = Infallible>,
    S: Service<Request<Body>, Response = Response<BoxBody>>,
{
    type Output = Result<S::Response, S::Error>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let either_proj = self.project().inner.project();

        match either_proj {
            EitherProj::Left { value } => {
                let polled: Poll<Self::Output> = value.poll(cx).map(|res| {
                    res.map(|status_code| {
                        Response::builder()
                            .status(status_code)
                            .body(crate::body::empty())
                            .unwrap()
                    })
                    .map_err(|never| match never {})
                });
                polled
            }
            EitherProj::Right { value } => value.poll(cx),
        }
    }
}

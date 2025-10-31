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
//! use aws_smithy_http_server::http::StatusCode;
//! use tower::Layer;
//!
//! // Handle all `/ping` health check requests by returning a `200 OK`.
//! let ping_layer = AlbHealthCheckLayer::from_handler("/ping", |_req: hyper::Request<hyper::body::Incoming>| async {
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
use pin_project_lite::pin_project;
use tower::{service_fn, util::Oneshot, Layer, Service, ServiceExt};

use hyper;

use crate::http::StatusCode;
use http_body::Body;

use hyper::{Request, Response};

use crate::body::BoxBody;

use crate::plugin::either::Either;
use crate::plugin::either::EitherProj;

/// A [`tower::Layer`] used to apply [`AlbHealthCheckService`].
#[derive(Clone, Debug)]
pub struct AlbHealthCheckLayer<HealthCheckHandler> {
    health_check_uri: Cow<'static, str>,
    health_check_handler: HealthCheckHandler,
}

impl AlbHealthCheckLayer<()> {
    /// Handle health check requests at `health_check_uri` with the specified handler.
    pub fn from_handler<
        B: Body,
        HandlerFuture: Future<Output = StatusCode>,
        H: Fn(Request<B>) -> HandlerFuture + Clone,
    >(
        health_check_uri: impl Into<Cow<'static, str>>,
        health_check_handler: H,
    ) -> AlbHealthCheckLayer<
        impl Service<
                Request<B>,
                Response = StatusCode,
                Error = Infallible,
                Future = impl Future<Output = Result<StatusCode, Infallible>>,
            > + Clone,
    > {
        let service = service_fn(move |req| health_check_handler(req).map(Ok));

        AlbHealthCheckLayer::new(health_check_uri, service)
    }

    /// Handle health check requests at `health_check_uri` with the specified service.
    pub fn new<B, H: Service<Request<B>, Response = StatusCode>>(
        health_check_uri: impl Into<Cow<'static, str>>,
        health_check_handler: H,
    ) -> AlbHealthCheckLayer<H> {
        AlbHealthCheckLayer {
            health_check_uri: health_check_uri.into(),
            health_check_handler,
        }
    }
}

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
#[derive(Clone, Debug)]
pub struct AlbHealthCheckService<H, S> {
    inner: S,
    layer: AlbHealthCheckLayer<H>,
}

impl<B, H, S> Service<Request<B>> for AlbHealthCheckService<H, S>
where
    S: Service<Request<B>, Response = Response<BoxBody>> + Clone,
    S::Future: Send + 'static,
    H: Service<Request<B>, Response = StatusCode, Error = Infallible> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = AlbHealthCheckFuture<B, H, S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the service is ready is done by `Oneshot` below.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
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

type HealthCheckFutureInner<B, H, S> = Either<Oneshot<H, Request<B>>, Oneshot<S, Request<B>>>;

pin_project! {
    /// Future for [`AlbHealthCheckService`].
    pub struct AlbHealthCheckFuture<B, H: Service<Request<B>, Response = StatusCode>, S: Service<Request<B>>> {
        #[pin]
        inner: HealthCheckFutureInner<B, H, S>
    }
}

impl<B, H, S> AlbHealthCheckFuture<B, H, S>
where
    H: Service<Request<B>, Response = StatusCode>,
    S: Service<Request<B>>,
{
    fn handler_future(handler_future: Oneshot<H, Request<B>>) -> Self {
        Self {
            inner: Either::Left { value: handler_future },
        }
    }

    fn service_future(service_future: Oneshot<S, Request<B>>) -> Self {
        Self {
            inner: Either::Right { value: service_future },
        }
    }
}

impl<B, H, S> Future for AlbHealthCheckFuture<B, H, S>
where
    H: Service<Request<B>, Response = StatusCode, Error = Infallible>,
    S: Service<Request<B>, Response = Response<BoxBody>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;
    use tower::{service_fn, ServiceExt};

    #[tokio::test]
    async fn test_health_check_handler_responds_to_matching_uri() {
        let layer = AlbHealthCheckLayer::from_handler("/health", |_req| async { StatusCode::OK });
        let inner_service = service_fn(|_req| async { Ok::<_, Infallible>(Response::new(crate::body::empty())) });
        let service = layer.layer(inner_service);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(crate::body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_non_health_check_requests_pass_through() {
        let layer = AlbHealthCheckLayer::from_handler("/health", |_req| async { StatusCode::OK });
        let inner_service = service_fn(|_req| async {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .body(crate::body::empty())
                    .unwrap(),
            )
        });
        let service = layer.layer(inner_service);

        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/data")
            .body(crate::body::empty())
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_handler_can_read_request_headers() {
        let layer = AlbHealthCheckLayer::from_handler("/ping", |req| async move {
            if req.headers().get("x-health-check").is_some() {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        let inner_service = service_fn(|_req| async { Ok::<_, Infallible>(Response::new(crate::body::empty())) });
        let service = layer.layer(inner_service);

        // Test with header present
        let request = Request::builder()
            .uri("/ping")
            .header("x-health-check", "true")
            .body(crate::body::empty())
            .unwrap();

        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Test without header
        let request = Request::builder().uri("/ping").body(crate::body::empty()).unwrap();

        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_works_with_any_body_type() {
        use bytes::Bytes;
        use http_body_util::Full;

        let layer = AlbHealthCheckLayer::from_handler("/health", |_req: Request<Full<Bytes>>| async { StatusCode::OK });
        let inner_service =
            service_fn(|_req: Request<Full<Bytes>>| async { Ok::<_, Infallible>(Response::new(crate::body::empty())) });
        let service = layer.layer(inner_service);

        let request = Request::builder()
            .uri("/health")
            .body(Full::new(Bytes::from("test body")))
            .unwrap();

        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

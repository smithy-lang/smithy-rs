/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html

mod into_make_service;
mod into_make_service_with_connect_info;
#[cfg(feature = "aws-lambda")]
#[cfg_attr(docsrs, doc(cfg(feature = "aws-lambda")))]
mod lambda_handler;

#[doc(hidden)]
pub mod request_spec;

mod route;

pub(crate) mod tiny_map;

use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tower::{util::Oneshot, Service, ServiceExt};
use tracing::debug;

use crate::body::BoxBody;

#[cfg(feature = "aws-lambda")]
#[cfg_attr(docsrs, doc(cfg(feature = "aws-lambda")))]
pub use self::lambda_handler::LambdaHandler;

#[allow(deprecated)]
pub use self::{
    into_make_service::IntoMakeService,
    into_make_service_with_connect_info::{Connected, IntoMakeServiceWithConnectInfo},
    route::Route,
};

pub(crate) const UNKNOWN_OPERATION_EXCEPTION: &str = "UnknownOperationException";

/// Constructs common response to method disallowed.
pub(crate) fn method_disallowed() -> http::Response<BoxBody> {
    let mut responses = http::Response::default();
    *responses.status_mut() = http::StatusCode::METHOD_NOT_ALLOWED;
    responses
}

/// An interface for retrieving an inner [`Service`] given a [`http::Request`].
pub trait Router<B> {
    type Service: Service<http::Request<B>, Error = Infallible>;
    type Error;

    /// Matches a [`http::Request`] to a target [`Service`].
    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error>;
}

/// A [`Service`] using the [`Router`] `R` to redirect messages to specific routes.
///
/// The `Protocol` parameter is used to determine the serialization of errors.
pub struct RoutingService<R> {
    router: R,
}

impl<R> fmt::Debug for RoutingService<R>
where
    R: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingService").field("router", &self.router).finish()
    }
}

impl<R> Clone for RoutingService<R>
where
    R: Clone,
{
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
        }
    }
}

impl<R> RoutingService<R> {
    /// Creates a [`RoutingService`] from a [`Router`].
    pub fn new(router: R) -> Self {
        Self { router }
    }
}

pin_project_lite::pin_project! {
    #[project = RoutingFutureInnerProj]
    enum RoutingFutureInner<B, R> where R: Router<B> {
        Oneshot {
            #[pin]
            oneshot: Oneshot<R::Service, http::Request<B>>
        },
        Error { error: Option<R::Error> }
    }
}

pin_project_lite::pin_project! {
    pub struct RoutingFuture<B, R> where R: Router<B> {
        #[pin]
        inner: RoutingFutureInner<B, R>
    }
}

impl<B, R> RoutingFuture<B, R>
where
    R: Router<B>,
{
    /// Creates a [`RoutingFuture`] from [`ServiceExt::oneshot`].
    pub(super) fn from_oneshot(oneshot: Oneshot<R::Service, http::Request<B>>) -> Self {
        Self {
            inner: RoutingFutureInner::Oneshot { oneshot },
        }
    }

    /// Creates a [`RoutingFuture`] from [`Service::Response`].
    pub(super) fn from_error(error: R::Error) -> Self {
        Self {
            inner: RoutingFutureInner::Error { error: Some(error) },
        }
    }
}

impl<B, R> Future for RoutingFuture<B, R>
where
    R: Router<B>,
{
    type Output = Result<<R::Service as Service<http::Request<B>>>::Response, R::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let inner = this.inner.project();
        use RoutingFutureInnerProj::*;
        match inner {
            Oneshot { oneshot } => oneshot.poll(cx).map_err(|err| match err {}),
            Error { error } => {
                let error = error.take().expect("futures should not be polled after completion");
                Poll::Ready(Err(error))
            }
        }
    }
}

impl<R, B> Service<http::Request<B>> for RoutingService<R>
where
    R: Router<B>,
    R::Service: Clone,
    R::Error: Error,
{
    type Response = <R::Service as Service<http::Request<B>>>::Response;
    type Error = R::Error;
    type Future = RoutingFuture<B, R>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        match self.router.match_route(&req) {
            // Successfully routed, use the routes `Service::call`.
            Ok(ok) => RoutingFuture::from_oneshot(ok.oneshot(req)),
            // Failed to route, use the `R::Error`s `IntoResponse<P>`.
            Err(error) => {
                debug!(%error, "failed to route");
                RoutingFuture::from_error(error)
            }
        }
    }
}

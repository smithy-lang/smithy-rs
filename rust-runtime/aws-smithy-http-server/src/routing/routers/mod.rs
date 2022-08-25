/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    error::Error,
    fmt,
    future::{ready, Future, Ready},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::future::Either;
use http::Response;
use tower::{util::Oneshot, Service, ServiceExt};
use tracing::debug;

use crate::{body::BoxBody, response::IntoResponse};

pub mod aws_json;
pub mod rest;

const UNKNOWN_OPERATION_EXCEPTION: &str = "UnknownOperationException";

/// Constructs common response to method disallowed.
fn method_disallowed() -> http::Response<BoxBody> {
    let mut responses = http::Response::default();
    *responses.status_mut() = http::StatusCode::METHOD_NOT_ALLOWED;
    responses
}

/// An interface for retrieving an inner [`Service`] given a [`http::Request`].
pub trait Router<B> {
    type Service;
    type Error;

    /// Matches a [`http::Request`] to a target [`Service`].
    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error>;
}

/// A [`Service`] using the a [`Router`] `R` to redirect messages to specific routes.
///
/// The `Protocol` parameter is used to determine
pub struct RoutingService<R, Protocol> {
    router: R,
    _protocol: PhantomData<Protocol>,
}

impl<R, P> fmt::Debug for RoutingService<R, P>
where
    R: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingService")
            .field("router", &self.router)
            .field("_protocol", &self._protocol)
            .finish()
    }
}

impl<R, P> Clone for RoutingService<R, P>
where
    R: Clone,
{
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            _protocol: PhantomData,
        }
    }
}

impl<R, P> RoutingService<R, P> {
    /// Creates a [`RoutingService`] from a [`Router`].
    pub fn new(router: R) -> Self {
        Self {
            router,
            _protocol: PhantomData,
        }
    }

    /// Maps a [`Router`] using a closure.
    pub fn map<RNew, F>(self, f: F) -> RoutingService<RNew, P>
    where
        F: FnOnce(R) -> RNew,
    {
        RoutingService {
            router: f(self.router),
            _protocol: PhantomData,
        }
    }
}

type EitherOneshotReady<S, B> = Either<
    Oneshot<S, http::Request<B>>,
    Ready<Result<<S as Service<http::Request<B>>>::Response, <S as Service<http::Request<B>>>::Error>>,
>;

pin_project_lite::pin_project! {
    pub struct RoutingFuture<S, B> where S: Service<http::Request<B>> {
        #[pin]
        inner: EitherOneshotReady<S, B>
    }
}

impl<S, B> RoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    /// Creates a [`RoutingFuture`] from [`ServiceExt::oneshot`].
    pub(super) fn from_oneshot(future: Oneshot<S, http::Request<B>>) -> Self {
        Self {
            inner: Either::Left(future),
        }
    }

    /// Creates a [`RoutingFuture`] from [`Service::Response`].
    pub(super) fn from_response(response: S::Response) -> Self {
        Self {
            inner: Either::Right(ready(Ok(response))),
        }
    }
}

impl<S, B> Future for RoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    type Output = <S::Future as Future>::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<R, P, B> Service<http::Request<B>> for RoutingService<R, P>
where
    R: Router<B>,
    R::Service: Service<http::Request<B>, Response = http::Response<BoxBody>> + Clone,
    R::Error: IntoResponse<P> + Error,
{
    type Response = Response<BoxBody>;
    type Error = <R::Service as Service<http::Request<B>>>::Error;
    type Future = RoutingFuture<R::Service, B>;

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
                RoutingFuture::from_response(error.into_response())
            }
        }
    }
}

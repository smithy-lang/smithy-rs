/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP routing that adheres to the [Smithy specification].
//!
//! [Smithy specification]: https://smithy.io/2.0/spec/http-bindings.html

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
    error::Error,
    fmt,
    future::{ready, Future, Ready},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_util::{
    future::{Either, MapOk},
    TryFutureExt,
};
use tower::{util::Oneshot, Service, ServiceExt};

use crate::http;
use http_body::Body as HttpBody;

use crate::http::Response;

use crate::{
    body::{boxed, BoxBody},
    error::BoxError,
    response::IntoResponse,
};

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
    type Service;
    type Error;

    /// Matches a [`http::Request`] to a target [`Service`].
    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error>;
}

/// A [`Service`] using the [`Router`] `R` to redirect messages to specific routes.
///
/// The `Protocol` parameter is used to determine the serialization of errors.
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
    MapOk<Oneshot<S, http::Request<B>>, fn(<S as Service<http::Request<B>>>::Response) -> http::Response<BoxBody>>,
    Ready<Result<http::Response<BoxBody>, <S as Service<http::Request<B>>>::Error>>,
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
    pub(super) fn from_oneshot<RespB>(future: Oneshot<S, http::Request<B>>) -> Self
    where
        S: Service<http::Request<B>, Response = http::Response<RespB>>,
        RespB: HttpBody<Data = Bytes> + Send + 'static,
        RespB::Error: Into<BoxError>,
    {
        Self {
            inner: Either::Left(future.map_ok(|x| x.map(boxed))),
        }
    }

    /// Creates a [`RoutingFuture`] from [`Service::Response`].
    pub(super) fn from_response(response: http::Response<BoxBody>) -> Self {
        Self {
            inner: Either::Right(ready(Ok(response))),
        }
    }
}

impl<S, B> Future for RoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    type Output = Result<http::Response<BoxBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<R, P, B, RespB> Service<http::Request<B>> for RoutingService<R, P>
where
    R: Router<B>,
    R::Service: Service<http::Request<B>, Response = http::Response<RespB>> + Clone,
    R::Error: IntoResponse<P> + Error,
    RespB: HttpBody<Data = Bytes> + Send + 'static,
    RespB::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = <R::Service as Service<http::Request<B>>>::Error;
    type Future = RoutingFuture<R::Service, B>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        tracing::debug!("inside routing service call");
        match self.router.match_route(&req) {
            // Successfully routed, use the routes `Service::call`.
            Ok(ok) => RoutingFuture::from_oneshot(ok.oneshot(req)),
            // Failed to route, use the `R::Error`s `IntoResponse<P>`.
            Err(error) => {
                tracing::debug!(%error, "failed to route");
                RoutingFuture::from_response(error.into_response())
            }
        }
    }
}

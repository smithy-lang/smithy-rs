/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    future::{Future, Ready},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_util::ready;
use http::{header, StatusCode, Version};
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Service, ServiceExt};
use tracing::error;

use crate::{
    body::BoxBody, plugin::Plugin, request::FromRequest, response::IntoResponse,
    runtime_error::InternalFailureException, service::ServiceShape,
};

use super::OperationShape;

/// A [`Plugin`] responsible for taking an operation [`Service`], accepting and returning Smithy
/// types and converting it into a [`Service`] taking and returning [`http`] types.
///
/// See [`Upgrade`].
#[derive(Debug, Clone)]
pub struct UpgradePlugin<Extractors> {
    _extractors: PhantomData<Extractors>,
    request_body_read_timeout: Option<RequestBodyReadTimeoutConfig>,
}

impl<Extractors> Default for UpgradePlugin<Extractors> {
    fn default() -> Self {
        Self {
            _extractors: PhantomData,
            request_body_read_timeout: None,
        }
    }
}

impl<Extractors> UpgradePlugin<Extractors> {
    /// Creates a new [`UpgradePlugin`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a deadline while the request body is read and the operation input is constructed.
    ///
    /// The deadline is removed before the operation handler is invoked.
    #[doc(hidden)]
    pub fn with_request_body_read_timeout(mut self, timeout: Duration, operation: &'static str) -> Self {
        self.request_body_read_timeout = Some(RequestBodyReadTimeoutConfig { timeout, operation });
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct RequestBodyReadTimeoutConfig {
    timeout: Duration,
    operation: &'static str,
}

impl<Ser, Op, T, Extractors> Plugin<Ser, Op, T> for UpgradePlugin<Extractors>
where
    Ser: ServiceShape,
    Op: OperationShape,
{
    type Output = Upgrade<Ser::Protocol, (Op::Input, Extractors), T>;

    fn apply(&self, inner: T) -> Self::Output {
        Upgrade {
            _protocol: PhantomData,
            _input: PhantomData,
            inner,
            request_body_read_timeout: self.request_body_read_timeout,
        }
    }
}

/// A [`Service`] responsible for wrapping an operation [`Service`] accepting and returning Smithy
/// types, and converting it into a [`Service`] accepting and returning [`http`] types.
pub struct Upgrade<Protocol, Input, S> {
    _protocol: PhantomData<Protocol>,
    _input: PhantomData<Input>,
    inner: S,
    request_body_read_timeout: Option<RequestBodyReadTimeoutConfig>,
}

impl<P, Input, S> Clone for Upgrade<P, Input, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            _protocol: PhantomData,
            _input: PhantomData,
            inner: self.inner.clone(),
            request_body_read_timeout: self.request_body_read_timeout,
        }
    }
}

struct RequestBodyReadTimeoutState {
    sleep: Pin<Box<tokio::time::Sleep>>,
    timeout: Duration,
    version: Version,
    operation: &'static str,
    #[cfg(feature = "request-id")]
    request_id: Option<String>,
}

impl RequestBodyReadTimeoutState {
    fn response(&self) -> http::Response<crate::body::BoxBody> {
        #[cfg(feature = "request-id")]
        if let Some(request_id) = self.request_id.as_deref() {
            tracing::debug!(
                operation = self.operation,
                request_id,
                timeout_millis = self.timeout.as_millis(),
                "request body read timed out"
            );
        } else {
            tracing::debug!(
                operation = self.operation,
                timeout_millis = self.timeout.as_millis(),
                "request body read timed out"
            );
        }
        #[cfg(not(feature = "request-id"))]
        tracing::debug!(
            operation = self.operation,
            timeout_millis = self.timeout.as_millis(),
            "request body read timed out"
        );

        let mut response = http::Response::builder().status(StatusCode::REQUEST_TIMEOUT);
        if matches!(self.version, Version::HTTP_10 | Version::HTTP_11) {
            response = response.header(header::CONNECTION, "close");
        }
        response.body(crate::body::empty()).expect("valid response")
    }
}

#[cfg(feature = "request-id")]
fn request_id<B>(req: &http::Request<B>) -> Option<String> {
    req.extensions()
        .get::<crate::request::request_id::ServerRequestId>()
        .map(ToString::to_string)
}

pin_project! {
    #[project = InnerProj]
    #[project_replace = InnerProjReplace]
    enum Inner<FromFut, HandlerFut> {
        FromRequest {
            #[pin]
            inner: FromFut
        },
        Inner {
            #[pin]
            call: HandlerFut
        }
    }
}

type InnerAlias<Input, Protocol, B, S> = Inner<<Input as FromRequest<Protocol, B>>::Future, Oneshot<S, Input>>;

pin_project! {
    /// The [`Service::Future`] of [`Upgrade`].
    pub struct UpgradeFuture<Protocol, Input, B, S>
    where
        Input: FromRequest<Protocol, B>,
        S: Service<Input>,
    {
        service: Option<S>,
        #[pin]
        inner: InnerAlias<Input, Protocol, B, S>,
        request_body_read_timeout: Option<RequestBodyReadTimeoutState>,
    }
}

impl<P, Input, B, S> Future for UpgradeFuture<P, Input, B, S>
where
    Input: FromRequest<P, B>,
    <Input as FromRequest<P, B>>::Rejection: std::fmt::Display,
    S: Service<Input>,
    S::Response: IntoResponse<P>,
    S::Error: IntoResponse<P>,
{
    type Output = Result<http::Response<crate::body::BoxBody>, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();
            let this2 = this.inner.as_mut().project();

            let call = match this2 {
                InnerProj::FromRequest { inner } => {
                    let result = match inner.poll(cx) {
                        Poll::Ready(result) => result,
                        Poll::Pending => {
                            if let Some(timeout) = this.request_body_read_timeout {
                                if timeout.sleep.as_mut().poll(cx).is_ready() {
                                    return Poll::Ready(Ok(timeout.response()));
                                }
                            }
                            return Poll::Pending;
                        }
                    };
                    *this.request_body_read_timeout = None;
                    match result {
                        Ok(ok) => this
                            .service
                            .take()
                            .expect("futures cannot be polled after completion")
                            .oneshot(ok),
                        Err(err) => {
                            // The error may arise either from a `FromRequest` failure for any user-defined
                            // handler's additional input parameters, or from a de-serialization failure
                            // of an input parameter specific to the operation.
                            tracing::trace!(error = %err, "parameter for the handler cannot be constructed");
                            return Poll::Ready(Ok(err.into_response()));
                        }
                    }
                }
                InnerProj::Inner { call } => {
                    let result = ready!(call.poll(cx));
                    let output = match result {
                        Ok(ok) => ok.into_response(),
                        Err(err) => err.into_response(),
                    };
                    return Poll::Ready(Ok(output));
                }
            };

            this.inner.as_mut().project_replace(Inner::Inner { call });
        }
    }
}

impl<P, Input, B, S> Service<http::Request<B>> for Upgrade<P, Input, S>
where
    Input: FromRequest<P, B>,
    <Input as FromRequest<P, B>>::Rejection: std::fmt::Display,
    S: Service<Input> + Clone,
    S::Response: IntoResponse<P>,
    S::Error: IntoResponse<P>,
{
    type Response = http::Response<crate::body::BoxBody>;
    type Error = Infallible;
    type Future = UpgradeFuture<P, Input, B, S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the inner service is ready is done by `Oneshot` in `UpgradeFuture`'s
        // implementation.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let service = std::mem::replace(&mut self.inner, clone);
        let request_body_read_timeout = self
            .request_body_read_timeout
            .map(|config| RequestBodyReadTimeoutState {
                sleep: Box::pin(tokio::time::sleep(config.timeout)),
                timeout: config.timeout,
                version: req.version(),
                operation: config.operation,
                #[cfg(feature = "request-id")]
                request_id: request_id(&req),
            });
        UpgradeFuture {
            service: Some(service),
            inner: Inner::FromRequest {
                inner: <Input as FromRequest<P, B>>::from_request(req),
            },
            request_body_read_timeout,
        }
    }
}

/// A [`Service`] which always returns an internal failure message and logs an error.
#[derive(Copy)]
pub struct MissingFailure<P> {
    _protocol: PhantomData<fn(P)>,
}

impl<P> Default for MissingFailure<P> {
    fn default() -> Self {
        Self { _protocol: PhantomData }
    }
}

impl<P> Clone for MissingFailure<P> {
    fn clone(&self) -> Self {
        MissingFailure { _protocol: PhantomData }
    }
}

impl<R, P> Service<R> for MissingFailure<P>
where
    InternalFailureException: IntoResponse<P>,
{
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: R) -> Self::Future {
        error!("the operation has not been set");
        std::future::ready(Ok(InternalFailureException.into_response()))
    }
}

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
};

use futures_util::ready;
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Service, ServiceExt};
use tracing::error;

use crate::{
    body::BoxBody, plugin::Plugin, request::FromRequest, response::IntoResponse,
    runtime_error::InternalFailureException, service::ServiceShape,
};

use super::{FixedService, OperationShape};

/// A [`Plugin`] responsible for taking an operation [`Service`], accepting and returning Smithy
/// types and converting it into a [`Service`] taking and returning [`http`] types.
///
/// See [`Upgrade`].
#[derive(Debug, Default, Clone)]
pub struct UpgradePlugin;

impl UpgradePlugin {
    /// Creates a new [`UpgradePlugin`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl<Ser, Op, T> Plugin<Ser, Op, T> for UpgradePlugin
where
    Ser: ServiceShape,
    Op: OperationShape,
{
    type Output = Upgrade<Ser, Op, T>;

    fn apply(&self, inner: T) -> Self::Output {
        Upgrade {
            _service: PhantomData,
            _operation: PhantomData,
            inner,
        }
    }
}

/// A [`Service`] responsible for wrapping an operation [`Service`] accepting and returning Smithy
/// types, and converting it into a [`Service`] accepting and returning [`http`] types.
pub struct Upgrade<Ser, Op, S> {
    _service: PhantomData<Ser>,
    _operation: PhantomData<Op>,
    inner: S,
}

impl<Ser, Op, S> Clone for Upgrade<Ser, Op, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            _service: PhantomData,
            _operation: PhantomData,
            inner: self.inner.clone(),
        }
    }
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

type InnerAlias<Input, Ser, Op, B, S> = Inner<<Input as FromRequest<Ser, Op, B>>::Future, Oneshot<S, Input>>;

pin_project! {
    /// The [`Service::Future`] of [`Upgrade`].
    pub struct UpgradeFuture<Ser, Op, B, S>
    where
        S: FixedService,
        S::FixedInput: FromRequest<Ser, Op, B>,
    {
        service: Option<S>,
        #[pin]
        inner: InnerAlias<S::FixedInput, Ser, Op, B, S>
    }
}

impl<Ser, Op, B, S> Future for UpgradeFuture<Ser, Op, B, S>
where
    S: FixedService,
    S::FixedInput: FromRequest<Ser, Op, B>,
    S::Response: IntoResponse<Ser, Op>,
    S::Error: IntoResponse<Ser, Op>,
{
    type Output = Result<http::Response<crate::body::BoxBody>, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();
            let this2 = this.inner.as_mut().project();

            let call = match this2 {
                InnerProj::FromRequest { inner } => {
                    let result = ready!(inner.poll(cx));
                    match result {
                        Ok(ok) => this
                            .service
                            .take()
                            .expect("futures cannot be polled after completion")
                            .oneshot(ok),
                        Err(err) => return Poll::Ready(Ok(err.into_response())),
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

impl<Ser, Op, B, S> Service<http::Request<B>> for Upgrade<Ser, Op, S>
where
    S: FixedService + Clone,
    S::FixedInput: FromRequest<Ser, Op, B>,
    S::Response: IntoResponse<Ser, Op>,
    S::Error: IntoResponse<Ser, Op>,
{
    type Response = http::Response<crate::body::BoxBody>;
    type Error = Infallible;
    type Future = UpgradeFuture<Ser, Op, B, S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let service = std::mem::replace(&mut self.inner, clone);
        UpgradeFuture {
            service: Some(service),
            inner: Inner::FromRequest {
                inner: <S::FixedInput as FromRequest<Ser, Op, B>>::from_request(req),
            },
        }
    }
}

/// A [`Service`] which always returns an internal failure message and logs an error.
#[derive(Copy)]
pub struct MissingFailure<Ser, Op> {
    _protocol: PhantomData<fn((Ser, Op))>,
}

impl<Ser, Op> Default for MissingFailure<Ser, Op> {
    fn default() -> Self {
        Self { _protocol: PhantomData }
    }
}

impl<Ser, Op> Clone for MissingFailure<Ser, Op> {
    fn clone(&self) -> Self {
        MissingFailure { _protocol: PhantomData }
    }
}

impl<R, Ser, Op> Service<R> for MissingFailure<Ser, Op>
where
    InternalFailureException: IntoResponse<Ser, Op>,
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

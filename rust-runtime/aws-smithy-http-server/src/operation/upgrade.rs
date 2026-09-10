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
    body::{BoxBody, HttpBody},
    plugin::Plugin,
    request::{FromParts, FromRequest},
    response::IntoResponse,
    runtime_error::InternalFailureException,
    schema::{
        collect_request_body, DeserializableShape, DeserializeError, RequestBodyCollectionConfig, RequestBodyHandling,
        SelectedProtocolOperation, ServerRequest,
    },
    service::ServiceShape,
};

use super::{OperationShape, SchemaOperationShape};
use aws_smithy_schema::serde::SerializableStruct;

/// A [`Plugin`] responsible for taking an operation [`Service`], accepting and returning Smithy
/// types and converting it into a [`Service`] taking and returning [`http`] types.
///
/// See [`Upgrade`].
#[derive(Debug, Clone)]
pub struct UpgradePlugin<Extractors> {
    _extractors: PhantomData<Extractors>,
}

/// Protocol-neutral marker for request-part extractors used by [`DynUpgrade`].
pub struct DynProtocol;

/// Converts a generated operation error enum using the protocol selected by routing.
pub trait IntoDynResponse {
    fn into_dyn_response(self, protocol: &dyn crate::schema::DynServerProtocol) -> http::Response<BoxBody>;
}

impl IntoDynResponse for Infallible {
    fn into_dyn_response(self, _protocol: &dyn crate::schema::DynServerProtocol) -> http::Response<BoxBody> {
        match self {}
    }
}

/// Schema-driven, protocol-neutral HTTP upgrade plugin.
#[derive(Debug, Clone)]
pub struct DynUpgradePlugin<Extractors> {
    config: RequestBodyCollectionConfig,
    _extractors: PhantomData<Extractors>,
}

impl<Extractors> DynUpgradePlugin<Extractors> {
    pub fn new(config: RequestBodyCollectionConfig) -> Self {
        Self {
            config,
            _extractors: PhantomData,
        }
    }
}

impl<Ser, Op, T, Extractors> Plugin<Ser, Op, T> for DynUpgradePlugin<Extractors>
where
    Ser: ServiceShape,
    Op: SchemaOperationShape,
{
    type Output = DynUpgrade<Op, Extractors, T>;
    fn apply(&self, inner: T) -> Self::Output {
        DynUpgrade {
            config: self.config,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner,
        }
    }
}

/// Upgrade service for a non-streaming schema operation.
pub struct DynUpgrade<Op, Extractors, S> {
    config: RequestBodyCollectionConfig,
    _operation: PhantomData<Op>,
    _extractors: PhantomData<Extractors>,
    inner: S,
}

impl<Op, Extractors, S: Clone> Clone for DynUpgrade<Op, Extractors, S> {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner: self.inner.clone(),
        }
    }
}

impl<Op, Extractors, B, S> Service<http::Request<B>> for DynUpgrade<Op, Extractors, S>
where
    Op: SchemaOperationShape,
    Op::Input: DeserializableShape + Send + 'static,
    Op::Output: SerializableStruct + Send + 'static,
    Extractors: FromParts<DynProtocol> + Send + 'static,
    <Extractors as FromParts<DynProtocol>>::Rejection: std::fmt::Display,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: std::error::Error + Send + Sync + 'static,
    S: Service<(Op::Input, Extractors), Response = Op::Output> + Clone + Send + 'static,
    S::Error: IntoDynResponse + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let service = std::mem::replace(&mut self.inner, clone);
        let config = self.config;
        Box::pin(async move {
            let (mut parts, body) = req.into_parts();
            let Some(selected) = parts.extensions.get::<SelectedProtocolOperation>().cloned() else {
                error!("selected protocol operation missing from request extensions");
                return Ok(empty_internal_server_error());
            };
            if !std::ptr::eq(selected.operation().schema(), Op::SCHEMA)
                || !selected.protocol().accepts_operation(&**selected.operation())
            {
                error!("selected protocol operation is incompatible with the routed operation");
                return Ok(empty_internal_server_error());
            }
            let extractors = match Extractors::from_parts(&mut parts) {
                Ok(value) => value,
                Err(err) => return Ok(err.into_response()),
            };
            let bytes = match selected.operation().request_body() {
                RequestBodyHandling::Unused => bytes::Bytes::new(),
                RequestBodyHandling::Collected => match collect_request_body(body, &config).await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return Ok(selected.protocol().serialize_rejection(DeserializeError::Serde(
                            aws_smithy_schema::serde::SerdeError::custom(err.to_string()),
                        )))
                    }
                },
                RequestBodyHandling::Streaming => {
                    error!("streaming operation routed through DynUpgrade");
                    return Ok(empty_internal_server_error());
                }
            };
            let converted = match aws_smithy_runtime_api::http::Request::try_from(http::Request::from_parts(parts, ()))
            {
                Ok(request) => request.into_parts(),
                Err(err) => {
                    return Ok(selected.protocol().serialize_rejection(DeserializeError::Serde(
                        aws_smithy_schema::serde::SerdeError::custom(err.to_string()),
                    )))
                }
            };
            let request = ServerRequest {
                uri: converted.uri,
                headers: converted.headers,
                body: bytes,
            };
            let input = {
                let mut deserializer = match selected
                    .protocol()
                    .deserialize_request(&**selected.operation(), &request)
                {
                    Ok(value) => value,
                    Err(err) => return Ok(selected.protocol().serialize_rejection(err)),
                };
                match Op::Input::deserialize(&mut *deserializer) {
                    Ok(value) => value,
                    Err(err) => return Ok(selected.protocol().serialize_rejection(err)),
                }
            };
            match service.oneshot((input, extractors)).await {
                Ok(output) => Ok(selected.protocol().serialize_response(&**selected.operation(), &output)),
                Err(err) => Ok(err.into_dyn_response(&**selected.protocol())),
            }
        })
    }
}

fn empty_internal_server_error() -> http::Response<BoxBody> {
    let mut response = http::Response::new(crate::body::empty());
    *response.status_mut() = http::StatusCode::INTERNAL_SERVER_ERROR;
    response
}

impl<Extractors> Default for UpgradePlugin<Extractors> {
    fn default() -> Self {
        Self {
            _extractors: PhantomData,
        }
    }
}

impl<Extractors> UpgradePlugin<Extractors> {
    /// Creates a new [`UpgradePlugin`].
    pub fn new() -> Self {
        Self::default()
    }
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
        }
    }
}

/// A [`Service`] responsible for wrapping an operation [`Service`] accepting and returning Smithy
/// types, and converting it into a [`Service`] accepting and returning [`http`] types.
pub struct Upgrade<Protocol, Input, S> {
    _protocol: PhantomData<Protocol>,
    _input: PhantomData<Input>,
    inner: S,
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
        inner: InnerAlias<Input, Protocol, B, S>
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
                    let result = ready!(inner.poll(cx));
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
        UpgradeFuture {
            service: Some(service),
            inner: Inner::FromRequest {
                inner: <Input as FromRequest<P, B>>::from_request(req),
            },
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

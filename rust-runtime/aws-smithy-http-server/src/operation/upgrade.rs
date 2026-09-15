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
        collect_request_body, DeserializableShape, DeserializeError, HttpModeledError, RequestBodyCollectionConfig,
        SelectedProtocolOperation, ServerRequest,
    },
    service::ServiceShape,
};

use super::{OperationShape, SchemaOperationShape, StreamingOperationShape};
use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_types::body::SdkBody;

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

/// Schema-driven, protocol-neutral HTTP upgrade plugin for operations without streaming members.
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
///
/// The body is collected under the operation's [`RequestBodyCollectionConfig`] when the selected
/// protocol asks for it, the input is read through the erased protocol handle from the request
/// extensions, and the output or error is serialized through the same handle.
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

/// Reads the routed operation out of the request extensions and checks it is `Op`.
pub(crate) fn selected_operation<Op: SchemaOperationShape>(
    extensions: &http::Extensions,
) -> Option<SelectedProtocolOperation> {
    let Some(selected) = extensions.get::<SelectedProtocolOperation>().cloned() else {
        error!("selected protocol operation missing from request extensions");
        return None;
    };
    if !std::ptr::eq(selected.operation(), Op::SCHEMA) {
        error!("selected protocol operation is incompatible with the routed operation");
        return None;
    }
    Some(selected)
}

/// Converts the HTTP parts and body into the runtime-api request the protocols read.
pub(crate) fn convert_request<B>(
    parts: http::request::Parts,
    body: B,
) -> Result<aws_smithy_runtime_api::http::Request<B>, DeserializeError> {
    aws_smithy_runtime_api::http::Request::try_from(http::Request::from_parts(parts, body))
        .map_err(|err| DeserializeError::Serde(aws_smithy_schema::serde::SerdeError::custom(err.to_string())))
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
    S::Error: HttpModeledError,
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
            let Some(selected) = selected_operation::<Op>(&parts.extensions) else {
                return Ok(empty_internal_server_error());
            };
            let protocol = selected.protocol();
            let operation = selected.operation();
            if operation.input().members().iter().any(|member| member.streaming()) {
                error!("streaming operation routed through DynUpgrade");
                return Ok(empty_internal_server_error());
            }
            let extractors = match Extractors::from_parts(&mut parts) {
                Ok(value) => value,
                Err(err) => return Ok(err.into_response()),
            };
            let converted = match convert_request(parts, body) {
                Ok(request) => request.into_parts(),
                Err(err) => return Ok(protocol.serialize_rejection(err)),
            };
            if let Err(err) = protocol.check_accept(operation.output(), &converted.headers) {
                return Ok(protocol.serialize_rejection(err));
            }
            let bytes = if protocol.reads_request_body(operation.input()) {
                match collect_request_body(converted.body, &config).await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return Ok(crate::schema::body_collection_rejection(&**protocol, err.map_body_error(crate::Error::new)))
                    }
                }
            } else {
                bytes::Bytes::new()
            };
            let request = ServerRequest {
                uri: converted.uri,
                headers: converted.headers,
                body: bytes,
            };
            let input = {
                let mut deserializer = match protocol.deserialize_request(operation.input(), &request) {
                    Ok(value) => value,
                    Err(err) => return Ok(protocol.serialize_rejection(err)),
                };
                match Op::Input::deserialize(&mut *deserializer) {
                    Ok(value) => value,
                    Err(err) => return Ok(protocol.serialize_rejection(err)),
                }
            };
            match service.oneshot((input, extractors)).await {
                Ok(output) => Ok(protocol.serialize_response(operation.output(), &output)),
                Err(err) => Ok(protocol.serialize_error(&err)),
            }
        })
    }
}

/// Schema-driven, protocol-neutral HTTP upgrade plugin for operations with a streaming member.
///
/// The request body is never collected when the input streams; it is handed to the generated
/// [`StreamingOperationShape`] glue as an [`SdkBody`]. That conversion needs the body to be
/// `Sync`, so services with streaming operations run on a `Sync` body such as
/// [`BoxBodySync`](crate::body::BoxBodySync) or hyper's incoming body.
#[derive(Debug, Clone)]
pub struct StreamingUpgradePlugin<Extractors> {
    config: RequestBodyCollectionConfig,
    _extractors: PhantomData<Extractors>,
}

impl<Extractors> StreamingUpgradePlugin<Extractors> {
    pub fn new(config: RequestBodyCollectionConfig) -> Self {
        Self {
            config,
            _extractors: PhantomData,
        }
    }
}

impl<Ser, Op, T, Extractors> Plugin<Ser, Op, T> for StreamingUpgradePlugin<Extractors>
where
    Ser: ServiceShape,
    Op: StreamingOperationShape,
{
    type Output = StreamingUpgrade<Op, Extractors, T>;
    fn apply(&self, inner: T) -> Self::Output {
        StreamingUpgrade {
            config: self.config,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner,
        }
    }
}

/// Upgrade service for a schema operation with a streaming input or output.
///
/// A streaming input reaches the protocol with an empty [`ServerRequest`] body, so the protocol
/// reads URI and header bindings only; the live body goes to
/// [`StreamingOperationShape::deserialize_streaming_input`]. A non-streaming input on such an
/// operation is collected exactly as [`DynUpgrade`] collects it.
pub struct StreamingUpgrade<Op, Extractors, S> {
    config: RequestBodyCollectionConfig,
    _operation: PhantomData<Op>,
    _extractors: PhantomData<Extractors>,
    inner: S,
}

impl<Op, Extractors, S: Clone> Clone for StreamingUpgrade<Op, Extractors, S> {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner: self.inner.clone(),
        }
    }
}

impl<Op, Extractors, B, S> Service<http::Request<B>> for StreamingUpgrade<Op, Extractors, S>
where
    Op: StreamingOperationShape,
    Op::Input: Send + 'static,
    Op::Output: Send + 'static,
    Extractors: FromParts<DynProtocol> + Send + 'static,
    <Extractors as FromParts<DynProtocol>>::Rejection: std::fmt::Display,
    B: HttpBody<Data = bytes::Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    S: Service<(Op::Input, Extractors), Response = Op::Output> + Clone + Send + 'static,
    S::Error: HttpModeledError,
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
            let Some(selected) = selected_operation::<Op>(&parts.extensions) else {
                return Ok(empty_internal_server_error());
            };
            let protocol = selected.protocol();
            let operation = selected.operation();
            let has_event_stream = [operation.input(), operation.output()].iter().any(|schema| {
                schema
                    .members()
                    .iter()
                    .any(|member| member.streaming() && member.shape_type() == aws_smithy_schema::ShapeType::Union)
            });
            if has_event_stream && protocol.event_stream().is_none() {
                error!(operation = %operation.shape_id(), protocol = %protocol.protocol_id(),
                    "selected protocol does not support event streams");
                return Ok(empty_internal_server_error());
            }

            let extractors = match Extractors::from_parts(&mut parts) {
                Ok(value) => value,
                Err(err) => return Ok(err.into_response()),
            };
            let converted = match convert_request(parts, body) {
                Ok(request) => request.into_parts(),
                Err(err) => return Ok(protocol.serialize_rejection(err)),
            };
            if let Err(err) = protocol.check_accept(operation.output(), &converted.headers) {
                return Ok(protocol.serialize_rejection(err));
            }
            let input_streams = operation.input().members().iter().any(|member| member.streaming());
            let (bytes, body) = if input_streams {
                (bytes::Bytes::new(), SdkBody::from_body_1_x(converted.body))
            } else if protocol.reads_request_body(operation.input()) {
                match collect_request_body(converted.body, &config).await {
                    Ok(bytes) => (bytes, SdkBody::empty()),
                    Err(err) => {
                        return Ok(crate::schema::body_collection_rejection(&**protocol, err.map_body_error(crate::Error::new)))
                    }
                }
            } else {
                (bytes::Bytes::new(), SdkBody::empty())
            };
            let request = ServerRequest {
                uri: converted.uri,
                headers: converted.headers,
                body: bytes,
            };
            // The deserializer borrows the request and is not `Send`; the walk over it happens
            // inside `deserialize_streaming_input` before the future is returned, so it is dropped
            // before the first await.
            let future = {
                let mut deserializer = match protocol.deserialize_request(operation.input(), &request) {
                    Ok(value) => value,
                    Err(err) => return Ok(protocol.serialize_rejection(err)),
                };
                Op::deserialize_streaming_input(&mut *deserializer, body, protocol.clone())
            };
            let input = match future.await {
                Ok(value) => value,
                Err(err) => return Ok(protocol.serialize_rejection(err)),
            };
            match service.oneshot((input, extractors)).await {
                Ok(output) => Ok(Op::serialize_streaming_output(output, protocol)),
                Err(err) => Ok(protocol.serialize_error(&err)),
            }
        })
    }
}

/// An empty `500 Internal Server Error`: the answer when generated glue cannot even build a
/// response, such as a failure to serialize an `initial-response` frame.
#[doc(hidden)]
pub fn empty_internal_server_error() -> http::Response<BoxBody> {
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

#[cfg(test)]
mod tests;

/// Missing-handler fallback using the protocol selected by schema routing.
#[derive(Clone, Copy, Debug, Default)]
pub struct SchemaMissingFailure;
impl Service<http::Request<crate::body::Body>> for SchemaMissingFailure {
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<crate::body::Body>) -> Self::Future {
        error!("the operation has not been set");
        let selected = request
            .extensions()
            .get::<crate::schema::SelectedProtocolOperation>()
            .expect("schema fallback requires selected protocol context");
        let rejection = crate::schema::DeserializeError::InternalFailure(crate::Error::new(String::from(
            "the operation has not been set",
        )));
        std::future::ready(Ok(selected.protocol().serialize_rejection(rejection)))
    }
}

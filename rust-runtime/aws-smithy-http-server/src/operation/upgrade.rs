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
    body::BoxBody,
    deserialize::{DeserializableShape, DeserializeError, RequestDeserializationError},
    error::BoxError,
    plugin::Plugin,
    request::{FromParts, FromRequest},
    response::IntoResponse,
    routing::SelectedProtocolContext,
    runtime_error::InternalFailureException,
    service::ServiceShape,
};
use aws_smithy_schema::{Schema, ShapeId};

use super::{DynOutput, IntoDynProtocolResponse, OperationShape, SchemaOperationShape};

/// A [`Plugin`] responsible for taking an operation [`Service`], accepting and returning Smithy
/// types and converting it into a [`Service`] taking and returning [`http`] types.
///
/// See [`Upgrade`].
#[derive(Debug, Clone)]
pub struct UpgradePlugin<Extractors> {
    _extractors: PhantomData<Extractors>,
}

/// Marker used for protocol-agnostic [`FromParts`] extraction in
/// [`DynUpgrade`].
pub struct DynProtocol;

/// Dynamic schema upgrade plugin.
#[derive(Debug, Clone)]
pub struct DynUpgradePlugin<Extractors> {
    request_body_max_bytes: usize,
    _extractors: PhantomData<Extractors>,
}

impl<Extractors> DynUpgradePlugin<Extractors> {
    /// Creates a dynamic upgrade plugin with the given non-streaming request
    /// body byte limit. `0` disables the limit.
    pub fn new(request_body_max_bytes: usize) -> Self {
        Self {
            request_body_max_bytes,
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
            request_body_max_bytes: self.request_body_max_bytes,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner,
        }
    }
}

/// Dynamic schema upgrade service.
pub struct DynUpgrade<Op, Extractors, S> {
    request_body_max_bytes: usize,
    _operation: PhantomData<Op>,
    _extractors: PhantomData<Extractors>,
    inner: S,
}

impl<Op, Extractors, S> Clone for DynUpgrade<Op, Extractors, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            request_body_max_bytes: self.request_body_max_bytes,
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
    Op::Output: DynOutput + Send + 'static,
    Op::Error: IntoDynProtocolResponse + Send + 'static,
    Extractors: FromParts<DynProtocol> + Send + 'static,
    <Extractors as FromParts<DynProtocol>>::Rejection: std::fmt::Display,
    S: Service<(Op::Input, Extractors), Response = Op::Output, Error = Op::Error> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: crate::body::HttpBody<Data = bytes::Bytes> + Send + 'static,
    B::Error: Into<BoxError> + std::fmt::Display + Send + 'static,
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

        let request_body_max_bytes = self.request_body_max_bytes;

        Box::pin(async move {
            let (mut parts, body) = req.into_parts();
            let Some(selected) = parts.extensions.get::<SelectedProtocolContext>().cloned() else {
                tracing::error!("selected protocol context missing from request extensions");
                return Ok(internal_server_error());
            };
            let protocol = selected.server_protocol().clone();

            let extractors = match Extractors::from_parts(&mut parts) {
                Ok(extractors) => extractors,
                Err(err) => {
                    tracing::error!(error = %err, "additional parameter for the handler function could not be constructed");
                    return Ok(err.into_response());
                }
            };

            let input_schema = Op::SCHEMA.input();
            let bytes = if input_schema_needs_body(protocol.protocol_id(), input_schema) {
                match crate::body::collect_body_limited(body, request_body_max_bytes).await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::debug!(error = %err, "failed to collect request body");
                        let mut response = http::Response::new(crate::body::to_boxed(err.to_string()));
                        *response.status_mut() = http::StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                }
            } else {
                bytes::Bytes::new()
            };

            let request = http::Request::from_parts(parts, bytes);
            let input = {
                let mut deserializer = match protocol.deserialize_request(&request, input_schema) {
                    Ok(deserializer) => deserializer,
                    Err(err) => return Ok(protocol.serialize_error(&*err)),
                };
                match Op::Input::deserialize(&mut *deserializer) {
                    Ok(input) => input,
                    Err(DeserializeError::Serde(err)) => {
                        return Ok(protocol.serialize_error(&RequestDeserializationError::new(err)));
                    }
                    Err(DeserializeError::ConstraintViolation(err)) => return Ok(protocol.serialize_error(&*err)),
                }
            };

            let result = service.oneshot((input, extractors)).await;
            let response = match result {
                Ok(output) => protocol.serialize_response(output.schema(), &output),
                Err(error) => error.into_dyn_response(&*protocol),
            };
            Ok(response)
        })
    }
}

fn input_schema_needs_body(protocol_id: &ShapeId<'_>, input_schema: &Schema<'_>) -> bool {
    match protocol_id.as_str() {
        "aws.protocols#restJson1" | "aws.protocols#restXml" => rest_input_schema_needs_body(input_schema),
        "aws.protocols#awsJson1_0" | "aws.protocols#awsJson1_1" | "smithy.protocols#rpcv2Cbor" => {
            !input_schema.members().is_empty()
        }
        // Internal decorator-provided protocols may interpret inputs in
        // protocol-specific ways, so keep the previous conservative behavior.
        _ => true,
    }
}

fn rest_input_schema_needs_body(input_schema: &Schema<'_>) -> bool {
    input_schema.members().iter().any(|member| {
        member.http_payload().is_some()
            || (member.http_label().is_none()
                && member.http_query().is_none()
                && member.http_header().is_none()
                && member.http_prefix_headers().is_none()
                && member.http_query_params().is_none())
    })
}

fn internal_server_error() -> http::Response<BoxBody> {
    http::Response::builder()
        .status(http::StatusCode::INTERNAL_SERVER_ERROR)
        .body(crate::body::empty())
        .expect("valid internal server error response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        routing::SelectedProtocolContext,
        schema::{protocol::SharedServerProtocol, OperationSchema},
    };
    use aws_smithy_schema::{
        codec::DynCodec,
        serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer},
        shape_id, ShapeType,
    };
    use bytes::Bytes;
    use http_body::{Frame, SizeHint};
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    static EMPTY_MEMBERS: &[&Schema<'static>] = &[];
    static LABEL_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BoundOnlyInput$name"), ShapeType::String, "name", 0).with_http_label();
    static QUERY_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BoundOnlyInput$age"), ShapeType::Integer, "age", 1)
            .with_http_query("age");
    static HEADER_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BoundOnlyInput$token"), ShapeType::String, "token", 2)
            .with_http_header("x-token");
    static PREFIX_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BoundOnlyInput$meta"), ShapeType::Map, "meta", 3)
            .with_http_prefix_headers("x-meta-");
    static QUERY_PARAMS_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BoundOnlyInput$params"), ShapeType::Map, "params", 4)
            .with_http_query_params();
    static BOUND_ONLY_MEMBERS: &[&Schema<'static>] = &[
        &LABEL_MEMBER,
        &QUERY_MEMBER,
        &HEADER_MEMBER,
        &PREFIX_MEMBER,
        &QUERY_PARAMS_MEMBER,
    ];
    static PAYLOAD_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "PayloadInput$body"), ShapeType::Blob, "body", 0).with_http_payload();
    static PAYLOAD_MEMBERS: &[&Schema<'static>] = &[&PAYLOAD_MEMBER];
    static UNBOUND_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BodyInput$value"), ShapeType::String, "value", 0);
    static UNBOUND_MEMBERS: &[&Schema<'static>] = &[&UNBOUND_MEMBER];

    static BOUND_ONLY_INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("test", "BoundOnlyInput"),
        ShapeType::Structure,
        BOUND_ONLY_MEMBERS,
    );
    static EMPTY_INPUT_SCHEMA: Schema<'static> =
        Schema::new_struct(shape_id!("test", "EmptyInput"), ShapeType::Structure, EMPTY_MEMBERS);
    static PAYLOAD_INPUT_SCHEMA: Schema<'static> =
        Schema::new_struct(shape_id!("test", "PayloadInput"), ShapeType::Structure, PAYLOAD_MEMBERS);
    static UNBOUND_INPUT_SCHEMA: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BodyInput"), ShapeType::Structure, UNBOUND_MEMBERS);
    static OUTPUT_SCHEMA: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, EMPTY_MEMBERS);
    static OPERATION_SCHEMA_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "Operation"), ShapeType::Operation);
    static OPERATION_SCHEMA: OperationSchema<'static> =
        OperationSchema::new(&OPERATION_SCHEMA_SHAPE, &BOUND_ONLY_INPUT_SCHEMA, &OUTPUT_SCHEMA, &[]);

    #[test]
    fn rest_input_schema_body_classification() {
        let rest_json = ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1");

        assert!(!input_schema_needs_body(&rest_json, &BOUND_ONLY_INPUT_SCHEMA));
        assert!(!input_schema_needs_body(&rest_json, &EMPTY_INPUT_SCHEMA));
        assert!(input_schema_needs_body(&rest_json, &PAYLOAD_INPUT_SCHEMA));
        assert!(input_schema_needs_body(&rest_json, &UNBOUND_INPUT_SCHEMA));
    }

    #[test]
    fn rpc_input_schema_body_classification() {
        let aws_json = ShapeId::from_parts("aws.protocols#awsJson1_1", "aws.protocols", "awsJson1_1");
        let rpc_v2_cbor = ShapeId::from_parts("smithy.protocols#rpcv2Cbor", "smithy.protocols", "rpcv2Cbor");

        assert!(!input_schema_needs_body(&aws_json, &EMPTY_INPUT_SCHEMA));
        assert!(input_schema_needs_body(&aws_json, &BOUND_ONLY_INPUT_SCHEMA));
        assert!(!input_schema_needs_body(&rpc_v2_cbor, &EMPTY_INPUT_SCHEMA));
        assert!(input_schema_needs_body(&rpc_v2_cbor, &UNBOUND_INPUT_SCHEMA));
    }

    #[tokio::test]
    async fn dyn_upgrade_does_not_collect_rest_bound_only_input_body() {
        let protocol = SharedServerProtocol::new(TestProtocol {
            protocol_id: ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
        });
        let mut request = http::Request::builder()
            .uri("/pets/rex?age=7")
            .body(PanicBody)
            .expect("valid request");
        request
            .extensions_mut()
            .insert(SelectedProtocolContext::new(protocol, &OPERATION_SCHEMA));

        let service = tower::service_fn(|(input, ()): (TestInput, ())| async move {
            assert_eq!(input.body_len, 0);
            Ok::<_, Infallible>(TestOutput)
        });
        let mut upgrade = DynUpgrade::<TestOperation, (), _> {
            request_body_max_bytes: 1024,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner: service,
        };

        let response = upgrade.call(request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);
    }

    #[tokio::test]
    async fn dyn_upgrade_collects_rest_payload_input_body() {
        let protocol = SharedServerProtocol::new(TestProtocol {
            protocol_id: ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
        });
        let mut request = http::Request::builder()
            .body(http_body_util::Full::new(Bytes::from_static(b"payload")))
            .expect("valid request");
        request
            .extensions_mut()
            .insert(SelectedProtocolContext::new(protocol, &PAYLOAD_OPERATION_SCHEMA));

        let service = tower::service_fn(|(input, ()): (TestInput, ())| async move {
            assert_eq!(input.body_len, 7);
            Ok::<_, Infallible>(TestOutput)
        });
        let mut upgrade = DynUpgrade::<PayloadOperation, (), _> {
            request_body_max_bytes: 1024,
            _operation: PhantomData,
            _extractors: PhantomData,
            inner: service,
        };

        let response = upgrade.call(request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);
    }

    static PAYLOAD_OPERATION_SCHEMA_SHAPE: Schema<'static> =
        Schema::new(shape_id!("test", "PayloadOperation"), ShapeType::Operation);
    static PAYLOAD_OPERATION_SCHEMA: OperationSchema<'static> = OperationSchema::new(
        &PAYLOAD_OPERATION_SCHEMA_SHAPE,
        &PAYLOAD_INPUT_SCHEMA,
        &OUTPUT_SCHEMA,
        &[],
    );

    struct TestOperation;
    impl OperationShape for TestOperation {
        const ID: crate::shape_id::ShapeId = crate::shape_id::ShapeId::new("test#Operation", "test", "Operation");
        type Input = TestInput;
        type Output = TestOutput;
        type Error = Infallible;
    }
    impl SchemaOperationShape for TestOperation {
        const SCHEMA: &'static OperationSchema<'static> = &OPERATION_SCHEMA;
    }

    struct PayloadOperation;
    impl OperationShape for PayloadOperation {
        const ID: crate::shape_id::ShapeId =
            crate::shape_id::ShapeId::new("test#PayloadOperation", "test", "PayloadOperation");
        type Input = TestInput;
        type Output = TestOutput;
        type Error = Infallible;
    }
    impl SchemaOperationShape for PayloadOperation {
        const SCHEMA: &'static OperationSchema<'static> = &PAYLOAD_OPERATION_SCHEMA;
    }

    #[derive(Debug)]
    struct TestInput {
        body_len: usize,
    }

    impl DeserializableShape for TestInput {
        fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
            Ok(Self {
                body_len: deserializer.read_integer(&BOUND_ONLY_INPUT_SCHEMA)? as usize,
            })
        }
    }

    struct TestOutput;

    impl DynOutput for TestOutput {
        fn schema(&self) -> &Schema<'_> {
            &OUTPUT_SCHEMA
        }
    }

    impl SerializableStruct for TestOutput {
        fn serialize_members(&self, _serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct TestProtocol {
        protocol_id: ShapeId<'static>,
    }

    impl crate::schema::protocol::ServerProtocolInner for TestProtocol {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol_id
        }

        fn codec(&self) -> &dyn DynCodec {
            panic!("test protocol codec should not be used")
        }

        fn deserialize_request<'a>(
            &self,
            request: &'a http::Request<Bytes>,
            _input_schema: &Schema<'_>,
        ) -> Result<Box<dyn ShapeDeserializer + 'a>, crate::modeled_error::ServerError> {
            Ok(Box::new(BodyLenDeserializer {
                body_len: request.body().len(),
            }))
        }

        fn serialize_response(
            &self,
            _schema: &Schema<'_>,
            _output: &dyn SerializableStruct,
        ) -> http::Response<BoxBody> {
            http::Response::new(crate::body::empty())
        }

        fn serialize_error(&self, _error: &dyn crate::modeled_error::HttpServerError) -> http::Response<BoxBody> {
            http::Response::builder()
                .status(http::StatusCode::BAD_REQUEST)
                .body(crate::body::empty())
                .expect("valid error response")
        }
    }

    struct BodyLenDeserializer {
        body_len: usize,
    }

    impl ShapeDeserializer for BodyLenDeserializer {
        fn read_struct(
            &mut self,
            _schema: &Schema<'_>,
            _state: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn read_list(
            &mut self,
            _schema: &Schema<'_>,
            _state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_map(
            &mut self,
            _schema: &Schema<'_>,
            _state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
            Ok(self.body_len as i32)
        }

        fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<aws_smithy_types::BigInteger, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<aws_smithy_types::BigDecimal, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<aws_smithy_types::Blob, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<aws_smithy_types::DateTime, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn read_document(&mut self, _schema: &Schema<'_>) -> Result<aws_smithy_types::Document, SerdeError> {
            Err(SerdeError::unsupported("test deserializer only reads body length"))
        }

        fn is_null(&self) -> bool {
            false
        }

        fn container_size(&self) -> Option<usize> {
            None
        }
    }

    struct PanicBody;

    impl http_body::Body for PanicBody {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            panic!("body should not be polled for REST inputs with only HTTP-bound members")
        }

        fn is_end_stream(&self) -> bool {
            false
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::with_exact(7)
        }
    }
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

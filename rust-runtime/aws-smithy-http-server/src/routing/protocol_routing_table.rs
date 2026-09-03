/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::{Schema, ShapeId, ShapeType};
use http::{HeaderMap, Method, Request, Uri};
use std::{any::Any, borrow::Cow, marker::PhantomData, sync::Arc};

use crate::{
    body::{Body, BoxBody},
    deserialize::{DeserializeError, RequestDeserializationError},
    modeled_error::{HttpModeledError, HttpServerError, ServerError},
    protocol::{
        accept_header_classifier,
        aws_json::{
            rejection as aws_json_rejection,
            router::{AwsJsonRouter, Error as AwsJsonError},
            runtime_error as aws_json_runtime_error,
        },
        aws_json_10::AwsJson1_0,
        aws_json_11::AwsJson1_1,
        rest::router::Error as RestError,
        rest_json_1::{rejection as rest_json_rejection, runtime_error as rest_json_runtime_error, RestJson1},
        rest_xml::{rejection as rest_xml_rejection, runtime_error as rest_xml_runtime_error, RestXml},
        rpc_v2_cbor::{
            rejection as rpc_v2_cbor_rejection, router::RpcV2CborRouter, runtime_error as rpc_v2_cbor_runtime_error,
            RpcV2Cbor,
        },
    },
    response::IntoResponse,
    routing::{
        operation_handler_bindings::{rest_request_spec, service_operation_key},
        request_spec::{LabelCapture, Match, RequestSpec},
        Router,
    },
    schema::{
        protocol::{
            aws_json as schema_aws_json, rest_json as schema_rest_json, rest_xml as schema_rest_xml,
            rpc_v2_cbor as schema_rpc_v2_cbor, DeserializeInputFuture, DynInputDeserializer, ServerProtocolInner,
            SharedServerProtocol,
        },
        OperationSchema, ServiceSchema,
    },
};

/// Common request metadata used by protocol routing tables.
#[derive(Debug, Clone, Copy)]
pub struct RequestRouteMetadata<'a> {
    method: &'a Method,
    uri: &'a Uri,
    headers: &'a HeaderMap,
}

impl<'a> RequestRouteMetadata<'a> {
    /// Creates route metadata from an HTTP request.
    pub fn from_request<B>(request: &'a Request<B>) -> Self {
        Self {
            method: request.method(),
            uri: request.uri(),
            headers: request.headers(),
        }
    }

    fn to_bodyless_request(self) -> Request<()> {
        self.to_bodyless_request_with_path(self.uri.path())
    }

    fn to_bodyless_request_with_path(self, path: &str) -> Request<()> {
        let path_and_query = if let Some(query) = self.uri.query() {
            format!("{path}?{query}")
        } else {
            path.to_owned()
        };
        let mut request = Request::new(());
        *request.method_mut() = self.method.clone();
        *request.uri_mut() = path_and_query
            .parse()
            .expect("prefix-adjusted path and query should be a valid URI");
        *request.headers_mut() = self.headers.clone();
        request
    }
}

/// Protocol and operation selected by routing.
#[derive(Debug, Clone)]
pub struct SelectedProtocolContext {
    server_protocol: SharedServerProtocol,
    operation: &'static OperationSchema<'static>,
    route_match: Option<RouteMatchData>,
}

impl SelectedProtocolContext {
    /// Creates selected protocol context.
    pub fn new(server_protocol: SharedServerProtocol, operation: &'static OperationSchema<'static>) -> Self {
        Self {
            server_protocol,
            operation,
            route_match: None,
        }
    }

    /// Creates selected protocol context with protocol-owned route match data.
    pub fn new_with_route_match(
        server_protocol: SharedServerProtocol,
        operation: &'static OperationSchema<'static>,
        route_match: Option<RouteMatchData>,
    ) -> Self {
        Self {
            server_protocol,
            operation,
            route_match,
        }
    }

    /// Returns the selected protocol shape ID.
    pub fn protocol(&self) -> &ShapeId<'static> {
        self.server_protocol.protocol_id()
    }

    /// Returns the selected erased server protocol.
    pub fn server_protocol(&self) -> &SharedServerProtocol {
        &self.server_protocol
    }

    /// Returns the matched operation schema.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.operation
    }

    /// Returns protocol-owned route match data.
    pub fn route_match(&self) -> Option<&RouteMatchData> {
        self.route_match.as_ref()
    }
}

/// Protocol-owned data captured during routing.
#[derive(Clone)]
pub struct RouteMatchData {
    inner: Arc<dyn Any + Send + Sync>,
}

impl RouteMatchData {
    /// Creates route match data from a concrete protocol-owned value.
    pub fn new<T>(data: T) -> Self
    where
        T: Any + Send + Sync,
    {
        Self { inner: Arc::new(data) }
    }

    /// Attempts to downcast the route match data.
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        self.inner.downcast_ref()
    }
}

impl std::fmt::Debug for RouteMatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteMatchData").finish_non_exhaustive()
    }
}

/// REST route data captured during routing.
#[derive(Debug, Clone)]
pub struct RestRouteMatch {
    labels: Vec<LabelCapture>,
}

impl RestRouteMatch {
    /// Creates a REST route match.
    pub fn new(labels: Vec<LabelCapture>) -> Self {
        Self { labels }
    }

    /// Returns the decoded URI label captures.
    pub fn labels(&self) -> &[LabelCapture] {
        &self.labels
    }
}

/// A matched operation and its selected protocol context.
#[derive(Debug, Clone)]
pub struct OperationMatch {
    operation: &'static OperationSchema<'static>,
    route_match: Option<RouteMatchData>,
}

impl OperationMatch {
    /// Creates a matched operation.
    pub fn new(operation: &'static OperationSchema<'static>) -> Self {
        Self {
            operation,
            route_match: None,
        }
    }

    /// Creates a matched operation with protocol-owned route data.
    pub fn new_with_route_match(
        operation: &'static OperationSchema<'static>,
        route_match: Option<RouteMatchData>,
    ) -> Self {
        Self { operation, route_match }
    }

    /// Returns the matched operation schema.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.operation
    }

    /// Returns protocol-owned route match data.
    pub fn route_match(&self) -> Option<RouteMatchData> {
        self.route_match.clone()
    }
}

/// Result of asking one protocol routing table to route a request.
pub enum ProtocolRoutingOutcome {
    /// This protocol has no claim on the request.
    NoClaim,
    /// This protocol selected an operation.
    OperationMatched(OperationMatch),
    /// This protocol rejected the request and later protocols must not be tried.
    Rejected(Box<dyn IntoProtocolResponse>),
    /// This protocol produced a candidate rejection, but later protocols may still match.
    RejectedNonExclusive(Box<dyn IntoProtocolResponse>),
}

/// Protocol-owned conversion into an HTTP response.
pub trait IntoProtocolResponse: Send {
    /// Converts this value into an HTTP response.
    fn into_response(self: Box<Self>) -> http::Response<BoxBody>;
}

/// Protocol routing behavior, separate from protocol serialization and deserialization.
pub trait ProtocolRouter: Send + Sync + std::fmt::Debug {
    /// Returns the Smithy protocol shape ID this router selects.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Attempts to route request metadata to an operation for this protocol.
    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome;
}

impl IntoProtocolResponse for http::Response<BoxBody> {
    fn into_response(self: Box<Self>) -> http::Response<BoxBody> {
        *self
    }
}

/// Bridge from a concrete protocol-owned value to the erased protocol response type.
pub struct ProtocolResponse<T, P> {
    inner: T,
    _protocol: PhantomData<P>,
}

impl<T, P> ProtocolResponse<T, P>
where
    T: IntoResponse<P> + Send + 'static,
    P: Send + 'static,
{
    /// Creates a bridge value for a concrete protocol-owned value.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            _protocol: PhantomData,
        }
    }

    /// Creates a boxed bridge value for a concrete protocol-owned value.
    pub fn boxed(inner: T) -> Box<dyn IntoProtocolResponse> {
        Box::new(Self::new(inner))
    }
}

impl<T, P> IntoProtocolResponse for ProtocolResponse<T, P>
where
    T: IntoResponse<P> + Send + 'static,
    P: Send + 'static,
{
    fn into_response(self: Box<Self>) -> http::Response<BoxBody> {
        IntoResponse::<P>::into_response(self.inner)
    }
}

fn request_deserialization_error(err: &RequestDeserializationError) -> DeserializeError {
    DeserializeError::Serde(aws_smithy_schema::serde::SerdeError::custom(err.source().to_string()))
}

fn modeled_or_bad_request_response(
    error: &dyn HttpServerError,
    modeled_response: impl FnOnce(&dyn HttpModeledError) -> http::Response<BoxBody>,
    request_deserialization_response: impl FnOnce(&RequestDeserializationError) -> http::Response<BoxBody>,
) -> http::Response<BoxBody> {
    if let Some(modeled) = error.as_modeled_error() {
        return modeled_response(modeled);
    }
    if let Some(err) = error.as_any().downcast_ref::<RequestDeserializationError>() {
        return request_deserialization_response(err);
    }

    let mut response = http::Response::new(crate::body::to_boxed(error.to_string()));
    *response.status_mut() =
        http::StatusCode::from_u16(error.status_code()).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    response
}

fn aws_json_request_deserialization_response<P>(err: &RequestDeserializationError) -> http::Response<BoxBody>
where
    aws_json_runtime_error::RuntimeError: IntoResponse<P>,
{
    let rejection = aws_json_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<P>::into_response(aws_json_runtime_error::RuntimeError::from(rejection))
}

fn rpc_v2_cbor_request_deserialization_response(err: &RequestDeserializationError) -> http::Response<BoxBody> {
    let rejection = rpc_v2_cbor_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RpcV2Cbor>::into_response(rpc_v2_cbor_runtime_error::RuntimeError::from(rejection))
}

fn rest_json_request_deserialization_response(err: &RequestDeserializationError) -> http::Response<BoxBody> {
    let rejection = rest_json_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RestJson1>::into_response(rest_json_runtime_error::RuntimeError::from(rejection))
}

fn rest_xml_request_deserialization_response(err: &RequestDeserializationError) -> http::Response<BoxBody> {
    let rejection = rest_xml_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RestXml>::into_response(rest_xml_runtime_error::RuntimeError::from(rejection))
}

fn aws_json_request_rejection_response<P>(err: &AwsJsonRequestRejection) -> http::Response<BoxBody>
where
    aws_json_runtime_error::RuntimeError: IntoResponse<P>,
{
    let runtime_error = match &err.0 {
        aws_json_rejection::RequestRejection::ConstraintViolation(reason) => {
            aws_json_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => aws_json_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<P>::into_response(runtime_error)
}

fn rpc_v2_cbor_request_rejection_response(err: &RpcV2CborRequestRejection) -> http::Response<BoxBody> {
    let runtime_error = match &err.0 {
        rpc_v2_cbor_rejection::RequestRejection::ConstraintViolation(reason) => {
            rpc_v2_cbor_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rpc_v2_cbor_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RpcV2Cbor>::into_response(runtime_error)
}

fn rest_json_request_rejection_response(err: &RestJsonRequestRejection) -> http::Response<BoxBody> {
    let runtime_error = match &err.0 {
        rest_json_rejection::RequestRejection::MissingContentType(_) => {
            rest_json_runtime_error::RuntimeError::UnsupportedMediaType
        }
        rest_json_rejection::RequestRejection::NotAcceptable => rest_json_runtime_error::RuntimeError::NotAcceptable,
        rest_json_rejection::RequestRejection::ConstraintViolation(reason) => {
            rest_json_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rest_json_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RestJson1>::into_response(runtime_error)
}

fn rest_xml_request_rejection_response(err: &RestXmlRequestRejection) -> http::Response<BoxBody> {
    let runtime_error = match &err.0 {
        rest_xml_rejection::RequestRejection::MissingContentType(_) => {
            rest_xml_runtime_error::RuntimeError::UnsupportedMediaType
        }
        rest_xml_rejection::RequestRejection::ConstraintViolation(reason) => {
            rest_xml_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rest_xml_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RestXml>::into_response(runtime_error)
}

#[derive(Debug)]
struct AwsJsonRequestRejection(aws_json_rejection::RequestRejection);

#[derive(Debug)]
struct RpcV2CborRequestRejection(rpc_v2_cbor_rejection::RequestRejection);

#[derive(Debug)]
struct RestJsonRequestRejection(rest_json_rejection::RequestRejection);

#[derive(Debug)]
struct RestXmlRequestRejection(rest_xml_rejection::RequestRejection);

macro_rules! request_rejection_error {
    ($name:ident) => {
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::error::Error for $name {}

        impl HttpServerError for $name {
            fn status_code(&self) -> u16 {
                http::StatusCode::BAD_REQUEST.as_u16()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

request_rejection_error!(AwsJsonRequestRejection);
request_rejection_error!(RpcV2CborRequestRejection);
request_rejection_error!(RestJsonRequestRejection);
request_rejection_error!(RestXmlRequestRejection);

#[derive(Debug)]
struct DynModeledServerError(Box<dyn HttpModeledError>);

impl std::fmt::Display for DynModeledServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for DynModeledServerError {}

impl HttpServerError for DynModeledServerError {
    fn status_code(&self) -> u16 {
        HttpModeledError::status_code(&*self.0)
    }

    fn as_modeled_error(&self) -> Option<&dyn HttpModeledError> {
        Some(&*self.0)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn request_body_error(error: impl std::fmt::Display) -> ServerError {
    Box::new(RequestDeserializationError::new(
        aws_smithy_schema::serde::SerdeError::custom(error.to_string()),
    ))
}

async fn collect_request_body(
    request: http::Request<Body>,
    request_body_max_bytes: usize,
) -> Result<http::Request<bytes::Bytes>, ServerError> {
    let (parts, body) = request.into_parts();
    let bytes = crate::body::collect_body_limited(body, request_body_max_bytes)
        .await
        .map_err(request_body_error)?;
    Ok(http::Request::from_parts(parts, bytes))
}

fn empty_request_body(request: http::Request<Body>) -> http::Request<bytes::Bytes> {
    let (parts, _body) = request.into_parts();
    http::Request::from_parts(parts, bytes::Bytes::new())
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

fn drive_dynamic_input(
    input: Box<dyn DynInputDeserializer>,
    deserializer: &mut dyn aws_smithy_schema::serde::ShapeDeserializer,
) -> Result<Box<dyn std::any::Any + Send>, ServerError> {
    match input.deserialize(deserializer) {
        Ok(input) => Ok(input),
        Err(DeserializeError::Serde(err)) => Err(Box::new(RequestDeserializationError::new(err))),
        Err(DeserializeError::ConstraintViolation(err)) => Err(Box::new(DynModeledServerError(err))),
    }
}

/// AWS JSON operation routing table.
#[derive(Debug, Clone)]
pub struct AwsJsonOperationRoutingTable {
    protocol: ShapeId<'static>,
    router: AwsJsonRouter<&'static OperationSchema<'static>>,
    version: AwsJsonVersion,
}

#[derive(Debug, Clone, Copy)]
enum AwsJsonVersion {
    Json10,
    Json11,
}

impl AwsJsonOperationRoutingTable {
    /// Creates an AWS JSON 1.0 operation routing table from service schema metadata.
    pub fn new_aws_json_10(service_schema: &'static ServiceSchema<'static>) -> Self {
        let protocol = ShapeId::from_parts("aws.protocols#awsJson1_0", "aws.protocols", "awsJson1_0");
        Self::new(protocol.clone(), AwsJsonVersion::Json10, service_schema)
    }

    /// Creates an AWS JSON 1.1 operation routing table from service schema metadata.
    pub fn new_aws_json_11(service_schema: &'static ServiceSchema<'static>) -> Self {
        let protocol = ShapeId::from_parts("aws.protocols#awsJson1_1", "aws.protocols", "awsJson1_1");
        Self::new(protocol.clone(), AwsJsonVersion::Json11, service_schema)
    }

    fn new(
        protocol: ShapeId<'static>,
        version: AwsJsonVersion,
        service_schema: &'static ServiceSchema<'static>,
    ) -> Self {
        let router = service_schema
            .operations()
            .iter()
            .map(|operation| (service_operation_key(service_schema, operation), *operation))
            .collect();
        Self {
            protocol,
            router,
            version,
        }
    }

    fn rejection(&self, error: AwsJsonError) -> Box<dyn IntoProtocolResponse> {
        match self.version {
            AwsJsonVersion::Json10 => ProtocolResponse::<_, AwsJson1_0>::boxed(error),
            AwsJsonVersion::Json11 => ProtocolResponse::<_, AwsJson1_1>::boxed(error),
        }
    }
}

impl ProtocolRouter for AwsJsonOperationRoutingTable {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
        if !request.headers.contains_key("x-amz-target") {
            return ProtocolRoutingOutcome::NoClaim;
        }

        let route_request = if request.uri.path() == "/" {
            request.to_bodyless_request()
        } else {
            request.to_bodyless_request_with_path("/")
        };

        match self.router.match_route(&route_request) {
            Ok(operation)
                if operation
                    .prefix_policy()
                    .candidates(request.uri.path())
                    .any(|path| path == "/") =>
            {
                if self.accept_matches(request.headers, operation) {
                    ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(operation))
                } else {
                    ProtocolRoutingOutcome::RejectedNonExclusive(self.not_acceptable())
                }
            }
            Ok(_) => ProtocolRoutingOutcome::NoClaim,
            Err(error) => ProtocolRoutingOutcome::Rejected(self.rejection(error)),
        }
    }
}

/// AWS JSON server protocol implementation.
#[derive(Debug, Clone)]
pub struct AwsJsonServerProtocol {
    protocol: ShapeId<'static>,
    version: AwsJsonVersion,
}

impl AwsJsonServerProtocol {
    /// Creates an AWS JSON 1.0 server protocol.
    pub fn aws_json_10() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#awsJson1_0", "aws.protocols", "awsJson1_0"),
            version: AwsJsonVersion::Json10,
        }
    }

    /// Creates an AWS JSON 1.1 server protocol.
    pub fn aws_json_11() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#awsJson1_1", "aws.protocols", "awsJson1_1"),
            version: AwsJsonVersion::Json11,
        }
    }
}

impl ServerProtocolInner<http::Request<bytes::Bytes>> for AwsJsonServerProtocol {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn codec(&self) -> &dyn aws_smithy_schema::codec::DynCodec {
        schema_aws_json::aws_json_codec()
    }

    fn deserialize_request<'a>(
        &self,
        request: &'a http::Request<bytes::Bytes>,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>, ServerError> {
        match self.version {
            AwsJsonVersion::Json10 => {
                schema_aws_json::aws_json_request_deserializer("application/x-amz-json-1.0", input_schema, request)
                    .map_err(|rejection| Box::new(AwsJsonRequestRejection(rejection)) as ServerError)
            }
            AwsJsonVersion::Json11 => {
                schema_aws_json::aws_json_request_deserializer("application/x-amz-json-1.1", input_schema, request)
                    .map_err(|rejection| Box::new(AwsJsonRequestRejection(rejection)) as ServerError)
            }
        }
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a> {
        Box::pin(async move {
            let request = if input_schema.members().is_empty() {
                empty_request_body(request)
            } else {
                collect_request_body(request, request_body_max_bytes).await?
            };
            let mut deserializer = self.deserialize_request(&request, input_schema)?;
            drive_dynamic_input(input, &mut *deserializer)
        })
    }

    fn serialize_response(
        &self,
        schema: &Schema<'_>,
        output: &dyn aws_smithy_schema::serde::SerializableStruct,
    ) -> http::Response<BoxBody> {
        match self.version {
            AwsJsonVersion::Json10 => schema_aws_json::aws_json_10_serialize_response(schema, output),
            AwsJsonVersion::Json11 => schema_aws_json::aws_json_11_serialize_response(schema, output),
        }
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        match self.version {
            AwsJsonVersion::Json10 => {
                if let Some(err) = error.as_any().downcast_ref::<AwsJsonRequestRejection>() {
                    return aws_json_request_rejection_response::<AwsJson1_0>(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_aws_json::aws_json_10_serialize_error,
                    aws_json_request_deserialization_response::<AwsJson1_0>,
                )
            }
            AwsJsonVersion::Json11 => {
                if let Some(err) = error.as_any().downcast_ref::<AwsJsonRequestRejection>() {
                    return aws_json_request_rejection_response::<AwsJson1_1>(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_aws_json::aws_json_11_serialize_error,
                    aws_json_request_deserialization_response::<AwsJson1_1>,
                )
            }
        }
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        Some("application/json")
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        match self.version {
            AwsJsonVersion::Json10 => Some("application/x-amz-json-1.0"),
            AwsJsonVersion::Json11 => Some("application/x-amz-json-1.1"),
        }
    }

    fn frames_initial_messages(&self) -> bool {
        true
    }
}

impl AwsJsonOperationRoutingTable {
    fn accept_matches(&self, headers: &HeaderMap, _operation: &'static OperationSchema<'static>) -> bool {
        match self.version {
            AwsJsonVersion::Json10 => accept_matches_content_type(headers, "application/x-amz-json-1.0"),
            AwsJsonVersion::Json11 => accept_matches_content_type(headers, "application/x-amz-json-1.1"),
        }
    }

    fn not_acceptable(&self) -> Box<dyn IntoProtocolResponse> {
        match self.version {
            AwsJsonVersion::Json10 => ProtocolResponse::<_, AwsJson1_0>::boxed(
                aws_json_runtime_error::RuntimeError::from(aws_json_rejection::RequestRejection::NotAcceptable),
            ),
            AwsJsonVersion::Json11 => ProtocolResponse::<_, AwsJson1_1>::boxed(
                aws_json_runtime_error::RuntimeError::from(aws_json_rejection::RequestRejection::NotAcceptable),
            ),
        }
    }
}

/// RPC v2 CBOR operation routing table.
#[derive(Debug, Clone)]
pub struct RpcV2CborOperationRoutingTable {
    protocol: ShapeId<'static>,
    router: RpcV2CborRouter<&'static OperationSchema<'static>>,
}

impl RpcV2CborOperationRoutingTable {
    /// Creates an RPC v2 CBOR operation routing table from service schema metadata.
    pub fn new(service_schema: &'static ServiceSchema<'static>) -> Self {
        let protocol = ShapeId::from_parts("smithy.protocols#rpcv2Cbor", "smithy.protocols", "rpcv2Cbor");
        let router = service_schema
            .operations()
            .iter()
            .map(|operation| (service_operation_key(service_schema, operation), *operation))
            .collect();
        Self { protocol, router }
    }
}

impl ProtocolRouter for RpcV2CborOperationRoutingTable {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
        if !request.headers.contains_key("smithy-protocol") {
            return ProtocolRoutingOutcome::NoClaim;
        }

        match self.router.match_route(&request.to_bodyless_request()) {
            Ok(operation) if rpc_path_matches_prefix_policy(request.uri.path(), operation) => {
                if accept_matches_content_type(request.headers, "application/cbor") {
                    ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(operation))
                } else {
                    ProtocolRoutingOutcome::RejectedNonExclusive(ProtocolResponse::<_, RpcV2Cbor>::boxed(
                        rpc_v2_cbor_runtime_error::RuntimeError::from(
                            rpc_v2_cbor_rejection::RequestRejection::NotAcceptable,
                        ),
                    ))
                }
            }
            Ok(_) => ProtocolRoutingOutcome::NoClaim,
            Err(error) => ProtocolRoutingOutcome::Rejected(ProtocolResponse::<_, RpcV2Cbor>::boxed(error)),
        }
    }
}

/// RPC v2 CBOR server protocol implementation.
#[derive(Debug, Clone)]
pub struct RpcV2CborServerProtocol {
    protocol: ShapeId<'static>,
}

impl RpcV2CborServerProtocol {
    /// Creates an RPC v2 CBOR server protocol.
    pub fn new() -> Self {
        Self {
            protocol: ShapeId::from_parts("smithy.protocols#rpcv2Cbor", "smithy.protocols", "rpcv2Cbor"),
        }
    }
}

impl Default for RpcV2CborServerProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerProtocolInner<http::Request<bytes::Bytes>> for RpcV2CborServerProtocol {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn codec(&self) -> &dyn aws_smithy_schema::codec::DynCodec {
        schema_rpc_v2_cbor::rpc_v2_cbor_codec()
    }

    fn deserialize_request<'a>(
        &self,
        request: &'a http::Request<bytes::Bytes>,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>, ServerError> {
        schema_rpc_v2_cbor::rpc_v2_cbor_request_deserializer(input_schema, request)
            .map_err(|rejection| Box::new(RpcV2CborRequestRejection(rejection)) as ServerError)
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a> {
        Box::pin(async move {
            let request = if input_schema.members().is_empty() {
                empty_request_body(request)
            } else {
                collect_request_body(request, request_body_max_bytes).await?
            };
            let mut deserializer = self.deserialize_request(&request, input_schema)?;
            drive_dynamic_input(input, &mut *deserializer)
        })
    }

    fn serialize_response(
        &self,
        schema: &Schema<'_>,
        output: &dyn aws_smithy_schema::serde::SerializableStruct,
    ) -> http::Response<BoxBody> {
        schema_rpc_v2_cbor::rpc_v2_cbor_serialize_response(schema, output)
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        if let Some(err) = error.as_any().downcast_ref::<RpcV2CborRequestRejection>() {
            return rpc_v2_cbor_request_rejection_response(err);
        }
        modeled_or_bad_request_response(
            error,
            schema_rpc_v2_cbor::rpc_v2_cbor_serialize_error,
            rpc_v2_cbor_request_deserialization_response,
        )
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        Some("application/cbor")
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        Some("application/vnd.amazon.eventstream")
    }

    fn frames_initial_messages(&self) -> bool {
        true
    }
}

fn rpc_path_matches_prefix_policy(request_path: &str, operation: &'static OperationSchema<'static>) -> bool {
    let canonical_suffix = format!("/operation/{}", operation.shape_id().shape_name());
    operation
        .prefix_policy()
        .candidates(request_path)
        .any(|path| path.starts_with("/service/") && path.ends_with(canonical_suffix.as_str()))
}

/// REST operation routing table.
#[derive(Debug, Clone)]
pub struct RestOperationRoutingTable {
    protocol: ShapeId<'static>,
    routes: Vec<(RequestSpec, &'static OperationSchema<'static>)>,
    version: RestVersion,
}

#[derive(Debug, Clone, Copy)]
enum RestVersion {
    RestJson1,
    RestXml,
}

impl RestOperationRoutingTable {
    /// Creates a restJson1 operation routing table from service schema metadata.
    pub fn new_rest_json_1(service_schema: &'static ServiceSchema<'static>) -> Self {
        let protocol = ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1");
        Self::new(protocol.clone(), RestVersion::RestJson1, service_schema)
    }

    /// Creates a restXml operation routing table from service schema metadata.
    pub fn new_rest_xml(service_schema: &'static ServiceSchema<'static>) -> Self {
        let protocol = ShapeId::from_parts("aws.protocols#restXml", "aws.protocols", "restXml");
        Self::new(protocol.clone(), RestVersion::RestXml, service_schema)
    }

    fn new(protocol: ShapeId<'static>, version: RestVersion, service_schema: &'static ServiceSchema<'static>) -> Self {
        let mut routes: Vec<_> = service_schema
            .operations()
            .iter()
            .map(|operation| (rest_request_spec(service_schema, operation), *operation))
            .collect();
        routes.sort_by_key(|(request_spec, _operation)| std::cmp::Reverse(request_spec.rank()));
        Self {
            protocol,
            routes,
            version,
        }
    }

    fn rejection(&self, error: RestError) -> Box<dyn IntoProtocolResponse> {
        match self.version {
            RestVersion::RestJson1 => ProtocolResponse::<_, RestJson1>::boxed(error),
            RestVersion::RestXml => ProtocolResponse::<_, RestXml>::boxed(error),
        }
    }
}

impl ProtocolRouter for RestOperationRoutingTable {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
        let mut method_allowed = true;

        for (request_spec, operation) in &self.routes {
            for path in operation.prefix_policy().candidates(request.uri.path()) {
                let candidate = request.to_bodyless_request_with_path(path);
                match request_spec.matches(&candidate) {
                    Match::Yes => {
                        if self.accept_matches(request.headers, operation) {
                            let route_match = match request_spec.match_and_capture(&candidate) {
                                Ok(Some(matched)) => {
                                    Some(RouteMatchData::new(RestRouteMatch::new(matched.labels().to_vec())))
                                }
                                Ok(None) => None,
                                Err(error) => {
                                    tracing::debug!(%error, "failed to capture REST route labels");
                                    None
                                }
                            };
                            return ProtocolRoutingOutcome::OperationMatched(OperationMatch::new_with_route_match(
                                operation,
                                route_match,
                            ));
                        } else {
                            return ProtocolRoutingOutcome::RejectedNonExclusive(self.not_acceptable());
                        }
                    }
                    Match::MethodNotAllowed => method_allowed = false,
                    Match::No => {}
                }
            }
        }

        if method_allowed {
            ProtocolRoutingOutcome::NoClaim
        } else {
            ProtocolRoutingOutcome::RejectedNonExclusive(self.rejection(RestError::MethodNotAllowed))
        }
    }
}

/// REST server protocol implementation.
#[derive(Debug, Clone)]
pub struct RestServerProtocol {
    protocol: ShapeId<'static>,
    version: RestVersion,
}

impl RestServerProtocol {
    /// Creates a restJson1 server protocol.
    pub fn rest_json_1() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
            version: RestVersion::RestJson1,
        }
    }

    /// Creates a restXml server protocol.
    pub fn rest_xml() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#restXml", "aws.protocols", "restXml"),
            version: RestVersion::RestXml,
        }
    }
}

impl ServerProtocolInner<http::Request<bytes::Bytes>> for RestServerProtocol {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn codec(&self) -> &dyn aws_smithy_schema::codec::DynCodec {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_codec(),
            RestVersion::RestXml => schema_rest_xml::rest_xml_codec(),
        }
    }

    fn deserialize_request<'a>(
        &self,
        request: &'a http::Request<bytes::Bytes>,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>, ServerError> {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_request_deserializer(input_schema, request)
                .map_err(|rejection| Box::new(RestJsonRequestRejection(rejection)) as ServerError),
            RestVersion::RestXml => schema_rest_xml::rest_xml_request_deserializer(input_schema, request)
                .map_err(|rejection| Box::new(RestXmlRequestRejection(rejection)) as ServerError),
        }
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a> {
        Box::pin(async move {
            let request = if rest_input_schema_needs_body(input_schema) {
                collect_request_body(request, request_body_max_bytes).await?
            } else {
                empty_request_body(request)
            };
            let mut deserializer = self.deserialize_request(&request, input_schema)?;
            drive_dynamic_input(input, &mut *deserializer)
        })
    }

    fn serialize_response(
        &self,
        schema: &Schema<'_>,
        output: &dyn aws_smithy_schema::serde::SerializableStruct,
    ) -> http::Response<BoxBody> {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_serialize_response(schema, output),
            RestVersion::RestXml => schema_rest_xml::rest_xml_serialize_response(schema, output),
        }
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        match self.version {
            RestVersion::RestJson1 => {
                if let Some(err) = error.as_any().downcast_ref::<RestJsonRequestRejection>() {
                    return rest_json_request_rejection_response(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_rest_json::rest_json_1_serialize_error,
                    rest_json_request_deserialization_response,
                )
            }
            RestVersion::RestXml => {
                if let Some(err) = error.as_any().downcast_ref::<RestXmlRequestRejection>() {
                    return rest_xml_request_rejection_response(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_rest_xml::rest_xml_serialize_error,
                    rest_xml_request_deserialization_response,
                )
            }
        }
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        match self.version {
            RestVersion::RestJson1 => Some("application/json"),
            RestVersion::RestXml => Some("application/xml"),
        }
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        Some("application/vnd.amazon.eventstream")
    }

    fn frames_initial_messages(&self) -> bool {
        false
    }
}

impl RestOperationRoutingTable {
    fn accept_matches(&self, headers: &HeaderMap, operation: &'static OperationSchema<'static>) -> bool {
        let codec_content_type = match self.version {
            RestVersion::RestJson1 => "application/json",
            RestVersion::RestXml => "application/xml",
        };
        match expected_response_content_type(operation.output(), codec_content_type) {
            Some(content_type) => accept_matches_content_type(headers, content_type.as_ref()),
            None => true,
        }
    }

    fn not_acceptable(&self) -> Box<dyn IntoProtocolResponse> {
        match self.version {
            RestVersion::RestJson1 => ProtocolResponse::<_, RestJson1>::boxed(
                rest_json_runtime_error::RuntimeError::from(rest_json_rejection::RequestRejection::NotAcceptable),
            ),
            RestVersion::RestXml => ProtocolResponse::<_, RestXml>::boxed(rest_xml_runtime_error::RuntimeError::from(
                rest_xml_rejection::RequestRejection::NotAcceptable,
            )),
        }
    }
}

fn accept_matches_content_type(headers: &HeaderMap, content_type: &str) -> bool {
    match content_type.parse::<mime::Mime>() {
        Ok(mime) => accept_header_classifier(headers, &mime),
        // An unparseable modeled @mediaType cannot be validated; accept.
        Err(_) => true,
    }
}

fn expected_response_content_type<'s>(
    output_schema: &'s Schema<'s>,
    codec_content_type: &'static str,
) -> Option<Cow<'s, str>> {
    if let Some(payload) = output_schema.members().iter().find(|m| m.http_payload().is_some()) {
        let media_type = payload.media_type().map(|m| m.value());
        return match (payload.shape_type(), media_type) {
            (ShapeType::Blob, Some(media)) => Some(Cow::Borrowed(media)),
            // A blob payload without `@mediaType` may produce any bytes.
            (ShapeType::Blob, None) => None,
            (ShapeType::String, Some(media)) => Some(Cow::Borrowed(media)),
            (ShapeType::String, None) => Some(Cow::Borrowed("text/plain")),
            _ => Some(Cow::Borrowed(codec_content_type)),
        };
    }
    let has_body_members = output_schema
        .members()
        .iter()
        .any(|m| m.http_header().is_none() && m.http_prefix_headers().is_none() && m.http_response_code().is_none());
    has_body_members.then_some(Cow::Borrowed(codec_content_type))
}

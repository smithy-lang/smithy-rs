/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::ShapeId;
use http::{HeaderMap, Method, Request, Uri};
use std::marker::PhantomData;

use crate::{
    body::BoxBody,
    protocol::{
        aws_json::router::{AwsJsonRouter, Error as AwsJsonError},
        aws_json_10::AwsJson1_0,
        aws_json_11::AwsJson1_1,
        rest::router::Error as RestError,
        rest_json_1::RestJson1,
        rest_xml::RestXml,
        rpc_v2_cbor::{router::RpcV2CborRouter, RpcV2Cbor},
    },
    response::IntoResponse,
    routing::{
        operation_handler_bindings::{rest_request_spec, service_operation_key},
        request_spec::{Match, RequestSpec},
        Router,
    },
    schema::{OperationSchema, ServiceSchema},
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
    protocol: ShapeId<'static>,
    operation: &'static OperationSchema<'static>,
}

impl SelectedProtocolContext {
    /// Creates selected protocol context.
    pub fn new(protocol: ShapeId<'static>, operation: &'static OperationSchema<'static>) -> Self {
        Self { protocol, operation }
    }

    /// Returns the selected protocol shape ID.
    pub fn protocol(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    /// Returns the matched operation schema.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.operation
    }
}

/// A matched operation and its selected protocol context.
#[derive(Debug, Clone)]
pub struct OperationMatch {
    context: SelectedProtocolContext,
}

impl OperationMatch {
    /// Creates a matched operation.
    pub fn new(protocol: ShapeId<'static>, operation: &'static OperationSchema<'static>) -> Self {
        Self {
            context: SelectedProtocolContext::new(protocol, operation),
        }
    }

    /// Returns the selected protocol context for this match.
    pub fn context(&self) -> &SelectedProtocolContext {
        &self.context
    }

    /// Returns the matched operation schema.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.context.operation()
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

/// Erased protocol routing table.
pub trait ProtocolRoutingTable: Send + Sync {
    /// Returns the protocol shape ID handled by this table.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Attempts to route the request metadata to an operation.
    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome;
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
        Self::new(
            ShapeId::from_parts("aws.protocols#awsJson1_0", "aws.protocols", "awsJson1_0"),
            AwsJsonVersion::Json10,
            service_schema,
        )
    }

    /// Creates an AWS JSON 1.1 operation routing table from service schema metadata.
    pub fn new_aws_json_11(service_schema: &'static ServiceSchema<'static>) -> Self {
        Self::new(
            ShapeId::from_parts("aws.protocols#awsJson1_1", "aws.protocols", "awsJson1_1"),
            AwsJsonVersion::Json11,
            service_schema,
        )
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

impl ProtocolRoutingTable for AwsJsonOperationRoutingTable {
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
                ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(self.protocol.clone(), operation))
            }
            Ok(_) => ProtocolRoutingOutcome::NoClaim,
            Err(error) => ProtocolRoutingOutcome::Rejected(self.rejection(error)),
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
        let router = service_schema
            .operations()
            .iter()
            .map(|operation| (service_operation_key(service_schema, operation), *operation))
            .collect();
        Self {
            protocol: ShapeId::from_parts("smithy.protocols#rpcv2Cbor", "smithy.protocols", "rpcv2Cbor"),
            router,
        }
    }
}

impl ProtocolRoutingTable for RpcV2CborOperationRoutingTable {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn route(&self, request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
        if !request.headers.contains_key("smithy-protocol") {
            return ProtocolRoutingOutcome::NoClaim;
        }

        match self.router.match_route(&request.to_bodyless_request()) {
            Ok(operation) if rpc_path_matches_prefix_policy(request.uri.path(), operation) => {
                ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(self.protocol.clone(), operation))
            }
            Ok(_) => ProtocolRoutingOutcome::NoClaim,
            Err(error) => ProtocolRoutingOutcome::Rejected(ProtocolResponse::<_, RpcV2Cbor>::boxed(error)),
        }
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
        Self::new(
            ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
            RestVersion::RestJson1,
            service_schema,
        )
    }

    /// Creates a restXml operation routing table from service schema metadata.
    pub fn new_rest_xml(service_schema: &'static ServiceSchema<'static>) -> Self {
        Self::new(
            ShapeId::from_parts("aws.protocols#restXml", "aws.protocols", "restXml"),
            RestVersion::RestXml,
            service_schema,
        )
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

impl ProtocolRoutingTable for RestOperationRoutingTable {
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
                        return ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(
                            self.protocol.clone(),
                            operation,
                        ));
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

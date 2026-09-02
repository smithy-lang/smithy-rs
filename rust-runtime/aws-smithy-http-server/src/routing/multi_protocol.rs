/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    fmt,
    future::{ready, Future, Ready},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_util::{
    future::{Either, MapOk},
    TryFutureExt,
};
use http::Response;
use http_body::Body as HttpBody;
use tower::{util::Oneshot, Service, ServiceExt};

use crate::{
    body::{boxed, BoxBody},
    error::BoxError,
    routing::{
        operation_handler_bindings::{BuildError, OperationHandlerBinding},
        operation_handler_map::OperationHandlerMap,
        protocol_routing_table::{
            AwsJsonOperationRoutingTable, AwsJsonServerProtocol, ProtocolRouter, ProtocolRoutingOutcome,
            RequestRouteMetadata, RestOperationRoutingTable, RestServerProtocol, RpcV2CborOperationRoutingTable,
            RpcV2CborServerProtocol, SelectedProtocolContext,
        },
    },
    schema::{protocol::SharedServerProtocol, ServiceSchema},
};

/// Ordering constraint for a protocol routing registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolRoutingOrderConstraint {
    /// This protocol should be routed before the referenced protocol, if the referenced protocol is registered.
    Before(&'static str),
    /// This protocol should be routed after the referenced protocol, if the referenced protocol is registered.
    After(&'static str),
}

/// A protocol routing table plus ordering constraints relative to other registered protocols.
pub struct ProtocolRoutingRegistration {
    protocol: SharedServerProtocol,
    router: Box<dyn ProtocolRouter>,
    constraints: Vec<ProtocolRoutingOrderConstraint>,
}

impl ProtocolRoutingRegistration {
    /// Creates a protocol routing registration.
    pub fn new(
        protocol: SharedServerProtocol,
        router: impl ProtocolRouter + 'static,
        constraints: impl Into<Vec<ProtocolRoutingOrderConstraint>>,
    ) -> Self {
        debug_assert_eq!(protocol.protocol_id(), router.protocol_id());
        Self {
            protocol,
            router: Box::new(router),
            constraints: constraints.into(),
        }
    }
}

impl fmt::Debug for ProtocolRoutingRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtocolRoutingRegistration")
            .field("protocol_id", &self.protocol.protocol_id())
            .field("constraints", &self.constraints)
            .finish()
    }
}

struct RegisteredProtocolRoute {
    protocol: SharedServerProtocol,
    router: Box<dyn ProtocolRouter>,
}

/// Factory for a protocol routing registration derived from a service schema.
#[derive(Clone, Copy)]
pub struct ProtocolRoutingFactory {
    build: fn(&'static ServiceSchema<'static>) -> Option<ProtocolRoutingRegistration>,
}

impl ProtocolRoutingFactory {
    /// Creates a protocol routing factory.
    pub const fn new(build: fn(&'static ServiceSchema<'static>) -> Option<ProtocolRoutingRegistration>) -> Self {
        Self { build }
    }

    fn registration(self, service_schema: &'static ServiceSchema<'static>) -> Option<ProtocolRoutingRegistration> {
        (self.build)(service_schema)
    }
}

impl fmt::Debug for ProtocolRoutingFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtocolRoutingFactory").finish_non_exhaustive()
    }
}

/// A routing service that selects a protocol table first, then dispatches through one shared operation handler map.
pub struct MultiProtocolRoutingService<S> {
    protocols: Vec<RegisteredProtocolRoute>,
    handlers: OperationHandlerMap<S>,
}

impl<S> fmt::Debug for MultiProtocolRoutingService<S>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("protocols_len", &self.protocols.len())
            .field("handlers", &self.handlers)
            .finish()
    }
}

impl<S> MultiProtocolRoutingService<S> {
    /// Creates a multi-protocol routing service from protocol objects and operation handlers.
    pub fn new<I>(protocols: Vec<ProtocolRoutingRegistration>, bindings: I) -> Self
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        Self {
            protocols: protocols
                .into_iter()
                .map(|registration| RegisteredProtocolRoute {
                    protocol: registration.protocol,
                    router: registration.router,
                })
                .collect(),
            handlers: OperationHandlerMap::new(bindings),
        }
    }

    /// Creates a multi-protocol routing service from protocol routing registrations and operation handlers.
    pub fn from_protocol_routing_registrations<I>(
        registrations: Vec<ProtocolRoutingRegistration>,
        bindings: I,
    ) -> Result<Self, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        let protocols = sort_protocol_routing_registrations(registrations)?;
        Ok(Self::new(protocols, bindings))
    }

    /// Creates a multi-protocol routing service from built-in protocols, additional protocol routing
    /// factories, and operation handlers.
    pub fn from_operation_handler_bindings<F, I>(
        service_schema: &'static ServiceSchema<'static>,
        additional_protocol_routing_factories: F,
        bindings: I,
    ) -> Result<Self, BuildError>
    where
        F: IntoIterator<Item = ProtocolRoutingFactory>,
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        let registrations = protocol_routing_registrations(service_schema, additional_protocol_routing_factories);
        if registrations.is_empty() {
            return Err(BuildError::MissingProtocol {
                expected: "known server protocol",
            });
        }
        Self::from_protocol_routing_registrations(registrations, bindings)
    }

    /// Maps every operation handler through a closure.
    pub fn map<SNew, F>(self, f: F) -> MultiProtocolRoutingService<SNew>
    where
        F: FnMut(S) -> SNew,
    {
        MultiProtocolRoutingService {
            protocols: self.protocols,
            handlers: self.handlers.map(f),
        }
    }
}

fn protocol_routing_registrations<F>(
    service_schema: &'static ServiceSchema<'static>,
    additional_protocol_routing_factories: F,
) -> Vec<ProtocolRoutingRegistration>
where
    F: IntoIterator<Item = ProtocolRoutingFactory>,
{
    let mut registrations: Vec<_> = builtin_protocol_routing_factories()
        .into_iter()
        .filter_map(|factory| factory.registration(service_schema))
        .collect();
    registrations.extend(
        additional_protocol_routing_factories
            .into_iter()
            .filter_map(|factory| factory.registration(service_schema)),
    );

    registrations
}

fn builtin_protocol_routing_factories() -> [ProtocolRoutingFactory; 5] {
    [
        ProtocolRoutingFactory::new(rpc_v2_cbor_routing_registration),
        ProtocolRoutingFactory::new(aws_json_11_routing_registration),
        ProtocolRoutingFactory::new(aws_json_10_routing_registration),
        ProtocolRoutingFactory::new(rest_json_1_routing_registration),
        ProtocolRoutingFactory::new(rest_xml_routing_registration),
    ]
}

fn has_protocol(service_schema: &'static ServiceSchema<'static>, id: &str) -> bool {
    service_schema
        .protocols()
        .iter()
        .any(|protocol| protocol.as_str() == id)
}

fn rpc_v2_cbor_routing_registration(
    service_schema: &'static ServiceSchema<'static>,
) -> Option<ProtocolRoutingRegistration> {
    has_protocol(service_schema, "smithy.protocols#rpcv2Cbor").then(|| {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(RpcV2CborServerProtocol::new()),
            RpcV2CborOperationRoutingTable::new(service_schema),
            [ProtocolRoutingOrderConstraint::Before("aws.protocols#awsJson1_1")],
        )
    })
}

fn aws_json_11_routing_registration(
    service_schema: &'static ServiceSchema<'static>,
) -> Option<ProtocolRoutingRegistration> {
    has_protocol(service_schema, "aws.protocols#awsJson1_1").then(|| {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(AwsJsonServerProtocol::aws_json_11()),
            AwsJsonOperationRoutingTable::new_aws_json_11(service_schema),
            [ProtocolRoutingOrderConstraint::Before("aws.protocols#awsJson1_0")],
        )
    })
}

fn aws_json_10_routing_registration(
    service_schema: &'static ServiceSchema<'static>,
) -> Option<ProtocolRoutingRegistration> {
    has_protocol(service_schema, "aws.protocols#awsJson1_0").then(|| {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(AwsJsonServerProtocol::aws_json_10()),
            AwsJsonOperationRoutingTable::new_aws_json_10(service_schema),
            [ProtocolRoutingOrderConstraint::Before("aws.protocols#restJson1")],
        )
    })
}

fn rest_json_1_routing_registration(
    service_schema: &'static ServiceSchema<'static>,
) -> Option<ProtocolRoutingRegistration> {
    has_protocol(service_schema, "aws.protocols#restJson1").then(|| {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(RestServerProtocol::rest_json_1()),
            RestOperationRoutingTable::new_rest_json_1(service_schema),
            [ProtocolRoutingOrderConstraint::Before("aws.protocols#restXml")],
        )
    })
}

fn rest_xml_routing_registration(
    service_schema: &'static ServiceSchema<'static>,
) -> Option<ProtocolRoutingRegistration> {
    has_protocol(service_schema, "aws.protocols#restXml").then(|| {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(RestServerProtocol::rest_xml()),
            RestOperationRoutingTable::new_rest_xml(service_schema),
            [],
        )
    })
}

fn sort_protocol_routing_registrations(
    registrations: Vec<ProtocolRoutingRegistration>,
) -> Result<Vec<ProtocolRoutingRegistration>, BuildError> {
    let len = registrations.len();
    let mut protocol_ids = Vec::with_capacity(len);
    for registration in &registrations {
        let protocol_id = registration.protocol.protocol_id().as_str().to_owned();
        if protocol_ids.iter().any(|existing| existing == &protocol_id) {
            return Err(BuildError::DuplicateServerProtocol { protocol: protocol_id });
        }
        protocol_ids.push(protocol_id);
    }

    let mut edges = vec![Vec::<usize>::new(); len];
    let mut indegrees = vec![0usize; len];
    for (source, registration) in registrations.iter().enumerate() {
        for constraint in &registration.constraints {
            let edge = match constraint {
                ProtocolRoutingOrderConstraint::Before(target) => protocol_ids
                    .iter()
                    .position(|id| id == target)
                    .map(|target| (source, target)),
                ProtocolRoutingOrderConstraint::After(target) => protocol_ids
                    .iter()
                    .position(|id| id == target)
                    .map(|target| (target, source)),
            };
            let Some((from, to)) = edge else {
                continue;
            };
            if !edges[from].contains(&to) {
                edges[from].push(to);
                indegrees[to] += 1;
            }
        }
    }

    let mut ordered_indices = Vec::with_capacity(len);
    let mut emitted = vec![false; len];
    while ordered_indices.len() < len {
        let Some(next) = (0..len).find(|&index| !emitted[index] && indegrees[index] == 0) else {
            return Err(BuildError::ProtocolRoutingOrderCycle);
        };
        emitted[next] = true;
        ordered_indices.push(next);
        for &to in &edges[next] {
            indegrees[to] -= 1;
        }
    }

    let mut registrations: Vec<_> = registrations.into_iter().map(Some).collect();
    Ok(ordered_indices
        .into_iter()
        .map(|index| registrations[index].take().expect("ordered index should exist"))
        .collect())
}

type EitherOneshotReady<S, B> = Either<
    MapOk<Oneshot<S, http::Request<B>>, fn(<S as Service<http::Request<B>>>::Response) -> http::Response<BoxBody>>,
    Ready<Result<http::Response<BoxBody>, <S as Service<http::Request<B>>>::Error>>,
>;

pin_project_lite::pin_project! {
    /// Future returned by [`MultiProtocolRoutingService`].
    pub struct MultiProtocolRoutingFuture<S, B> where S: Service<http::Request<B>> {
        #[pin]
        inner: EitherOneshotReady<S, B>
    }
}

impl<S, B> MultiProtocolRoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    fn from_oneshot<RespB>(future: Oneshot<S, http::Request<B>>) -> Self
    where
        S: Service<http::Request<B>, Response = http::Response<RespB>>,
        RespB: HttpBody<Data = Bytes> + Send + 'static,
        RespB::Error: Into<BoxError>,
    {
        Self {
            inner: Either::Left(future.map_ok(|x| x.map(boxed))),
        }
    }

    fn from_response(response: http::Response<BoxBody>) -> Self {
        Self {
            inner: Either::Right(ready(Ok(response))),
        }
    }
}

impl<S, B> Future for MultiProtocolRoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    type Output = Result<http::Response<BoxBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<S, B, RespB> Service<http::Request<B>> for MultiProtocolRoutingService<S>
where
    S: Service<http::Request<B>, Response = http::Response<RespB>> + Clone,
    RespB: HttpBody<Data = Bytes> + Send + 'static,
    RespB::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = MultiProtocolRoutingFuture<S, B>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        let metadata = RequestRouteMetadata::from_request(&req);
        let mut fallback = None;

        for registered in &self.protocols {
            match registered.router.route(metadata) {
                ProtocolRoutingOutcome::NoClaim => {}
                ProtocolRoutingOutcome::OperationMatched(operation_match) => {
                    tracing::debug!(
                        protocol = %registered.protocol.protocol_id(),
                        operation = %operation_match.operation().shape_id(),
                        "matched multi-protocol route",
                    );
                    let Some(handler) = self.handlers.get(operation_match.operation()) else {
                        return MultiProtocolRoutingFuture::from_response(
                            http::Response::builder()
                                .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(crate::body::to_boxed("operation handler missing"))
                                .expect("valid missing operation handler response"),
                        );
                    };
                    req.extensions_mut()
                        .insert::<SelectedProtocolContext>(SelectedProtocolContext::new(
                            registered.protocol.clone(),
                            operation_match.operation(),
                        ));
                    return MultiProtocolRoutingFuture::from_oneshot(handler.oneshot(req));
                }
                ProtocolRoutingOutcome::Rejected(response) => {
                    tracing::debug!(protocol = %registered.protocol.protocol_id(), "terminal multi-protocol routing rejection");
                    return MultiProtocolRoutingFuture::from_response(response.into_response());
                }
                ProtocolRoutingOutcome::RejectedNonExclusive(response) => {
                    tracing::debug!(protocol = %registered.protocol.protocol_id(), "candidate multi-protocol routing rejection");
                    fallback.get_or_insert(response);
                }
            }
        }

        MultiProtocolRoutingFuture::from_response(fallback.map(|response| response.into_response()).unwrap_or_else(
            || {
                http::Response::builder()
                    .status(http::StatusCode::NOT_FOUND)
                    .body(crate::body::empty())
                    .expect("valid multi-protocol not found response")
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        task::{Context, Poll},
    };

    use aws_smithy_schema::{traits::HttpTrait, Schema, ShapeId, ShapeType};
    use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
    use tower::Service;

    use super::*;
    use crate::{
        body::{empty, to_boxed, BoxBody},
        routing::protocol_routing_table::{
            OperationMatch, ProtocolRouter, ProtocolRoutingOutcome, RequestRouteMetadata,
        },
        routing::PrefixPolicy,
        schema::{protocol::SharedServerProtocol, OperationSchema},
    };

    static UNIT: Schema<'static> = Schema::new(
        ShapeId::from_parts("smithy.api#Unit", "smithy.api", "Unit"),
        ShapeType::Structure,
    );
    static OUTPUT_VALUE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Output$value", "test", "Output$value"),
        ShapeType::String,
        "value",
        0,
    );
    static OUTPUT_MEMBERS: &[&Schema<'static>] = &[&OUTPUT_VALUE_MEMBER];
    static OUTPUT: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Output", "test", "Output"),
        ShapeType::Structure,
        OUTPUT_MEMBERS,
    );
    static GET_VALUE_SHAPE: Schema<'static> = Schema::new(
        ShapeId::from_parts("test#GetValue", "test", "GetValue"),
        ShapeType::Operation,
    )
    .with_http(HttpTrait::new("GET", "/value/{id}", None));
    static GET_VALUE_LATEST_SHAPE: Schema<'static> = Schema::new(
        ShapeId::from_parts("test#GetValueLatest", "test", "GetValueLatest"),
        ShapeType::Operation,
    )
    .with_http(HttpTrait::new("GET", "/value/{id}?mode=latest", None));
    static GET_VALUE: OperationSchema<'static> = OperationSchema::new(&GET_VALUE_SHAPE, &UNIT, &OUTPUT, &[]);
    static GET_VALUE_LATEST: OperationSchema<'static> =
        OperationSchema::new(&GET_VALUE_LATEST_SHAPE, &UNIT, &OUTPUT, &[]);
    static PREFIXED_SHAPE: Schema<'static> = Schema::new(
        ShapeId::from_parts("test#Prefixed", "test", "Prefixed"),
        ShapeType::Operation,
    )
    .with_http(HttpTrait::new("GET", "/prefixed", None));
    static PREFIXED_PREFIXES: &[&str] = &["/v1"];
    static PREFIXED: OperationSchema<'static> = OperationSchema::new(&PREFIXED_SHAPE, &UNIT, &UNIT, &[])
        .with_prefix_policy(PrefixPolicy::new(false, PREFIXED_PREFIXES));
    static SERVICE_SHAPE: Schema<'static> = Schema::new(
        ShapeId::from_parts("test#MultiProtocolService", "test", "MultiProtocolService"),
        ShapeType::Service,
    );
    static PROTOCOLS: &[ShapeId<'static>] = &[
        ShapeId::from_parts("smithy.protocols#rpcv2Cbor", "smithy.protocols", "rpcv2Cbor"),
        ShapeId::from_parts("aws.protocols#awsJson1_1", "aws.protocols", "awsJson1_1"),
        ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
    ];
    static OPERATIONS: &[&OperationSchema<'static>] = &[&GET_VALUE, &GET_VALUE_LATEST];
    static SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, PROTOCOLS, OPERATIONS);
    static PREFIXED_OPERATIONS: &[&OperationSchema<'static>] = &[&PREFIXED];
    static PREFIXED_SERVICE: ServiceSchema<'static> =
        ServiceSchema::new(&SERVICE_SHAPE, None, PROTOCOLS, PREFIXED_OPERATIONS);

    #[derive(Clone)]
    struct EchoSelectedProtocol;

    impl<B> Service<Request<B>> for EchoSelectedProtocol {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request<B>) -> Self::Future {
            let selected = req
                .extensions()
                .get::<SelectedProtocolContext>()
                .expect("selected protocol context should be present");
            std::future::ready(Ok(Response::new(to_boxed(format!(
                "{}:{}",
                selected.protocol().as_str(),
                selected.operation().shape_id().shape_name()
            )))))
        }
    }

    fn binding(operation: &'static OperationSchema<'static>) -> OperationHandlerBinding<EchoSelectedProtocol> {
        OperationHandlerBinding::new(operation, EchoSelectedProtocol)
    }

    #[tokio::test]
    async fn aws_json_request_selects_operation_and_context() {
        let mut service = MultiProtocolRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [],
            [binding(&GET_VALUE), binding(&GET_VALUE_LATEST)],
        )
        .unwrap();

        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-amz-target", "MultiProtocolService.GetValue")
            .body(())
            .unwrap();
        let response = service.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(body, "aws.protocols#awsJson1_1:GetValue".as_bytes(),);
    }

    #[tokio::test]
    async fn rpc_v2_cbor_request_selects_operation_and_context() {
        let mut service = MultiProtocolRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [],
            [binding(&GET_VALUE), binding(&GET_VALUE_LATEST)],
        )
        .unwrap();

        let request = Request::builder()
            .method(Method::POST)
            .uri("/service/MultiProtocolService/operation/GetValue")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();
        let response = service.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(body, "smithy.protocols#rpcv2Cbor:GetValue".as_bytes(),);
    }

    #[test]
    fn rest_table_uses_ranked_route_and_query_matching() {
        let table = RestOperationRoutingTable::new_rest_json_1(&SERVICE);
        let request = Request::builder()
            .method(Method::GET)
            .uri("/value/abc?mode=latest")
            .body(())
            .unwrap();

        match table.route(RequestRouteMetadata::from_request(&request)) {
            ProtocolRoutingOutcome::OperationMatched(operation_match) => {
                assert_eq!(operation_match.operation().shape_id().shape_name(), "GetValueLatest");
            }
            _ => panic!("expected REST operation match"),
        }
    }

    #[test]
    fn rest_table_applies_operation_prefix_policy() {
        let table = RestOperationRoutingTable::new_rest_json_1(&PREFIXED_SERVICE);
        let prefixed = Request::builder()
            .method(Method::GET)
            .uri("/v1/prefixed")
            .body(())
            .unwrap();
        let canonical = Request::builder()
            .method(Method::GET)
            .uri("/prefixed")
            .body(())
            .unwrap();

        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&prefixed)),
            ProtocolRoutingOutcome::OperationMatched(_)
        ));
        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&canonical)),
            ProtocolRoutingOutcome::NoClaim
        ));
    }

    #[test]
    fn rest_table_rejects_unacceptable_accept_before_match() {
        let table = RestOperationRoutingTable::new_rest_json_1(&SERVICE);
        let request = Request::builder()
            .method(Method::GET)
            .uri("/value/abc")
            .header("accept", "application/xml")
            .body(())
            .unwrap();

        match table.route(RequestRouteMetadata::from_request(&request)) {
            ProtocolRoutingOutcome::RejectedNonExclusive(response) => {
                assert_eq!(response.into_response().status(), StatusCode::NOT_ACCEPTABLE);
            }
            _ => panic!("expected non-exclusive not acceptable rejection"),
        }
    }

    #[tokio::test]
    async fn rest_accept_rejection_allows_later_protocol_to_match() {
        let protocols = vec![
            ProtocolRoutingRegistration::new(
                SharedServerProtocol::new(RestServerProtocol::rest_xml()),
                RestOperationRoutingTable::new_rest_xml(&SERVICE),
                [],
            ),
            ProtocolRoutingRegistration::new(
                SharedServerProtocol::new(RestServerProtocol::rest_json_1()),
                RestOperationRoutingTable::new_rest_json_1(&SERVICE),
                [],
            ),
        ];
        let mut service = MultiProtocolRoutingService::new(protocols, [binding(&GET_VALUE)]);
        let request = Request::builder()
            .method(Method::GET)
            .uri("/value/abc")
            .header("accept", "application/json")
            .body(())
            .unwrap();

        let response = service.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(body, "aws.protocols#restJson1:GetValue".as_bytes());
    }

    #[test]
    fn aws_json_table_applies_operation_prefix_policy() {
        let table = AwsJsonOperationRoutingTable::new_aws_json_11(&PREFIXED_SERVICE);
        let prefixed = Request::builder()
            .method(Method::POST)
            .uri("/v1")
            .header("x-amz-target", "MultiProtocolService.Prefixed")
            .body(())
            .unwrap();
        let canonical = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-amz-target", "MultiProtocolService.Prefixed")
            .body(())
            .unwrap();

        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&prefixed)),
            ProtocolRoutingOutcome::OperationMatched(_)
        ));
        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&canonical)),
            ProtocolRoutingOutcome::NoClaim
        ));
    }

    #[test]
    fn rpc_v2_cbor_table_applies_operation_prefix_policy() {
        let table = RpcV2CborOperationRoutingTable::new(&PREFIXED_SERVICE);
        let prefixed = Request::builder()
            .method(Method::POST)
            .uri("/v1/service/MultiProtocolService/operation/Prefixed")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();
        let canonical = Request::builder()
            .method(Method::POST)
            .uri("/service/MultiProtocolService/operation/Prefixed")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();

        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&prefixed)),
            ProtocolRoutingOutcome::OperationMatched(_)
        ));
        assert!(matches!(
            table.route(RequestRouteMetadata::from_request(&canonical)),
            ProtocolRoutingOutcome::NoClaim
        ));
    }

    #[derive(Debug)]
    struct FakeTable {
        protocol: ShapeId<'static>,
        outcome: FakeOutcome,
        called: Arc<AtomicBool>,
    }

    #[derive(Debug)]
    struct FakeProtocol {
        protocol: ShapeId<'static>,
    }

    struct FakeRoutingResponse(StatusCode);

    impl crate::routing::IntoProtocolResponse for FakeRoutingResponse {
        fn into_response(self: Box<Self>) -> Response<BoxBody> {
            Response::builder().status(self.0).body(empty()).unwrap()
        }
    }

    #[derive(Debug)]
    enum FakeOutcome {
        NoClaim,
        Match(&'static OperationSchema<'static>),
        Rejected(StatusCode),
        RejectedNonExclusive(StatusCode),
    }

    impl FakeTable {
        fn new(protocol: ShapeId<'static>, outcome: FakeOutcome, called: Arc<AtomicBool>) -> Self {
            Self {
                protocol,
                outcome,
                called,
            }
        }
    }

    impl ProtocolRouter for FakeTable {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol
        }

        fn route(&self, _request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
            self.called.store(true, Ordering::SeqCst);
            match self.outcome {
                FakeOutcome::NoClaim => ProtocolRoutingOutcome::NoClaim,
                FakeOutcome::Match(operation) => {
                    ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(operation))
                }
                FakeOutcome::Rejected(status) => {
                    ProtocolRoutingOutcome::Rejected(Box::new(FakeRoutingResponse(status)))
                }
                FakeOutcome::RejectedNonExclusive(status) => {
                    ProtocolRoutingOutcome::RejectedNonExclusive(Box::new(FakeRoutingResponse(status)))
                }
            }
        }
    }

    impl crate::schema::protocol::ServerProtocolInner for FakeProtocol {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol
        }

        fn codec(&self) -> &dyn aws_smithy_schema::codec::DynCodec {
            panic!("fake protocol does not deserialize requests")
        }

        fn deserialize_request<'a>(
            &self,
            _request: &'a http::Request<bytes::Bytes>,
            _input_schema: &Schema<'_>,
        ) -> Result<Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>, crate::modeled_error::ServerError>
        {
            panic!("fake protocol does not deserialize requests")
        }

        fn serialize_response(
            &self,
            _schema: &Schema<'_>,
            _output: &dyn aws_smithy_schema::serde::SerializableStruct,
        ) -> http::Response<BoxBody> {
            panic!("fake protocol does not serialize responses")
        }

        fn serialize_error(&self, _error: &dyn crate::modeled_error::HttpServerError) -> http::Response<BoxBody> {
            panic!("fake protocol does not serialize errors")
        }
    }

    fn protocol_id(name: &'static str) -> ShapeId<'static> {
        ShapeId::from_parts(name, "test", name.strip_prefix("test#").unwrap_or(name))
    }

    fn registration(
        protocol: ShapeId<'static>,
        constraints: impl Into<Vec<ProtocolRoutingOrderConstraint>>,
    ) -> ProtocolRoutingRegistration {
        registration_with_outcome(
            protocol,
            FakeOutcome::Match(&GET_VALUE),
            Arc::new(AtomicBool::new(false)),
            constraints,
        )
    }

    fn registration_with_outcome(
        protocol: ShapeId<'static>,
        outcome: FakeOutcome,
        called: Arc<AtomicBool>,
        constraints: impl Into<Vec<ProtocolRoutingOrderConstraint>>,
    ) -> ProtocolRoutingRegistration {
        ProtocolRoutingRegistration::new(
            SharedServerProtocol::new(FakeProtocol {
                protocol: protocol.clone(),
            }),
            FakeTable::new(protocol, outcome, called),
            constraints,
        )
    }

    fn sorted_protocol_ids(registrations: Vec<ProtocolRoutingRegistration>) -> Vec<String> {
        sort_protocol_routing_registrations(registrations)
            .unwrap()
            .into_iter()
            .map(|registration| registration.protocol.protocol_id().as_str().to_owned())
            .collect()
    }

    #[test]
    fn protocol_routing_order_constraints_reorder_tables() {
        let sorted = sorted_protocol_ids(vec![
            registration(
                protocol_id("test#second"),
                [ProtocolRoutingOrderConstraint::After("test#first")],
            ),
            registration(protocol_id("test#first"), []),
            registration(
                protocol_id("test#third"),
                [ProtocolRoutingOrderConstraint::Before("test#fourth")],
            ),
            registration(protocol_id("test#fourth"), []),
        ]);

        assert_eq!(sorted, ["test#first", "test#second", "test#third", "test#fourth"]);
    }

    #[test]
    fn protocol_routing_order_constraints_ignore_missing_targets() {
        let sorted = sorted_protocol_ids(vec![
            registration(
                protocol_id("test#first"),
                [ProtocolRoutingOrderConstraint::After("test#missing")],
            ),
            registration(protocol_id("test#second"), []),
        ]);

        assert_eq!(sorted, ["test#first", "test#second"]);
    }

    #[test]
    fn protocol_routing_order_rejects_duplicate_protocol_ids() {
        let error = match sort_protocol_routing_registrations(vec![
            registration(protocol_id("test#first"), []),
            registration(protocol_id("test#first"), []),
        ]) {
            Ok(_) => panic!("expected duplicate protocol ID error"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            BuildError::DuplicateServerProtocol { protocol } if protocol == "test#first"
        ));
    }

    #[test]
    fn protocol_routing_order_rejects_cycles() {
        let error = match sort_protocol_routing_registrations(vec![
            registration(
                protocol_id("test#first"),
                [ProtocolRoutingOrderConstraint::Before("test#second")],
            ),
            registration(
                protocol_id("test#second"),
                [ProtocolRoutingOrderConstraint::Before("test#first")],
            ),
        ]) {
            Ok(_) => panic!("expected protocol routing order cycle error"),
            Err(error) => error,
        };

        assert!(matches!(error, BuildError::ProtocolRoutingOrderCycle));
    }

    #[test]
    fn unconstrained_protocol_routing_order_preserves_input_order() {
        let sorted = sorted_protocol_ids(vec![
            registration(protocol_id("test#third"), []),
            registration(protocol_id("test#first"), []),
            registration(protocol_id("test#second"), []),
        ]);

        assert_eq!(sorted, ["test#third", "test#first", "test#second"]);
    }

    #[test]
    fn builtin_protocol_routing_factories_keep_canonical_order() {
        let sorted = sorted_protocol_ids(protocol_routing_registrations(&SERVICE, []));

        assert_eq!(
            sorted,
            [
                "smithy.protocols#rpcv2Cbor",
                "aws.protocols#awsJson1_1",
                "aws.protocols#restJson1"
            ]
        );
    }

    fn additional_protocol_routing_factory(
        _service_schema: &'static ServiceSchema<'static>,
    ) -> Option<ProtocolRoutingRegistration> {
        Some(registration(
            protocol_id("internal.example#customProtocol"),
            [
                ProtocolRoutingOrderConstraint::Before("aws.protocols#awsJson1_1"),
                ProtocolRoutingOrderConstraint::Before("aws.protocols#restJson1"),
            ],
        ))
    }

    #[test]
    fn additional_protocol_routing_factories_are_added_to_builtins() {
        let sorted = sorted_protocol_ids(protocol_routing_registrations(
            &SERVICE,
            [ProtocolRoutingFactory::new(additional_protocol_routing_factory)],
        ));

        assert_eq!(
            sorted,
            [
                "smithy.protocols#rpcv2Cbor",
                "internal.example#customProtocol",
                "aws.protocols#awsJson1_1",
                "aws.protocols#restJson1"
            ]
        );
    }

    #[tokio::test]
    async fn terminal_rejection_stops_later_protocols() {
        let first_called = Arc::new(AtomicBool::new(false));
        let second_called = Arc::new(AtomicBool::new(false));
        let protocols = vec![
            registration_with_outcome(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::Rejected(StatusCode::BAD_REQUEST),
                first_called.clone(),
                [],
            ),
            registration_with_outcome(
                ShapeId::from_parts("test#second", "test", "second"),
                FakeOutcome::Match(&GET_VALUE),
                second_called.clone(),
                [],
            ),
        ];
        let mut service = MultiProtocolRoutingService::new(protocols, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(first_called.load(Ordering::SeqCst));
        assert!(!second_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn non_exclusive_rejection_allows_later_protocol_match() {
        let protocols = vec![
            registration_with_outcome(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::RejectedNonExclusive(StatusCode::METHOD_NOT_ALLOWED),
                Arc::new(AtomicBool::new(false)),
                [],
            ),
            registration_with_outcome(
                ShapeId::from_parts("internal.example#customProtocol", "internal.example", "customProtocol"),
                FakeOutcome::Match(&GET_VALUE),
                Arc::new(AtomicBool::new(false)),
                [],
            ),
        ];
        let mut service = MultiProtocolRoutingService::new(protocols, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();

        assert_eq!(body, "internal.example#customProtocol:GetValue".as_bytes(),);
    }

    #[tokio::test]
    async fn non_exclusive_rejection_is_used_when_no_later_protocol_matches() {
        let protocols = vec![
            registration_with_outcome(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::RejectedNonExclusive(StatusCode::METHOD_NOT_ALLOWED),
                Arc::new(AtomicBool::new(false)),
                [],
            ),
            registration_with_outcome(
                ShapeId::from_parts("test#second", "test", "second"),
                FakeOutcome::NoClaim,
                Arc::new(AtomicBool::new(false)),
                [],
            ),
        ];
        let mut service = MultiProtocolRoutingService::new(protocols, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn aws_json_unknown_operation_is_terminal() {
        let later_called = Arc::new(AtomicBool::new(false));
        let protocols = vec![
            ProtocolRoutingRegistration::new(
                SharedServerProtocol::new(AwsJsonServerProtocol::aws_json_11()),
                AwsJsonOperationRoutingTable::new_aws_json_11(&SERVICE),
                [],
            ),
            registration_with_outcome(
                ShapeId::from_parts("test#later", "test", "later"),
                FakeOutcome::Match(&GET_VALUE),
                later_called.clone(),
                [],
            ),
        ];
        let mut service = MultiProtocolRoutingService::new(protocols, [binding(&GET_VALUE)]);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-amz-target", "MultiProtocolService.Unknown")
            .body(())
            .unwrap();

        let response = service.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/x-amz-json-1.1"
        );
        assert!(!later_called.load(Ordering::SeqCst));
    }

    #[test]
    fn selected_protocol_context_is_shape_id_based() {
        let context =
            SelectedProtocolContext::new(SharedServerProtocol::new(RestServerProtocol::rest_json_1()), &GET_VALUE);

        assert_eq!(context.protocol().as_str(), "aws.protocols#restJson1");
        assert_eq!(context.operation().shape_id().as_str(), "test#GetValue");
    }

    #[test]
    fn selected_shared_protocol_uses_dynamic_deserialization_rejection() {
        let context =
            SelectedProtocolContext::new(SharedServerProtocol::new(RestServerProtocol::rest_json_1()), &GET_VALUE);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/value/abc")
            .body(bytes::Bytes::from_static(b"{}"))
            .unwrap();

        let error = match context.server_protocol().deserialize_request(&request, &OUTPUT) {
            Ok(_) => panic!("expected restJson1 content-type rejection"),
            Err(error) => error,
        };
        let response = context.server_protocol().serialize_error(&*error);

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            response.headers().get("x-amzn-errortype").unwrap(),
            "UnsupportedMediaTypeException"
        );
    }

    #[test]
    fn aws_json_table_matches_known_operation() {
        let table = AwsJsonOperationRoutingTable::new_aws_json_11(&SERVICE);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-target",
            HeaderValue::from_static("MultiProtocolService.GetValue"),
        );
        let request = Request::builder().method(Method::POST).uri("/").body(()).unwrap();
        let (mut parts, body) = request.into_parts();
        parts.headers = headers;
        let request = Request::from_parts(parts, body);

        match table.route(RequestRouteMetadata::from_request(&request)) {
            ProtocolRoutingOutcome::OperationMatched(operation_match) => {
                assert_eq!(operation_match.operation().shape_id().as_str(), "test#GetValue");
            }
            _ => panic!("expected AWS JSON operation match"),
        }
    }
}

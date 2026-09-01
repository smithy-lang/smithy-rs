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
            AwsJsonOperationRoutingTable, ProtocolRoutingOutcome, ProtocolRoutingTable, RequestRouteMetadata,
            RestOperationRoutingTable, RpcV2CborOperationRoutingTable, SelectedProtocolContext,
        },
    },
    schema::ServiceSchema,
};

/// A routing service that selects a protocol table first, then dispatches through one shared operation handler map.
pub struct MultiProtocolRoutingService<S> {
    tables: Vec<Box<dyn ProtocolRoutingTable>>,
    handlers: OperationHandlerMap<S>,
}

impl<S> fmt::Debug for MultiProtocolRoutingService<S>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("tables_len", &self.tables.len())
            .field("handlers", &self.handlers)
            .finish()
    }
}

impl<S> MultiProtocolRoutingService<S> {
    /// Creates a multi-protocol routing service from protocol routing tables and operation handlers.
    pub fn new<I>(tables: Vec<Box<dyn ProtocolRoutingTable>>, bindings: I) -> Self
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        Self {
            tables,
            handlers: OperationHandlerMap::new(bindings),
        }
    }

    /// Creates a multi-protocol routing service for all built-in protocols present on the service schema.
    pub fn from_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        let tables = builtin_protocol_tables(service_schema);
        if tables.is_empty() {
            return Err(BuildError::MissingProtocol {
                expected: "known server protocol",
            });
        }
        Ok(Self::new(tables, bindings))
    }

    /// Maps every operation handler through a closure.
    pub fn map<SNew, F>(self, f: F) -> MultiProtocolRoutingService<SNew>
    where
        F: FnMut(S) -> SNew,
    {
        MultiProtocolRoutingService {
            tables: self.tables,
            handlers: self.handlers.map(f),
        }
    }
}

fn builtin_protocol_tables(service_schema: &'static ServiceSchema<'static>) -> Vec<Box<dyn ProtocolRoutingTable>> {
    let has_protocol = |id: &str| {
        service_schema
            .protocols()
            .iter()
            .any(|protocol| protocol.as_str() == id)
    };

    let mut tables: Vec<Box<dyn ProtocolRoutingTable>> = Vec::new();
    if has_protocol("smithy.protocols#rpcv2Cbor") {
        tables.push(Box::new(RpcV2CborOperationRoutingTable::new(service_schema)));
    }
    if has_protocol("aws.protocols#awsJson1_1") {
        tables.push(Box::new(AwsJsonOperationRoutingTable::new_aws_json_11(service_schema)));
    }
    if has_protocol("aws.protocols#awsJson1_0") {
        tables.push(Box::new(AwsJsonOperationRoutingTable::new_aws_json_10(service_schema)));
    }
    if has_protocol("aws.protocols#restJson1") {
        tables.push(Box::new(RestOperationRoutingTable::new_rest_json_1(service_schema)));
    }
    if has_protocol("aws.protocols#restXml") {
        tables.push(Box::new(RestOperationRoutingTable::new_rest_xml(service_schema)));
    }

    tables
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

        for table in &self.tables {
            match table.route(metadata) {
                ProtocolRoutingOutcome::NoClaim => {}
                ProtocolRoutingOutcome::OperationMatched(operation_match) => {
                    tracing::debug!(
                        protocol = %operation_match.context().protocol(),
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
                        .insert::<SelectedProtocolContext>(operation_match.context().clone());
                    return MultiProtocolRoutingFuture::from_oneshot(handler.oneshot(req));
                }
                ProtocolRoutingOutcome::Rejected(response) => {
                    tracing::debug!(protocol = %table.protocol_id(), "terminal multi-protocol routing rejection");
                    return MultiProtocolRoutingFuture::from_response(response);
                }
                ProtocolRoutingOutcome::RejectedNonExclusive(response) => {
                    tracing::debug!(protocol = %table.protocol_id(), "candidate multi-protocol routing rejection");
                    fallback.get_or_insert(response);
                }
            }
        }

        MultiProtocolRoutingFuture::from_response(fallback.unwrap_or_else(|| {
            http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .body(crate::body::empty())
                .expect("valid multi-protocol not found response")
        }))
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
            OperationMatch, ProtocolRoutingOutcome, ProtocolRoutingTable, RequestRouteMetadata,
        },
        routing::PrefixPolicy,
        schema::OperationSchema,
    };

    static UNIT: Schema<'static> = Schema::new(
        ShapeId::from_parts("smithy.api#Unit", "smithy.api", "Unit"),
        ShapeType::Structure,
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
    static GET_VALUE: OperationSchema<'static> = OperationSchema::new(&GET_VALUE_SHAPE, &UNIT, &UNIT, &[]);
    static GET_VALUE_LATEST: OperationSchema<'static> =
        OperationSchema::new(&GET_VALUE_LATEST_SHAPE, &UNIT, &UNIT, &[]);
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

    struct FakeTable {
        protocol: ShapeId<'static>,
        outcome: FakeOutcome,
        called: Arc<AtomicBool>,
    }

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

    impl ProtocolRoutingTable for FakeTable {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol
        }

        fn route(&self, _request: RequestRouteMetadata<'_>) -> ProtocolRoutingOutcome {
            self.called.store(true, Ordering::SeqCst);
            match self.outcome {
                FakeOutcome::NoClaim => ProtocolRoutingOutcome::NoClaim,
                FakeOutcome::Match(operation) => {
                    ProtocolRoutingOutcome::OperationMatched(OperationMatch::new(self.protocol.clone(), operation))
                }
                FakeOutcome::Rejected(status) => {
                    ProtocolRoutingOutcome::Rejected(Response::builder().status(status).body(empty()).unwrap())
                }
                FakeOutcome::RejectedNonExclusive(status) => ProtocolRoutingOutcome::RejectedNonExclusive(
                    Response::builder().status(status).body(empty()).unwrap(),
                ),
            }
        }
    }

    #[tokio::test]
    async fn terminal_rejection_stops_later_protocols() {
        let first_called = Arc::new(AtomicBool::new(false));
        let second_called = Arc::new(AtomicBool::new(false));
        let tables: Vec<Box<dyn ProtocolRoutingTable>> = vec![
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::Rejected(StatusCode::BAD_REQUEST),
                first_called.clone(),
            )),
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#second", "test", "second"),
                FakeOutcome::Match(&GET_VALUE),
                second_called.clone(),
            )),
        ];
        let mut service = MultiProtocolRoutingService::new(tables, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(first_called.load(Ordering::SeqCst));
        assert!(!second_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn non_exclusive_rejection_allows_later_protocol_match() {
        let tables: Vec<Box<dyn ProtocolRoutingTable>> = vec![
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::RejectedNonExclusive(StatusCode::METHOD_NOT_ALLOWED),
                Arc::new(AtomicBool::new(false)),
            )),
            Box::new(FakeTable::new(
                ShapeId::from_parts("internal.example#customProtocol", "internal.example", "customProtocol"),
                FakeOutcome::Match(&GET_VALUE),
                Arc::new(AtomicBool::new(false)),
            )),
        ];
        let mut service = MultiProtocolRoutingService::new(tables, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();

        assert_eq!(body, "internal.example#customProtocol:GetValue".as_bytes(),);
    }

    #[tokio::test]
    async fn non_exclusive_rejection_is_used_when_no_later_protocol_matches() {
        let tables: Vec<Box<dyn ProtocolRoutingTable>> = vec![
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#first", "test", "first"),
                FakeOutcome::RejectedNonExclusive(StatusCode::METHOD_NOT_ALLOWED),
                Arc::new(AtomicBool::new(false)),
            )),
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#second", "test", "second"),
                FakeOutcome::NoClaim,
                Arc::new(AtomicBool::new(false)),
            )),
        ];
        let mut service = MultiProtocolRoutingService::new(tables, [binding(&GET_VALUE)]);

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn aws_json_unknown_operation_is_terminal() {
        let later_called = Arc::new(AtomicBool::new(false));
        let tables: Vec<Box<dyn ProtocolRoutingTable>> = vec![
            Box::new(AwsJsonOperationRoutingTable::new_aws_json_11(&SERVICE)),
            Box::new(FakeTable::new(
                ShapeId::from_parts("test#later", "test", "later"),
                FakeOutcome::Match(&GET_VALUE),
                later_called.clone(),
            )),
        ];
        let mut service = MultiProtocolRoutingService::new(tables, [binding(&GET_VALUE)]);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-amz-target", "MultiProtocolService.Unknown")
            .body(())
            .unwrap();

        let response = service.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!later_called.load(Ordering::SeqCst));
    }

    #[test]
    fn selected_protocol_context_is_shape_id_based() {
        let context = SelectedProtocolContext::new(
            ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
            &GET_VALUE,
        );

        assert_eq!(context.protocol().as_str(), "aws.protocols#restJson1");
        assert_eq!(context.operation().shape_id().as_str(), "test#GetValue");
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
                assert_eq!(
                    operation_match.context().protocol().as_str(),
                    "aws.protocols#awsJson1_1"
                );
                assert_eq!(operation_match.operation().shape_id().as_str(), "test#GetValue");
            }
            _ => panic!("expected AWS JSON operation match"),
        }
    }
}

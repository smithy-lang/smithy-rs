/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use super::*;
use crate::error::Error;
use crate::schema::{
    DeserializeError, HttpModeledError, RequestBodyCollectionConfig, ServerProtocol, ServerRequest,
};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, traits::HttpTrait, Schema, ShapeId, ShapeType};
use http::{HeaderMap, HeaderValue, StatusCode};
use http_body::Frame;
use http_body_util::BodyExt;
use std::num::NonZeroUsize;
use std::time::Duration;
use tower::ServiceExt;

static UNIT: Schema<'static> = Schema::new(shape_id!("test", "Unit"), ShapeType::Structure);
static FIRST_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "first"), ShapeType::Operation)
    .with_http(HttpTrait::new("POST", "/first", Some(200)));
static SECOND_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "second"), ShapeType::Operation)
    .with_http(HttpTrait::new("POST", "/second", Some(200)));
static FIRST: OperationSchema<'static> = OperationSchema::new(&FIRST_SHAPE, &UNIT, &UNIT, &[]);
static SECOND: OperationSchema<'static> = OperationSchema::new(&SECOND_SHAPE, &UNIT, &UNIT, &[]);
static SERVICE_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "Service"), ShapeType::Service);
static OPERATIONS: &[&OperationSchema<'static>] = &[&FIRST, &SECOND];
static PROTOCOLS: &[ShapeId<'static>] = &[shape_id!("test", "bodyRouting")];
static SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, PROTOCOLS, OPERATIONS);
static REST_JSON: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    None,
    &[shape_id!("aws.protocols", "restJson1")],
    OPERATIONS,
);
static REST_XML: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    None,
    &[shape_id!("aws.protocols", "restXml")],
    OPERATIONS,
);
static AWS_JSON_10: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    None,
    &[shape_id!("aws.protocols", "awsJson1_0")],
    OPERATIONS,
);
static AWS_JSON_11: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    None,
    &[shape_id!("aws.protocols", "awsJson1_1")],
    OPERATIONS,
);
static RPC: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    None,
    &[shape_id!("smithy.protocols", "rpcv2Cbor")],
    OPERATIONS,
);

/// The first line of this test protocol's request body names the operation.
#[derive(Debug, Default)]
struct BodyProtocol(crate::protocol::rest_json_1::RestJson1Protocol);
#[derive(Debug)]
struct BodyRouter {
    targets: Vec<OperationIndex>,
    config: RequestBodyCollectionConfig,
}
fn rejection(status: StatusCode, message: impl Into<Bytes>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .body(crate::body::from_bytes(message.into()))
        .unwrap()
}
/// Asserts a body-collection rejection and returns its message for failure-kind checks.
async fn rejection_message(response: Response<BoxBody>) -> String {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
impl AsyncProtocolRouter for BodyRouter {
    fn route(self: Arc<Self>, request: Request<Body>) -> ProtocolRouteFuture {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let (bytes, body) = crate::schema::collect_for_routing(body, &self.config)
                .await
                .map_err(|error| rejection(StatusCode::BAD_REQUEST, error.to_string()))?;
            let first_line = bytes.split(|byte| *byte == b'\n').next().unwrap_or_default();
            let name = std::str::from_utf8(first_line)
                .map_err(|_| rejection(StatusCode::BAD_REQUEST, "invalid operation name"))?;
            let selected = self
                .targets
                .iter()
                .find(|target| target.operation().shape_id().shape_name() == name)
                .copied()
                .ok_or_else(|| rejection(StatusCode::NOT_FOUND, "unknown operation"))?;
            Ok((selected, Request::from_parts(parts, body)))
        })
    }
}
impl ServerProtocol for BodyProtocol {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOLS[0]
    }
    fn build_router(
        &self,
        _: &'static ServiceSchema<'static>,
        targets: &[OperationIndex],
        options: &SchemaRoutingOptions,
    ) -> Result<SharedProtocolRouter, RouterBuildError> {
        Ok(SharedProtocolRouter::new_async(BodyRouter {
            targets: targets.to_vec(),
            config: options.request_body.for_routing(),
        }))
    }
    fn serialize_internal_failure(&self) -> Response<BoxBody> {
        rejection(StatusCode::INTERNAL_SERVER_ERROR, "test protocol missing handler")
    }
    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.0.deserialize_request(input, request)
    }
    fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response<BoxBody> {
        self.0.serialize_response(output, value)
    }
    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Response<BoxBody> {
        self.0.serialize_streaming_response(output, value, body)
    }
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response<BoxBody> {
        self.0.serialize_error(error)
    }
    fn serialize_rejection(&self, error: DeserializeError) -> Response<BoxBody> {
        rejection(StatusCode::BAD_REQUEST, error.to_string())
    }
}
fn registration() -> ProtocolRegistration {
    ProtocolRegistration::new(|service| {
        (service.protocols()[0].as_str() == PROTOCOLS[0].as_str())
            .then(|| SharedServerProtocol::new(BodyProtocol::default()))
    })
}
fn binding(operation: &'static OperationSchema<'static>) -> OperationHandlerBinding {
    OperationHandlerBinding::new(
        operation,
        Route::new(tower::service_fn(move |request: Request<Body>| async move {
            let selected = request
                .extensions()
                .get::<SelectedProtocolOperation>()
                .expect("selection before handler");
            assert!(std::ptr::eq(selected.operation(), operation));
            // Echo the frames, which also checks that trailers survive the routing helper.
            Ok::<_, Infallible>(Response::new(crate::body::boxed(request.into_body())))
        })),
    )
}
fn service(options: SchemaRoutingOptions) -> SchemaRoutingService {
    SchemaRoutingService::from_operation_handler_bindings_with_options(
        &SERVICE,
        [registration()],
        [binding(&SECOND), binding(&FIRST)],
        options,
    )
    .unwrap()
}
fn config(bytes: usize, timeout_ms: u64) -> RequestBodyCollectionConfig {
    RequestBodyCollectionConfig {
        max_bytes: NonZeroUsize::new(bytes),
        read_timeout: Some(Duration::from_millis(timeout_ms)),
    }
}
fn request(body: impl Into<Bytes>) -> Request<Body> {
    Request::new(Body::from_bytes(body.into()))
}

#[tokio::test]
async fn body_selects_index_and_preserves_payload_and_trailers() {
    let mut trailers = HeaderMap::new();
    trailers.insert("checksum", HeaderValue::from_static("abc"));
    let frames = vec![
        Ok::<_, Error>(Frame::data(Bytes::from_static(b"fir"))),
        Ok(Frame::data(Bytes::from_static(b"st\n123"))),
        Ok(Frame::trailers(trailers.clone())),
    ];
    let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::iter(frames)));
    let response = service(SchemaRoutingOptions::default())
        .oneshot(Request::new(body))
        .await
        .unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.trailers(), Some(&trailers));
    assert_eq!(collected.to_bytes(), "first\n123");
    let response = service(SchemaRoutingOptions::default())
        .oneshot(request("second\npayload"))
        .await
        .unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "second\npayload"
    );
}

#[tokio::test]
async fn malformed_and_unknown_operation_are_terminal_rejections() {
    for (body, status) in [
        (Bytes::from_static(b"\xff\n"), StatusCode::BAD_REQUEST),
        (Bytes::from_static(b"unknown\n"), StatusCode::NOT_FOUND),
    ] {
        let response = service(SchemaRoutingOptions::default())
            .oneshot(request(body))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn provisional_maximum_caps_collection_and_is_the_only_routing_limit() {
    let mut options = SchemaRoutingOptions::default();
    options.request_body.global = config(8, 1000);
    options
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), config(64, 1000));
    options
        .request_body
        .per_operation
        .insert(SECOND.shape_id().to_string(), config(10, 1000));
    let response = service(options.clone())
        .oneshot(request("first\n1234567890123456"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Bodies under the provisional maximum route through; the selected operation's tighter
    // limit is enforced when its body is collected for deserialization, not at routing.
    let response = service(options.clone())
        .oneshot(request("second\n1234567890123456"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = service(options)
        .oneshot(request(format!("first\n{}", "x".repeat(100))))
        .await
        .unwrap();
    assert!(rejection_message(response).await.contains("exceeded the configured maximum"));
}

#[tokio::test]
async fn explicit_unlimited_operation_dominates_and_absent_override_inherits_global() {
    let mut options = SchemaRoutingOptions::default();
    options.request_body.global = config(8, 1000);
    options
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), RequestBodyCollectionConfig::default());
    // The provisional allowance the router was built with: an explicit unlimited override
    // dominates the global limit on both axes.
    let provisional = options.request_body.for_routing();
    assert!(provisional.max_bytes.is_none());
    assert!(provisional.read_timeout.is_none());
    let app = service(options);
    assert_eq!(
        app.clone()
            .oneshot(request("first\nlong payload"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // Routing collects under the unlimited provisional allowance; the inherited global limit
    // applies when the selected operation collects its body, not here.
    let response = app.oneshot(request("second\nlong payload")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

fn delayed_body(delay: Duration, bytes: &'static [u8]) -> Body {
    Body::new(http_body_util::StreamBody::new(futures_util::stream::once(
        async move {
            tokio::time::sleep(delay).await;
            Ok::<_, Error>(Frame::data(Bytes::from_static(bytes)))
        },
    )))
}
#[tokio::test(start_paused = true)]
async fn collection_enforces_the_provisional_maximum_timeout() {
    let mut options = SchemaRoutingOptions::default();
    options.request_body.global = config(64, 50);
    options
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), config(64, 500));
    assert_eq!(
        options.request_body.for_routing().read_timeout,
        Some(Duration::from_millis(500))
    );
    let app = service(options);
    assert_eq!(
        app.clone()
            .oneshot(Request::new(delayed_body(Duration::from_millis(100), b"first\n")))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // Bodies arriving within the provisional maximum timeout route through even when the
    // selected operation configures a tighter timeout.
    assert_eq!(
        app.clone()
            .oneshot(Request::new(delayed_body(Duration::from_millis(100), b"second\n")))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = app
        .oneshot(Request::new(delayed_body(Duration::from_millis(600), b"first\n")))
        .await
        .unwrap();
    assert!(rejection_message(response).await.contains("timed out"));
}

#[tokio::test]
async fn body_read_failure_is_owned_by_protocol() {
    let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::iter([Err::<
        Frame<Bytes>,
        Error,
    >(
        Error::new("transport failure"),
    )])));
    assert_eq!(
        service(SchemaRoutingOptions::default())
            .oneshot(Request::new(body))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn binding_and_protocol_validation() {
    assert!(matches!(
        SchemaRoutingService::from_operation_handler_bindings(&SERVICE, [registration()], [binding(&FIRST)]),
        Err(RouterBuildError::Binding(_))
    ));
    assert!(matches!(
        SchemaRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [registration()],
            [binding(&FIRST), binding(&FIRST)]
        ),
        Err(RouterBuildError::Binding(_))
    ));
    assert!(matches!(
        SchemaRoutingService::from_operation_handler_bindings(&SERVICE, [], [binding(&FIRST), binding(&SECOND)]),
        Err(RouterBuildError::UnknownProtocol)
    ));
    static NONE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &[], OPERATIONS);
    static MANY: ServiceSchema<'static> = ServiceSchema::new(
        &SERVICE_SHAPE,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("aws.protocols", "restXml"),
        ],
        OPERATIONS,
    );
    for service in [&NONE, &MANY] {
        assert!(matches!(
            SchemaRoutingService::from_operation_handler_bindings(service, [], [binding(&FIRST), binding(&SECOND)]),
            Err(RouterBuildError::ProtocolCount(_))
        ));
    }
    static COPY: OperationSchema<'static> = OperationSchema::new(&FIRST_SHAPE, &UNIT, &UNIT, &[]);
    assert!(matches!(
        SchemaRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [registration()],
            [binding(&COPY), binding(&SECOND)]
        ),
        Err(RouterBuildError::Binding(_))
    ));
}

#[tokio::test]
async fn all_builtins_route_without_polling_body_and_preserve_fallback_errors() {
    for (schema, path, target) in [
        (&REST_JSON, "/first", None),
        (&REST_XML, "/first", None),
        (&AWS_JSON_10, "/", Some("Service.first")),
        (&AWS_JSON_11, "/", Some("Service.first")),
        (&RPC, "/service/Service/operation/first", None),
    ] {
        let bindings = OPERATIONS
            .iter()
            .map(|op| OperationHandlerBinding::new(op, Route::new(crate::operation::SchemaMissingFailure)));
        let app = SchemaRoutingService::from_operation_handler_bindings(schema, [], bindings).unwrap();
        let expected = app.inner.protocol.serialize_internal_failure();
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("smithy-protocol", "rpc-v2-cbor");
        if let Some(target) = target {
            req = req.header("x-amz-target", target);
        }
        let body = http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> { panic!("metadata routing or fallback polled the body") },
        ));
        let actual = app.oneshot(req.body(body).unwrap()).await.unwrap();
        assert_eq!(actual.status(), expected.status());
        assert_eq!(actual.headers(), expected.headers());
        assert_eq!(
            actual.into_body().collect().await.unwrap().to_bytes(),
            expected.into_body().collect().await.unwrap().to_bytes()
        );
    }
}

#[tokio::test]
async fn aws_names_and_rpc_aliases_are_runtime_options() {
    for (schema, path, target, aliases) in [
        (&AWS_JSON_11, "/", Some("Service.FirstSymbol"), false),
        (&RPC, "/service/Service/operation/First", None, true),
    ] {
        let mut options = SchemaRoutingOptions {
            rpc_v2_cbor_add_capitalized_route: aliases,
            ..Default::default()
        };
        options
            .operation_names
            .insert(FIRST.shape_id().to_string(), "FirstSymbol".into());
        let app = SchemaRoutingService::from_operation_handler_bindings_with_options(
            schema,
            [],
            [binding(&SECOND), binding(&FIRST)],
            options,
        )
        .unwrap();
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("smithy-protocol", "rpc-v2-cbor");
        if let Some(target) = target {
            req = req.header("x-amz-target", target);
        }
        assert_eq!(
            app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn layers_see_selection_and_do_not_observe_routing_rejections() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let layer = tower::layer::layer_fn(move |inner: Route<Body>| {
        let counter = counter.clone();
        tower::service_fn(move |request: Request<Body>| {
            assert!(request.extensions().get::<SelectedProtocolOperation>().is_some());
            counter.fetch_add(1, Ordering::SeqCst);
            inner.clone().oneshot(request)
        })
    });
    let app = service(SchemaRoutingOptions::default()).layer(&layer);
    assert_eq!(
        app.clone().oneshot(request("first\n")).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(request("unknown\n")).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelling_body_routing_drops_the_pending_stream() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct PendingBody(Arc<AtomicBool>);
    impl Drop for PendingBody {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl http_body::Body for PendingBody {
        type Data = Bytes;
        type Error = Error;
        fn poll_frame(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
            Poll::Pending
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let mut service = service(SchemaRoutingOptions::default());
    let mut future = Box::pin(service.call(Request::new(PendingBody(dropped.clone()))));
    let waker = futures_util::task::noop_waker();
    assert!(future.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
    assert!(!dropped.load(Ordering::SeqCst));
    drop(future);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn immediate_routing_uses_ready_future_and_rejects_unknown_routes() {
    let targets = [
        OperationIndex {
            index: 0,
            operation: &FIRST,
        },
        OperationIndex {
            index: 1,
            operation: &SECOND,
        },
    ];
    let router = rest_router::<crate::protocol::rest_json_1::RestJson1>(&targets).unwrap();
    assert!(!router.routes_on_body());
    let RouterKind::Metadata(router) = &router.0 else {
        panic!("REST routing selects from metadata");
    };
    let req = Request::builder()
        .method("POST")
        .uri("/first")
        .body(Body::empty())
        .unwrap();
    assert_eq!(router.route(&req).unwrap().index(), 0);
    let req = Request::builder()
        .method("GET")
        .uri("/first")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.route(&req).unwrap_err().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[cfg(debug_assertions)]
#[tokio::test]
#[should_panic(expected = "router index belongs to a different operation")]
async fn inconsistent_operation_identity_is_detected_before_handler_dispatch() {
    #[derive(Debug)]
    struct IncorrectRouter;
    impl ProtocolRouter for IncorrectRouter {
        fn route(&self, _: &Request<Body>) -> Result<OperationIndex, Response<BoxBody>> {
            // This test is in the defining module; external routers cannot construct arbitrary indices.
            Ok(OperationIndex {
                index: 0,
                operation: &FIRST,
            })
        }
    }
    let mut app = service(SchemaRoutingOptions::default()); // Index zero belongs to SECOND.
    app.inner.router = SharedProtocolRouter::new(IncorrectRouter);
    let _ = app.oneshot(request("first\n")).await;
}

#[tokio::test]
async fn owned_non_sync_handlers_preserve_readiness_and_clone_only_when_needed() {
    use std::{
        cell::Cell,
        sync::atomic::{AtomicUsize, Ordering},
    };
    struct Handler {
        clones: Arc<AtomicUsize>,
        ready: Cell<bool>,
    }
    impl Clone for Handler {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::SeqCst);
            Self {
                clones: self.clones.clone(),
                ready: Cell::new(false),
            }
        }
    }
    impl Service<Request<Body>> for Handler {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Infallible>>;
        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            if self.ready.replace(true) {
                Poll::Ready(Ok(()))
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
        fn call(&mut self, _: Request<Body>) -> Self::Future {
            assert!(self.ready.replace(false), "handler must be polled ready before call");
            std::future::ready(Ok(Response::new(crate::body::empty())))
        }
    }
    for schema in [&REST_JSON, &SERVICE] {
        let clones = Arc::new(AtomicUsize::new(0));
        let bindings = OPERATIONS.iter().map(|op| {
            OperationHandlerBinding::new(
                op,
                Route::new(Handler {
                    clones: clones.clone(),
                    ready: Cell::new(false),
                }),
            )
        });
        let mut app =
            SchemaRoutingService::from_operation_handler_bindings(schema, [registration()], bindings).unwrap();
        clones.store(0, Ordering::SeqCst);
        let req = || {
            Request::builder()
                .method("POST")
                .uri("/first")
                .body(Body::from_bytes(Bytes::from_static(b"first\n")))
                .unwrap()
        };
        // Both requests stay in flight on the same service, with Send, non-Sync handlers.
        let first = app.call(req());
        let second = app.call(req());
        let task = tokio::spawn(async move { tokio::join!(first, second) });
        let (first, second) = task.await.unwrap();
        assert_eq!(first.unwrap().status(), StatusCode::OK);
        assert_eq!(second.unwrap().status(), StatusCode::OK);
        let expected = if std::ptr::eq(schema, &REST_JSON) { 2 } else { 6 };
        assert_eq!(clones.load(Ordering::SeqCst), expected);
    }
}

#[test]
fn body_routing_rejects_streaming_inputs_and_outputs_at_construction() {
    static STREAM_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "Input", "data"), ShapeType::Blob, "data", 0).with_streaming();
    static STREAM: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Input"), ShapeType::Structure, &[&STREAM_MEMBER]);
    static INPUT: OperationSchema<'static> = OperationSchema::new(&FIRST_SHAPE, &STREAM, &UNIT, &[]);
    static OUTPUT: OperationSchema<'static> = OperationSchema::new(&FIRST_SHAPE, &UNIT, &STREAM, &[]);
    static BODY_INPUT: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, PROTOCOLS, &[&INPUT]);
    static BODY_OUTPUT: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, PROTOCOLS, &[&OUTPUT]);
    static META_INPUT: ServiceSchema<'static> = ServiceSchema::new(
        &SERVICE_SHAPE,
        None,
        &[shape_id!("aws.protocols", "restJson1")],
        &[&INPUT],
    );
    static META_OUTPUT: ServiceSchema<'static> = ServiceSchema::new(
        &SERVICE_SHAPE,
        None,
        &[shape_id!("aws.protocols", "restJson1")],
        &[&OUTPUT],
    );
    for schema in [&BODY_INPUT, &BODY_OUTPUT] {
        let error = SchemaRoutingService::from_operation_handler_bindings(
            schema,
            [registration()],
            [binding(schema.operations()[0])],
        )
        .unwrap_err();
        assert!(
            matches!(error, RouterBuildError::StreamingBodyRouting { protocol, operation } if protocol == "test#bodyRouting" && operation == "test#first")
        );
    }
    for schema in [&META_INPUT, &META_OUTPUT] {
        assert!(
            SchemaRoutingService::from_operation_handler_bindings(schema, [], [binding(schema.operations()[0])])
                .is_ok()
        );
    }
}

#[tokio::test]
async fn buffered_content_is_reused_and_replacements_and_wrappers_are_read() {
    let original = Bytes::from_static(b"first\npayload");
    let mut app =
        service(SchemaRoutingOptions::default()).layer(&tower::layer::layer_fn(move |_inner: Route<Body>| {
            let original = original.clone();
            tower::service_fn(move |request: Request<Body>| {
                let original = original.clone();
                async move {
                    // Selection has already consumed the read budget. Untouched buffered content
                    // uses its allocation directly, even with a zero timeout at the upgrade.
                    let bytes = crate::schema::collect_request_body(request.into_body(), &config(100, 0))
                        .await
                        .unwrap();
                    assert_eq!(bytes, original);
                    Ok::<_, Infallible>(Response::new(crate::body::from_bytes(bytes)))
                }
            })
        }));
    let response = app.call(request("first\npayload")).await.unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        b"first\npayload"[..]
    );

    let bytes = Bytes::from_static(b"cached content");
    let body = Body::buffered(bytes.clone(), None);
    let result = crate::schema::collect_request_body(body, &config(100, 0))
        .await
        .unwrap();
    assert_eq!(
        result.as_ptr(),
        bytes.as_ptr(),
        "fast path must reuse collected storage"
    );

    let mut body = Body::buffered(bytes, None);
    let _ = body.frame().await;
    assert!(body.buffered_content().is_none(), "polled content cannot be reused");
    let replacement = Body::from_bytes(Bytes::from_static(b"replacement"));
    assert_eq!(
        crate::schema::collect_request_body(replacement, &config(100, 100))
            .await
            .unwrap(),
        b"replacement"[..]
    );
    let wrapped = Body::new(
        Body::buffered(Bytes::from_static(b"old"), None).map_frame(|_| Frame::data(Bytes::from_static(b"wrapped"))),
    );
    assert_eq!(
        crate::schema::collect_request_body(wrapped, &config(100, 100))
            .await
            .unwrap(),
        b"wrapped"[..]
    );
}

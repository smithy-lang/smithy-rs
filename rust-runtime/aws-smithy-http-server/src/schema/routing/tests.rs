/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use super::service::ProtocolRoute;
use super::*;
use crate::body::{Body, BoxBody};
use crate::error::Error;
use crate::response::Response;
use crate::routing::SyncRoute;
use crate::schema::{DeserializeError, HttpModeledError, RequestBodyCollectionConfig, ServerProtocol};
use crate::schema::{
    OperationSchema, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, SelectedProtocolOperation, ServiceSchema,
};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, traits::HttpTrait, Schema, ShapeId, ShapeType};
use bytes::Bytes;
use http::Request;
use http::{HeaderMap, HeaderValue, StatusCode};
use http_body::Frame;
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Service, ServiceExt};

static UNIT: Schema<'static> = Schema::new(shape_id!("test", "Unit"), ShapeType::Structure);
// Codegen records an operation's `@http` binding on its input schema.
static FIRST_INPUT: Schema<'static> = Schema::new(shape_id!("test", "firstInput"), ShapeType::Structure)
    .with_http(HttpTrait::new("POST", "/first", Some(200)));
static SECOND_INPUT: Schema<'static> = Schema::new(shape_id!("test", "secondInput"), ShapeType::Structure)
    .with_http(HttpTrait::new("POST", "/second", Some(200)));
const FIRST_ID: ShapeId<'static> = shape_id!("test", "first");
const SECOND_ID: ShapeId<'static> = shape_id!("test", "second");
const SERVICE_ID: ShapeId<'static> = shape_id!("test", "Service");
static FIRST: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_INPUT, &UNIT, &[]);
static SECOND: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &SECOND_INPUT, &UNIT, &[]);
static OPERATIONS: &[&OperationSchema<'static>] = &[&FIRST, &SECOND];
static PROTOCOLS: &[ShapeId<'static>] = &[shape_id!("test", "bodyRouting")];
static SERVICE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, PROTOCOLS, OPERATIONS);
static REST_JSON: ServiceSchema<'static> =
    ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], OPERATIONS);
static REST_XML: ServiceSchema<'static> =
    ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restXml")], OPERATIONS);
static AWS_JSON_10: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("aws.protocols", "awsJson1_0")],
    OPERATIONS,
);
static AWS_JSON_11: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("aws.protocols", "awsJson1_1")],
    OPERATIONS,
);
static RPC: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("smithy.protocols", "rpcv2Cbor")],
    OPERATIONS,
);

/// The first line of this test protocol's request body names the operation.
#[derive(Debug, Default)]
struct BodyProtocol {
    inner: crate::schema::protocol::RestJson1Protocol,
}
#[derive(Debug)]
struct BodyRouter {
    targets: Vec<OperationTarget>,
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
/// The stub's malformed-request diagnostic; the wire form comes from the protocol's
/// `serialize_routing_error`, which maps `MalformedRequest` to a `400`.
#[derive(Debug)]
struct InvalidName;
impl std::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid operation name")
    }
}
impl std::error::Error for InvalidName {}
impl BodyRouter {
    /// Names the operation the collected body's first line selects, if any.
    fn select(&self, request: &Request<CollectedBody>) -> Result<Option<OperationTarget>, RoutingError> {
        let first_line = request
            .body()
            .bytes()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        let name = std::str::from_utf8(first_line).map_err(|_| RoutingError::malformed(InvalidName))?;
        Ok(self
            .targets
            .iter()
            .find(|target| target.operation().shape_id().shape_name() == name)
            .copied())
    }
}
/// The `x-body-claim` header drives the router's claiming style per request, so one protocol
/// covers every escalation shape. Absent, the router claims on the complete body's first line.
fn claim_mode(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-body-claim").and_then(|value| value.to_str().ok())
}
impl BodyProtocolRouter for BodyRouter {
    fn claim(&self, request: &Request<()>) -> BodyRouteClaim {
        match claim_mode(request.headers()) {
            Some("known-route") => BodyRouteClaim::ClaimedWithRoute(self.targets[0]),
            Some("envelope") => BodyRouteClaim::Claimed,
            _ => BodyRouteClaim::NeedsBodyToClaim(BodyRequirement::Complete),
        }
    }
    fn claim_with_body(&self, request: &Request<CollectedBody>) -> RouteClaim {
        if claim_mode(request.headers()) == Some("deferred-route") {
            return RouteClaim::Claimed;
        }
        if claim_mode(request.headers()) == Some("magic") {
            return if request.body().bytes().starts_with(b"BSF!") {
                RouteClaim::ClaimedWithRoute(self.targets[0])
            } else {
                RouteClaim::NoClaim
            };
        }
        match self.select(request) {
            Ok(Some(selected)) => RouteClaim::ClaimedWithRoute(selected),
            Ok(None) => RouteClaim::NoClaim,
            Err(_) => RouteClaim::Claimed,
        }
    }
    fn route_with_body(&self, request: &Request<CollectedBody>) -> Result<OperationTarget, RoutingError> {
        assert_ne!(claim_mode(request.headers()), Some("known-route"));
        self.select(request)?.ok_or_else(RoutingError::unknown_operation)
    }
}
impl crate::schema::BodyRoutedProtocol for BodyProtocol {
    fn from_build_context(_ctx: &crate::schema::ProtocolBuildContext<'_>) -> Result<Self, RouterBuildError> {
        Ok(BodyProtocol::default())
    }
    fn build_router(
        &self,
        ctx: RouterBuildContext<'_>,
    ) -> Result<impl BodyProtocolRouter + 'static + use<>, RouterBuildError> {
        Ok(BodyRouter {
            targets: ctx.targets.to_vec(),
        })
    }
}
impl ServerProtocol for BodyProtocol {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOLS[0]
    }
    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.inner.deserialize_request(input, request)
    }
    fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response<BoxBody> {
        self.inner.serialize_response(output, value)
    }
    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Response<BoxBody> {
        self.inner.serialize_streaming_response(output, value, body)
    }
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response<BoxBody> {
        self.inner.serialize_error(error)
    }
    fn serialize_rejection(&self, error: DeserializeError) -> Response<BoxBody> {
        rejection(StatusCode::BAD_REQUEST, error.to_string())
    }
    fn serialize_routing_error(&self, err: &RoutingError) -> Response<BoxBody> {
        match err.kind() {
            RoutingErrorKind::MalformedRequest => rejection(StatusCode::BAD_REQUEST, err.to_string()),
            _ => self.inner.serialize_error(err),
        }
    }
}
/// Leaks a one-registration registry; tests parameterize constraints at runtime.
fn registry_of(registration: ProtocolRegistration) -> &'static ProtocolRegistry {
    Box::leak(Box::new(ProtocolRegistry::new(Box::leak(Box::new([registration])))))
}
fn registry() -> &'static ProtocolRegistry {
    static REGISTRY: ProtocolRegistry =
        ProtocolRegistry::new(&[ProtocolRegistration::body_routed::<BodyProtocol>("test#bodyRouting")]);
    &REGISTRY
}
fn binding(operation: &'static OperationSchema<'static>) -> OperationHandlerBinding {
    OperationHandlerBinding::new(
        operation,
        SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
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
fn service(options: RoutingOptions) -> MultiProtocolRoutingService {
    MultiProtocolRoutingService::from_operation_handler_bindings_with_options(
        &SERVICE,
        [registry()],
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
    let response = service(RoutingOptions::default())
        .oneshot(Request::new(body))
        .await
        .unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.trailers(), Some(&trailers));
    assert_eq!(collected.to_bytes(), "first\n123");
    let response = service(RoutingOptions::default())
        .oneshot(request("second\npayload"))
        .await
        .unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "second\npayload"
    );
}

/// The legacy router enums classify onto the three standard kinds; in particular the rpcv2
/// wire-format errors split — a missing or unsupported `smithy-protocol` header means the
/// request never identified the protocol (Coral falls through to its generic 404), while an
/// invalid value on an rpcv2-shaped header is a framing violation.
#[test]
fn legacy_router_errors_classify_onto_the_standard_kinds() {
    use crate::protocol::rpc_v2_cbor::router::{Error as CborError, WireFormatError};
    use crate::protocol::{aws_json::router::Error as JsonError, rest::router::Error as RestError};

    assert_eq!(
        RoutingError::from(RestError::NotFound).kind(),
        RoutingErrorKind::UnknownOperation
    );
    assert_eq!(
        RoutingError::from(RestError::MethodNotAllowed).kind(),
        RoutingErrorKind::MethodNotAllowed
    );

    for err in [JsonError::NotFound, JsonError::NotRootUrl, JsonError::MissingHeader] {
        assert_eq!(RoutingError::from(err).kind(), RoutingErrorKind::UnknownOperation);
    }
    let invalid = http::HeaderValue::from_bytes(b"\xff").unwrap().to_str().unwrap_err();
    assert_eq!(
        RoutingError::from(JsonError::InvalidHeader(invalid)).kind(),
        RoutingErrorKind::MalformedRequest
    );

    assert_eq!(
        RoutingError::from(CborError::NotFound).kind(),
        RoutingErrorKind::UnknownOperation
    );
    assert_eq!(
        RoutingError::from(CborError::ForbiddenHeaders).kind(),
        RoutingErrorKind::MalformedRequest
    );
    let unidentified = RoutingError::from(CborError::InvalidWireFormatHeader(WireFormatError::HeaderNotFound));
    assert_eq!(unidentified.kind(), RoutingErrorKind::UnknownOperation);
    // The diagnostic survives in the source chain even when the kind coarsens it.
    assert!(std::error::Error::source(&unidentified).is_some());
    assert_eq!(
        RoutingError::from(CborError::InvalidWireFormatHeader(
            WireFormatError::WireFormatNotSupported("rpc-v2-json".to_owned())
        ))
        .kind(),
        RoutingErrorKind::UnknownOperation
    );
    assert_eq!(
        RoutingError::from(CborError::InvalidWireFormatHeader(
            WireFormatError::HeaderValueNotValid("not-rpc-v2".to_owned())
        ))
        .kind(),
        RoutingErrorKind::MalformedRequest
    );
}

#[tokio::test]
async fn malformed_and_unknown_operation_are_terminal_rejections() {
    for (body, status) in [
        (Bytes::from_static(b"\xff\n"), StatusCode::BAD_REQUEST),
        (Bytes::from_static(b"unknown\n"), StatusCode::NOT_FOUND),
    ] {
        let response = service(RoutingOptions::default()).oneshot(request(body)).await.unwrap();
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn provisional_maximum_caps_collection_and_is_the_only_routing_limit() {
    let mut options = RoutingOptions::default();
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
    assert!(rejection_message(response)
        .await
        .contains("exceeded the configured maximum"));
}

#[tokio::test]
async fn explicit_unlimited_operation_dominates_and_absent_override_inherits_global() {
    let mut options = RoutingOptions::default();
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
    let mut options = RoutingOptions::default();
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
        service(RoutingOptions::default())
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
        MultiProtocolRoutingService::from_operation_handler_bindings(&SERVICE, [registry()], [binding(&FIRST)]),
        Err(RouterBuildError::Binding(_))
    ));
    assert!(matches!(
        MultiProtocolRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [registry()],
            [binding(&FIRST), binding(&FIRST)]
        ),
        Err(RouterBuildError::Binding(_))
    ));
    assert!(matches!(
        MultiProtocolRoutingService::from_operation_handler_bindings(&SERVICE, [], [binding(&FIRST), binding(&SECOND)]),
        Err(RouterBuildError::MissingProtocols { protocols }) if protocols == ["test#bodyRouting"]
    ));
    static NONE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, &[], OPERATIONS);
    static MANY: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("aws.protocols", "restXml"),
        ],
        OPERATIONS,
    );
    assert!(matches!(
        MultiProtocolRoutingService::from_operation_handler_bindings(&NONE, [], [binding(&FIRST), binding(&SECOND)]),
        Err(RouterBuildError::UnknownProtocol)
    ));
    let many =
        MultiProtocolRoutingService::from_operation_handler_bindings(&MANY, [], [binding(&FIRST), binding(&SECOND)])
            .expect("every declared protocol is served");
    assert_eq!(many.inner.protocols.len(), 2);
    static COPY: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_INPUT, &UNIT, &[]);
    assert!(matches!(
        MultiProtocolRoutingService::from_operation_handler_bindings(
            &SERVICE,
            [registry()],
            [binding(&COPY), binding(&SECOND)]
        ),
        Err(RouterBuildError::Binding(_))
    ));
}

#[test]
fn partially_registered_service_reports_every_missing_protocol_before_building() {
    static PARTIAL: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("test", "unregisteredFirst"),
            shape_id!("test", "unregisteredSecond"),
        ],
        OPERATIONS,
    );
    // Missing registrations fail even before the missing operation bindings are checked.
    let error = MultiProtocolRoutingService::<Body>::from_operation_handler_bindings(&PARTIAL, [], []).unwrap_err();
    assert_eq!(
        error.to_string(),
        "missing protocol registrations: test#unregisteredFirst, test#unregisteredSecond"
    );
    assert!(matches!(
        error,
        RouterBuildError::MissingProtocols { protocols }
            if protocols == ["test#unregisteredFirst", "test#unregisteredSecond"]
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
            .map(|op| OperationHandlerBinding::new(op, SyncRoute::new(crate::operation::SchemaMissingFailure)));
        let app = MultiProtocolRoutingService::from_operation_handler_bindings(schema, [], bindings).unwrap();
        let expected = app.inner.protocols[0]
            .protocol
            .serialize_rejection(DeserializeError::InternalFailure(Error::new(String::from(
                "the operation has not been set",
            ))));
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("smithy-protocol", "rpc-v2-cbor");
        if let Some(target) = target {
            req = req.header("x-amz-target", target);
        }
        let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> { panic!("metadata routing or fallback polled the body") },
        )));
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
async fn rpc_capitalized_alias_is_a_protocol_setting() {
    for (settings, capitalized_status) in [
        (r#"{"capitalizeRoutes":true}"#, StatusCode::OK),
        (r#"{"capitalizeRoutes":false}"#, StatusCode::NOT_FOUND),
        (r#"{}"#, StatusCode::NOT_FOUND),
    ] {
        let options = RoutingOptions {
            protocol_settings: HashMap::from([(
                "smithy.protocols#rpcv2Cbor".to_owned(),
                crate::schema::parse_settings_json(settings.as_bytes()),
            )]),
            ..Default::default()
        };
        let app = MultiProtocolRoutingService::from_operation_handler_bindings_with_options(
            &RPC,
            [],
            [binding(&SECOND), binding(&FIRST)],
            options,
        )
        .unwrap();
        for (path, status) in [
            ("/service/Service/operation/First", capitalized_status),
            ("/service/Service/operation/first", StatusCode::OK),
        ] {
            let req = Request::builder()
                .method("POST")
                .uri(path)
                .header("smithy-protocol", "rpc-v2-cbor")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                app.clone().oneshot(req).await.unwrap().status(),
                status,
                "{settings} {path}"
            );
        }
    }
}

#[test]
fn invalid_protocol_settings_fail_the_build() {
    for settings in [r#"{"capitalizeRoutes":"yes"}"#, r#""not an object""#] {
        let options = RoutingOptions {
            protocol_settings: HashMap::from([(
                "smithy.protocols#rpcv2Cbor".to_owned(),
                crate::schema::parse_settings_json(settings.as_bytes()),
            )]),
            ..Default::default()
        };
        assert!(matches!(
            MultiProtocolRoutingService::from_operation_handler_bindings_with_options(
                &RPC,
                [],
                [binding(&SECOND), binding(&FIRST)],
                options,
            ),
            Err(RouterBuildError::Configuration(_))
        ));
    }
}

#[tokio::test]
async fn layers_see_selection_and_do_not_observe_routing_rejections() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let layer = tower::layer::layer_fn(move |inner: SyncRoute<Body>| {
        let counter = counter.clone();
        tower::service_fn(move |request: Request<Body>| {
            assert!(request.extensions().get::<SelectedProtocolOperation>().is_some());
            counter.fetch_add(1, Ordering::SeqCst);
            inner.clone().oneshot(request)
        })
    });
    let app = service(RoutingOptions::default()).layer(&layer);
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
async fn ready_body_frames_yield_and_wake_before_collection_finishes() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    for limit in [0, 1024] {
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        let frames = futures_util::stream::poll_fn(move |_| {
            let index = observed.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(match index {
                0 => Some(Ok::<_, Error>(Frame::data(Bytes::from_static(b"first\n")))),
                1..=1000 => Some(Ok(Frame::data(Bytes::new()))),
                _ => None,
            })
        });
        let mut options = RoutingOptions::default();
        options.request_body.global = config(limit, 1000);
        let mut app = service(options);
        let mut future = Box::pin(app.call(Request::new(Body::new(http_body_util::StreamBody::new(frames)))));
        let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(wakes.clone());
        assert!(future.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        assert!((1..1000).contains(&reads.load(Ordering::SeqCst)));
        assert!(wakes.0.load(Ordering::SeqCst) > 0, "yield must schedule another poll");
        assert_eq!(future.await.unwrap().status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn claim_with_complete_body_can_defer_to_routing() {
    for (body, status) in [
        ("first\npayload", StatusCode::OK),
        ("unknown\npayload", StatusCode::NOT_FOUND),
        ("", StatusCode::NOT_FOUND),
    ] {
        let response = service(RoutingOptions::default())
            .oneshot(
                Request::builder()
                    .header("x-body-claim", "deferred-route")
                    .body(Body::from_bytes(Bytes::from_static(body.as_bytes())))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
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
    let mut service = service(RoutingOptions::default());
    let mut future = Box::pin(service.call(Request::new(Body::new(PendingBody(dropped.clone())))));
    let waker = futures_util::task::noop_waker();
    assert!(future.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
    assert!(!dropped.load(Ordering::SeqCst));
    drop(future);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn immediate_routing_uses_ready_future_and_rejects_unknown_routes() {
    let targets = [OperationTarget::new(0, &FIRST), OperationTarget::new(1, &SECOND)];
    let router = rest_router(&targets, "application/json").unwrap();
    let shared = SharedProtocolRouter::new(rest_router(&targets, "application/json").unwrap());
    assert!(matches!(shared, SharedProtocolRouter::Metadata(_)));
    let req = Request::builder().method("POST").uri("/first").body(()).unwrap();
    assert_eq!(router.route(&req).unwrap().index(), 0);
    let req = Request::builder().method("GET").uri("/first").body(()).unwrap();
    assert_eq!(router.route(&req).unwrap_err().status_code(), 405);
}

#[test]
fn operation_metadata_classifies_streaming_and_cbor_routes_without_an_indexed_table() {
    static BLOB_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "BlobInput", "data"), ShapeType::Blob, "data", 0).with_streaming();
    static EVENT_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "EventInput", "events"), ShapeType::Union, "events", 0).with_streaming();
    static BLOB: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BlobInput"), ShapeType::Structure, &[&BLOB_MEMBER]);
    static EVENT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "EventInput"), ShapeType::Structure, &[&EVENT_MEMBER]);
    static INPUT_BLOB: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &BLOB, &UNIT, &[]);
    static OUTPUT_BLOB: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &UNIT, &BLOB, &[]);
    static INPUT_EVENT: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &EVENT, &UNIT, &[]);
    static OUTPUT_EVENT: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &UNIT, &EVENT, &[]);
    static BOTH: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &EVENT, &BLOB, &[]);

    let config = RoutingOptions::default();
    for (operation, input, output) in [
        (&FIRST, None, None),
        (&INPUT_BLOB, Some(StreamingKind::Blob), None),
        (&OUTPUT_BLOB, None, Some(StreamingKind::Blob)),
        (&INPUT_EVENT, Some(StreamingKind::EventStream), None),
        (&OUTPUT_EVENT, None, Some(StreamingKind::EventStream)),
        (&BOTH, Some(StreamingKind::EventStream), Some(StreamingKind::Blob)),
    ] {
        // Metadata and CBOR eligibility must not depend on a dense handler-index table.
        let target = OperationTarget::new(usize::MAX, operation);
        assert_eq!(target.index(), usize::MAX);
        assert!(std::ptr::eq(target.operation(), operation));
        assert_eq!(target.input_streaming(), input);
        assert_eq!(target.output_streaming(), output);
        assert_eq!(target.has_streaming_input(), input.is_some());
        assert_eq!(target.has_streaming_output(), output.is_some());
        let blob = input == Some(StreamingKind::Blob) || output == Some(StreamingKind::Blob);
        assert_eq!(target.has_streaming_blob(), blob);

        let router = rpc_v2_cbor_router(&RouterBuildContext {
            service: &REST_JSON,
            targets: &[target],
            config: &config.request_body,
            protocol_settings: None,
        })
        .unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/service/Service/operation/first")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();
        assert_eq!(router.recognizes_streaming_input(&request), input.is_some() && !blob);
        if blob {
            assert_eq!(router.route(&request).unwrap_err().status_code(), 404);
            assert!(matches!(router.claim(&request), RouteClaim::Claimed));
        } else {
            assert_eq!(router.route(&request).unwrap().index(), usize::MAX);
        }
    }
}

#[cfg(debug_assertions)]
#[tokio::test]
#[should_panic(expected = "router index belongs to a different operation")]
async fn inconsistent_operation_identity_is_detected_before_handler_dispatch() {
    #[derive(Debug)]
    struct IncorrectRouter;
    impl MetadataProtocolRouter for IncorrectRouter {
        fn route(&self, _: &Request<()>) -> Result<OperationTarget, RoutingError> {
            // This test is in the defining module; external routers cannot construct arbitrary indices.
            Ok(OperationTarget::new(0, &FIRST))
        }
        fn claim(&self, request: &Request<()>) -> RouteClaim {
            RouteClaim::ClaimedWithRoute(self.route(request).unwrap())
        }
    }
    let mut app = service(RoutingOptions::default()); // Index zero belongs to SECOND.
    app.inner.protocols = Arc::from([ProtocolRoute {
        router: SharedProtocolRouter::new(IncorrectRouter),
        protocol: app.inner.protocols[0].protocol.clone(),
    }]);
    let _ = app.oneshot(request("first\n")).await;
}

#[tokio::test]
async fn shared_handlers_preserve_readiness_and_clone_only_the_selected_route() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Handler {
        clones: Arc<AtomicUsize>,
        ready: AtomicBool,
    }
    impl Clone for Handler {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::SeqCst);
            Self {
                clones: self.clones.clone(),
                ready: AtomicBool::new(false),
            }
        }
    }
    impl Service<Request<Body>> for Handler {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Infallible>>;
        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            if self.ready.swap(true, Ordering::SeqCst) {
                Poll::Ready(Ok(()))
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
        fn call(&mut self, _: Request<Body>) -> Self::Future {
            assert!(
                self.ready.swap(false, Ordering::SeqCst),
                "handler must be polled ready before call"
            );
            std::future::ready(Ok(Response::new(crate::body::empty())))
        }
    }
    for schema in [&REST_JSON, &SERVICE] {
        let clones = Arc::new(AtomicUsize::new(0));
        let bindings = OPERATIONS.iter().map(|op| {
            OperationHandlerBinding::new(
                op,
                SyncRoute::new(Handler {
                    clones: clones.clone(),
                    ready: AtomicBool::new(false),
                }),
            )
        });
        let mut app =
            MultiProtocolRoutingService::from_operation_handler_bindings(schema, [registry()], bindings).unwrap();
        clones.store(0, Ordering::SeqCst);
        let req = || {
            Request::builder()
                .method("POST")
                .uri("/first")
                .body(Body::from_bytes(Bytes::from_static(b"first\n")))
                .unwrap()
        };
        // Both requests stay in flight on the same service, which shares its handlers.
        let first = app.call(req());
        let second = app.call(req());
        let task = tokio::spawn(async move { tokio::join!(first, second) });
        let (first, second) = task.await.unwrap();
        assert_eq!(first.unwrap().status(), StatusCode::OK);
        assert_eq!(second.unwrap().status(), StatusCode::OK);
        // Cloning the service shares the handlers; each request clones only its selected route,
        // including body routing, whose future holds a service clone while it reads the body.
        drop(app.clone());
        assert_eq!(clones.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn body_routing_leaves_streaming_operations_unrouted() {
    static STREAM_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "Input", "data"), ShapeType::Blob, "data", 0).with_streaming();
    static STREAM: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Input"), ShapeType::Structure, &[&STREAM_MEMBER])
            .with_http(HttpTrait::new("POST", "/first", Some(200)));
    static INPUT: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &STREAM, &UNIT, &[]);
    static OUTPUT: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_INPUT, &STREAM, &[]);
    static BODY_INPUT: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, PROTOCOLS, &[&INPUT]);
    static BODY_OUTPUT: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, PROTOCOLS, &[&OUTPUT]);
    static META_INPUT: ServiceSchema<'static> =
        ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], &[&INPUT]);
    static META_OUTPUT: ServiceSchema<'static> =
        ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], &[&OUTPUT]);
    for schema in [&BODY_INPUT, &BODY_OUTPUT] {
        let app = MultiProtocolRoutingService::from_operation_handler_bindings(
            schema,
            [registry()],
            [binding(schema.operations()[0])],
        )
        .expect("a streaming operation does not fail the build");
        let response = app.oneshot(request("first\n")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    for schema in [&META_INPUT, &META_OUTPUT] {
        assert!(MultiProtocolRoutingService::from_operation_handler_bindings(
            schema,
            [],
            [binding(schema.operations()[0])]
        )
        .is_ok());
    }
}

#[tokio::test]
async fn buffered_content_is_reused_and_replacements_and_wrappers_are_read() {
    let original = Bytes::from_static(b"first\npayload");
    let mut app = service(RoutingOptions::default()).layer(&tower::layer::layer_fn(move |_inner: SyncRoute<Body>| {
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

mod multi_protocol {
    use super::*;

    static NAME: Schema<'static> = Schema::new_member(shape_id!("test", "In", "name"), ShapeType::String, "name", 0);
    static IN_MEMBERS: [&Schema<'static>; 1] = [&NAME];
    // Both operations take the same input shape; each input schema carries its operation's `@http`.
    static FIRST_IN: Schema<'static> = Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS)
        .with_original_name("In")
        .with_http(HttpTrait::new("POST", "/first", Some(200)));
    static SECOND_IN: Schema<'static> = Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS)
        .with_original_name("In")
        .with_http(HttpTrait::new("POST", "/second", Some(200)));
    static FIRST_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_IN, &UNIT, &[]);
    static SECOND_OP: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &SECOND_IN, &UNIT, &[]);
    static OPS: &[&OperationSchema<'static>] = &[&FIRST_OP, &SECOND_OP];
    /// Every built-in, declared in the reverse of their priority order.
    static BUILTINS: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restXml"),
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("aws.protocols", "awsJson1_1"),
            shape_id!("aws.protocols", "awsJson1_0"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        OPS,
    );
    static WITH_BODY_ROUTING: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("test", "bodyRouting"),
            shape_id!("aws.protocols", "restJson1"),
        ],
        OPS,
    );

    /// Answers with the selected protocol, the operation and the body the handler received.
    fn echo(operation: &'static OperationSchema<'static>) -> OperationHandlerBinding {
        OperationHandlerBinding::new(
            operation,
            SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
                let selected = request
                    .extensions()
                    .get::<SelectedProtocolOperation>()
                    .expect("selection before handler")
                    .clone();
                assert!(std::ptr::eq(selected.operation(), operation));
                let body = request.into_body().collect().await.unwrap().to_bytes();
                let text = format!(
                    "{} {} {}",
                    selected.protocol().protocol_id().as_str(),
                    operation.shape_id().shape_name(),
                    String::from_utf8_lossy(&body)
                );
                Ok::<_, Infallible>(Response::new(crate::body::from_bytes(text.into())))
            })),
        )
    }

    fn body_routing(order: &'static [ProtocolOrder]) -> &'static ProtocolRegistry {
        registry_of(ProtocolRegistration::body_routed::<BodyProtocol>("test#bodyRouting").with_order(order))
    }

    fn app(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
    ) -> MultiProtocolRoutingService {
        MultiProtocolRoutingService::from_operation_handler_bindings(
            service,
            registries,
            service.operations().iter().map(|operation| echo(operation)),
        )
        .unwrap()
    }

    fn priority(app: &MultiProtocolRoutingService) -> Vec<&'static str> {
        app.inner
            .protocols
            .iter()
            .map(|route| route.protocol.protocol_id().as_str())
            .collect()
    }

    async fn send(
        app: &MultiProtocolRoutingService,
        request: http::request::Builder,
        body: &'static str,
    ) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from_bytes(Bytes::from_static(body.as_bytes())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        // Rejection bodies may be CBOR; the assertions only match ASCII fragments.
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn post(uri: &str) -> http::request::Builder {
        Request::builder().method("POST").uri(uri)
    }

    #[test]
    fn builtins_follow_their_chained_constraints_whatever_the_declaration_order() {
        assert_eq!(
            priority(&app(&BUILTINS, [])),
            [
                "smithy.protocols#rpcv2Cbor",
                "aws.protocols#awsJson1_0",
                "aws.protocols#awsJson1_1",
                "aws.protocols#restJson1",
                "aws.protocols#restXml",
            ]
        );
    }

    #[tokio::test]
    async fn each_builtin_claims_by_its_identifying_characteristics() {
        let app = app(&BUILTINS, []);
        let cases = [
            (
                post("/service/Service/operation/first").header("smithy-protocol", "rpc-v2-cbor"),
                "smithy.protocols#rpcv2Cbor first ",
            ),
            (
                post("/")
                    .header("content-type", "application/x-amz-json-1.0")
                    .header("x-amz-target", "Service.first"),
                "aws.protocols#awsJson1_0 first ",
            ),
            (
                post("/")
                    .header("content-type", "application/x-amz-json-1.1")
                    .header("x-amz-target", "Service.second"),
                "aws.protocols#awsJson1_1 second ",
            ),
            // awsJson identifies on the path; a query string some clients add is ignored.
            (
                post("/?times=2")
                    .header("content-type", "application/x-amz-json-1.0")
                    .header("x-amz-target", "Service.first"),
                "aws.protocols#awsJson1_0 first ",
            ),
            (
                post("/first").header("content-type", "application/json; charset=utf-8"),
                "aws.protocols#restJson1 first ",
            ),
            (
                post("/second").header("content-type", "application/xml"),
                "aws.protocols#restXml second ",
            ),
            // Without `Content-Type` or a body, REST protocols tie and priority decides.
            (post("/first"), "aws.protocols#restJson1 first "),
        ];
        for (request, expected) in cases {
            assert_eq!(send(&app, request, "").await, (StatusCode::OK, expected.to_owned()));
        }
    }

    #[tokio::test]
    async fn requests_no_protocol_identifies_get_corals_unknown_operation_response() {
        let app = app(&BUILTINS, []);
        let unclaimed = [
            // awsJson requires a known target to claim the request.
            post("/")
                .header("content-type", "application/x-amz-json-1.0")
                .header("x-amz-target", "Service.unknown"),
            // A REST route whose `Content-Type` no REST protocol derives.
            post("/first").header("content-type", "text/plain"),
            // awsJson without its media type.
            post("/").header("x-amz-target", "Service.first"),
            Request::builder().method("GET").uri("/nowhere"),
        ];
        for request in unclaimed {
            let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            // Coral sends this response without a `Content-Type`.
            assert!(response.headers().is_empty());
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "<UnknownOperationException/>\n"
            );
        }
    }

    /// A routing rejection goes out as the rejecting protocol's modeled error: the same
    /// discriminator framing as any handler-returned error, status and shape from the
    /// rejection's schema.
    #[tokio::test]
    async fn routing_rejections_are_protocol_framed_modeled_errors() {
        // restJson1 alone matched `/first` by URI but not by method: MethodNotAllowedException,
        // 405. (Among several protocols a method mismatch is `NoClaim`, not a rejection.)
        static REST_ONLY: ServiceSchema<'static> =
            ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], OPS);
        let response = app(&REST_ONLY, [])
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/first")
                    .header("content-type", "application/json")
                    .body(Body::from_bytes(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response.headers().get("x-amzn-errortype").unwrap(),
            "MethodNotAllowedException"
        );
        assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "{}");
        // awsJson1_0 alone routes an unknown target to UnknownOperationException, 404, with the
        // discriminator in its JSON body. (Among several protocols an unknown target is
        // `NoClaim`, answered by the service-level unclaimed response instead.)
        static AWS_JSON_ONLY: ServiceSchema<'static> =
            ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "awsJson1_0")], OPS);
        let (status, body) = send(
            &app(&AWS_JSON_ONLY, []),
            post("/")
                .header("content-type", "application/x-amz-json-1.0")
                .header("x-amz-target", "Service.bogus"),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.starts_with('{') && body.contains("UnknownOperationException"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn rpc_v2_cbor_claims_unknown_routes_without_falling_through_or_reading_body() {
        let app = app(&BUILTINS, []);
        for path in ["/service/Service/operation/unknown", "/first"] {
            // `/first` could be handled by REST JSON, but CBOR has already claimed it.
            let response = app
                .clone()
                .oneshot(
                    post(path)
                        .header("smithy-protocol", "rpc-v2-cbor")
                        .header("content-type", "application/json")
                        .body(untouchable_body())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(response.headers()[http::header::CONTENT_TYPE], "application/cbor");
        }
    }

    #[tokio::test]
    async fn rpc_v2_cbor_rejects_a_claimed_request_it_cannot_serve() {
        let app = app(&BUILTINS, []);
        // `x-amz-target` is forbidden on rpcv2Cbor, which claimed the request first. The
        // response byte-matches Coral's: `400`, no `Content-Type`, `Connection: close`, and
        // the bare 33-byte body.
        let response = app
            .clone()
            .oneshot(
                post("/service/Service/operation/first")
                    .header("smithy-protocol", "rpc-v2-cbor")
                    .header("x-amz-target", "Service.first")
                    .header("content-type", "application/x-amz-json-1.0")
                    .body(Body::from_bytes(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key(http::header::CONTENT_TYPE));
        assert_eq!(
            response.headers().get(http::header::CONNECTION).map(|v| v.as_bytes()),
            Some(b"close".as_slice())
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, "<MalformedHttpRequestException/>\n");
    }

    #[tokio::test]
    async fn rpc_v2_cbor_rejects_streaming_blobs_while_rest_serves_them() {
        static DATA: Schema<'static> =
            Schema::new_member(shape_id!("test", "Upload", "data"), ShapeType::Blob, "data", 0)
                .with_streaming()
                .with_http_payload();
        static UPLOAD_MEMBERS: [&Schema<'static>; 1] = [&DATA];
        static UPLOAD: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Upload"), ShapeType::Structure, &UPLOAD_MEMBERS)
                .with_original_name("Upload")
                .with_http(HttpTrait::new("POST", "/first", Some(200)));
        static STREAMING: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &UPLOAD, &UNIT, &[]);
        static BOTH: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                shape_id!("aws.protocols", "restJson1"),
            ],
            &[&STREAMING],
        );
        static CBOR_ONLY: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("smithy.protocols", "rpcv2Cbor")],
            &[&STREAMING],
        );
        let rpc = || post("/service/Service/operation/first").header("smithy-protocol", "rpc-v2-cbor");
        for service in [&BOTH, &CBOR_ONLY] {
            let (status, _) = send(&app(service, []), rpc(), "").await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{:?}", service.protocols());
        }
        let (status, text) = send(&app(&BOTH, []), post("/first"), "").await;
        assert_eq!(
            (status, text.as_str()),
            (StatusCode::OK, "aws.protocols#restJson1 first ")
        );
    }

    #[test]
    fn unordered_served_protocols_fail_the_build() {
        let error = MultiProtocolRoutingService::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(&[])],
            OPS.iter().map(|operation| echo(operation)),
        )
        .unwrap_err();
        assert!(
            matches!(error, RouterBuildError::AmbiguousProtocolOrder { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_duplicate_protocol_fails_the_build() {
        let error = MultiProtocolRoutingService::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(&[]), body_routing(&[])],
            OPS.iter().map(|operation| echo(operation)),
        )
        .unwrap_err();
        assert!(matches!(error, RouterBuildError::DuplicateProtocol { protocol } if protocol == "test#bodyRouting"),);
    }

    #[test]
    fn a_constraint_against_an_unregistered_protocol_fails_the_build() {
        static TYPO: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson2")];
        let error = MultiProtocolRoutingService::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(TYPO)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .unwrap_err();
        assert!(matches!(error, RouterBuildError::Configuration(_)), "{error}");
    }

    #[tokio::test]
    async fn body_first_protocols_can_be_ordered_after_metadata_protocols() {
        static AFTER: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(AFTER)]);
        assert_eq!(priority(&app), ["aws.protocols#restJson1", "test#bodyRouting"]);
        // restJson1 claims from the head, so the body router never reads the body.
        assert_eq!(
            send(&app, post("/first").header("content-type", "application/json"), "{}").await,
            (StatusCode::OK, "aws.protocols#restJson1 first {}".to_owned())
        );
        // restJson1 passes; the body router claims and the handler replays the collected bytes.
        assert_eq!(
            send(&app, post("/elsewhere"), "second\npayload").await,
            (StatusCode::OK, "test#bodyRouting second second\npayload".to_owned())
        );
        let (status, _) = send(&app, post("/elsewhere"), "unknown\n").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn protocols_after_a_passing_body_router_see_the_collected_body() {
        static FIRST: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(FIRST)]);
        assert_eq!(priority(&app), ["test#bodyRouting", "aws.protocols#restJson1"]);
        assert_eq!(
            send(
                &app,
                post("/first").header("content-type", "application/json"),
                "{\"name\":\"n\"}"
            )
            .await,
            (
                StatusCode::OK,
                "aws.protocols#restJson1 first {\"name\":\"n\"}".to_owned()
            )
        );
        assert_eq!(
            send(
                &app,
                post("/first").header("content-type", "application/json"),
                "second\n"
            )
            .await,
            (StatusCode::OK, "test#bodyRouting second second\n".to_owned())
        );
    }

    /// A body that panics if routing polls it, proving claims resolved from the head alone.
    fn untouchable_body() -> Body {
        Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> { panic!("routing polled an event-stream body") },
        )))
    }

    static STREAM_MEMBER: Schema<'static> =
        Schema::new_member(shape_id!("test", "Streaming", "events"), ShapeType::Union, "events", 0)
            .with_streaming()
            .with_http_payload();
    static STREAM_INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Streaming"), ShapeType::Structure, &[&STREAM_MEMBER])
            .with_http(HttpTrait::new("POST", "/stream", Some(200)));
    static STREAM_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &STREAM_INPUT, &UNIT, &[]);
    static OUTPUT_OP: OperationSchema<'static> =
        OperationSchema::new(shape_id!("test", "output"), &SECOND_IN, &STREAM_INPUT, &[]);
    static STREAM_SERVICE: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("test", "bodyRouting"),
            shape_id!("aws.protocols", "awsJson1_1"),
        ],
        &[&STREAM_OP, &SECOND_OP, &OUTPUT_OP],
    );

    fn streaming_app() -> MultiProtocolRoutingService {
        static BEFORE: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#awsJson1_1")];
        MultiProtocolRoutingService::from_operation_handler_bindings(
            &STREAM_SERVICE,
            [body_routing(BEFORE)],
            STREAM_SERVICE.operations().iter().map(|operation| {
                OperationHandlerBinding::new(
                    operation,
                    SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
                        let selected = request.extensions().get::<SelectedProtocolOperation>().unwrap().clone();
                        let bytes = if operation.input().members().iter().any(|member| member.streaming()) {
                            Bytes::new()
                        } else {
                            request.into_body().collect().await.unwrap().to_bytes()
                        };
                        Ok::<_, Infallible>(Response::new(crate::body::from_bytes(
                            format!(
                                "{} {} {}",
                                selected.protocol().protocol_id().as_str(),
                                operation.shape_id().shape_name(),
                                String::from_utf8_lossy(&bytes)
                            )
                            .into(),
                        )))
                    })),
                )
            }),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn aws_json_streaming_input_defers_body_claimants_without_polling() {
        let app = streaming_app();
        assert_eq!(app.inner.streaming_recognizers.as_ref(), &[1]);
        let response = app
            .oneshot(
                post("/")
                    .header("content-type", "application/x-amz-json-1.1; charset=UTF-8")
                    .header("x-amz-target", "Service.first")
                    .body(untouchable_body())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "aws.protocols#awsJson1_1 first "
        );
    }

    #[tokio::test]
    async fn media_type_alone_does_not_defer_body_claimants() {
        static BEFORE: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(BEFORE)]);
        assert!(app.inner.streaming_recognizers.is_empty());
        assert_eq!(
            send(
                &app,
                post("/first").header("content-type", "application/vnd.amazon.eventstream"),
                "second\npayload"
            )
            .await
            .1,
            "test#bodyRouting second second\npayload"
        );
    }

    #[tokio::test]
    async fn ordinary_and_output_only_streaming_requests_keep_body_precedence() {
        let app = streaming_app();
        for target in ["Service.second", "Service.output", "Service.unknown"] {
            assert_eq!(
                send(
                    &app,
                    post("/")
                        .header("content-type", "application/x-amz-json-1.1")
                        .header("x-amz-target", target),
                    "second\npayload"
                )
                .await,
                (StatusCode::OK, "test#bodyRouting second second\npayload".into())
            );
        }
        // The target alone does not identify awsJson; method, path and content type matter.
        for builder in [
            Request::builder().method("GET").uri("/"),
            post("/other"),
            post("/").header("content-type", "application/json"),
        ] {
            assert_eq!(
                send(&app, builder.header("x-amz-target", "Service.first"), "second\npayload")
                    .await
                    .1,
                "test#bodyRouting second second\npayload"
            );
        }
    }

    #[test]
    fn built_in_recognizers_use_input_routes_and_protocol_identification() {
        static ALL: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
                shape_id!("aws.protocols", "awsJson1_0"),
                shape_id!("aws.protocols", "awsJson1_1"),
                shape_id!("smithy.protocols", "rpcv2Cbor"),
            ],
            &[&STREAM_OP, &SECOND_OP],
        );
        let all_app = app(&ALL, []);
        assert_eq!(all_app.inner.streaming_recognizers.len(), 5);
        for route in all_app.inner.protocols.iter() {
            let SharedProtocolRouter::Metadata(router) = &route.router else {
                unreachable!()
            };
            let builder = match route.protocol.protocol_id().as_str() {
                "aws.protocols#restJson1" | "aws.protocols#restXml" => {
                    post("/stream").header("content-type", "application/vnd.amazon.eventstream")
                }
                "aws.protocols#awsJson1_0" => post("/")
                    .header("content-type", "application/x-amz-json-1.0")
                    .header("x-amz-target", "Service.first"),
                "aws.protocols#awsJson1_1" => post("/")
                    .header("content-type", "application/x-amz-json-1.1")
                    .header("x-amz-target", "Service.first"),
                "smithy.protocols#rpcv2Cbor" => {
                    post("/service/Service/operation/first").header("smithy-protocol", "rpc-v2-cbor")
                }
                _ => unreachable!(),
            };
            assert!(router.recognizes_streaming_input(&builder.body(()).unwrap()));
            assert!(!router.recognizes_streaming_input(&post("/second").body(()).unwrap()));
        }
        static OUTPUT_ONLY: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "awsJson1_1")],
            &[&OUTPUT_OP],
        );
        let output = app(&OUTPUT_ONLY, []);
        assert!(output.inner.streaming_recognizers.is_empty());
    }

    #[derive(Debug)]
    struct AdvisoryRouter {
        checks: Arc<std::sync::atomic::AtomicUsize>,
        claims: Arc<std::sync::atomic::AtomicUsize>,
        reject: bool,
        streaming: bool,
    }
    impl MetadataProtocolRouter for AdvisoryRouter {
        fn route(&self, _: &Request<()>) -> Result<OperationTarget, RoutingError> {
            Err(RoutingError::unknown_operation())
        }
        fn recognizes_streaming_input(&self, _: &Request<()>) -> bool {
            self.checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.streaming
        }
        fn claim(&self, _: &Request<()>) -> RouteClaim {
            self.claims.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.reject {
                RouteClaim::Claimed
            } else {
                RouteClaim::NoClaim
            }
        }
    }
    #[derive(Debug)]
    struct TrackedBodyRouter {
        router: BodyRouter,
        calls: Arc<std::sync::Mutex<Vec<usize>>>,
        index: usize,
    }
    impl BodyProtocolRouter for TrackedBodyRouter {
        fn claim(&self, request: &Request<()>) -> BodyRouteClaim {
            self.calls.lock().unwrap().push(self.index);
            self.router.claim(request)
        }
        fn claim_with_body(&self, request: &Request<CollectedBody>) -> RouteClaim {
            self.router.claim_with_body(request)
        }
        fn route_with_body(&self, request: &Request<CollectedBody>) -> Result<OperationTarget, RoutingError> {
            self.router.route_with_body(request)
        }
    }
    fn advisory_app(
        reject: bool,
        streaming: bool,
    ) -> (
        MultiProtocolRoutingService,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::Mutex<Vec<usize>>>,
    ) {
        let mut app = streaming_app();
        let checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let claims = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut routes = app.inner.protocols.to_vec();
        routes[1].router = SharedProtocolRouter::new(AdvisoryRouter {
            checks: checks.clone(),
            claims: claims.clone(),
            reject,
            streaming,
        });
        let mut second_body = routes[0].clone();
        for (index, route) in [(0, &mut routes[0]), (2, &mut second_body)] {
            route.router = SharedProtocolRouter::new_body_routed(TrackedBodyRouter {
                router: BodyRouter {
                    targets: if index == 0 {
                        vec![]
                    } else {
                        vec![OperationTarget::new(1, &SECOND_OP)]
                    },
                },
                calls: calls.clone(),
                index,
            });
        }
        routes.push(second_body);
        app.inner.protocols = routes.into();
        app.inner.body_routers = vec![0, 2].into();
        (app, checks, claims, calls)
    }

    #[tokio::test]
    async fn normal_and_deferred_walks_preserve_order_replay_and_cached_recognition_across_suspension() {
        for streaming in [true, false] {
            let (app, checks, claims, calls) = advisory_app(false, streaming);
            let mut pending_once = true;
            let mut frames = vec![b"ond\npayload".as_slice(), b"sec".as_slice()];
            let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
                move |cx| {
                    if pending_once {
                        pending_once = false;
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    Poll::Ready(
                        frames
                            .pop()
                            .map(|bytes| Ok::<_, Error>(Frame::data(Bytes::copy_from_slice(bytes)))),
                    )
                },
            )));
            let response = app.oneshot(post("/").body(body).unwrap()).await.unwrap();
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "test#bodyRouting second second\npayload"
            );
            assert_eq!(*calls.lock().unwrap(), vec![0, 2]);
            assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn metadata_rejection_bypasses_deferred_body_fallback() {
        let (app, checks, claims, calls) = advisory_app(true, true);
        let response = app.oneshot(post("/").body(untouchable_body()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn claimed_with_route_dispatches_without_matching_the_body() {
        static FIRST: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(FIRST)]);
        assert_eq!(
            send(
                &app,
                post("/anywhere").header("x-body-claim", "known-route"),
                "not an operation"
            )
            .await,
            (StatusCode::OK, "test#bodyRouting first not an operation".to_owned())
        );
    }

    #[tokio::test]
    async fn claimed_collects_the_complete_body_before_routing() {
        static FIRST: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(FIRST)]);
        let frames = vec![
            Ok::<_, Error>(Frame::data(Bytes::from_static(b"se"))),
            Ok(Frame::data(Bytes::from_static(b"cond\npayload"))),
        ];
        let response = app
            .oneshot(
                post("/anywhere")
                    .header("x-body-claim", "envelope")
                    .body(Body::new(http_body_util::StreamBody::new(futures_util::stream::iter(
                        frames,
                    ))))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "test#bodyRouting second second\npayload"
        );
    }

    /// Body inspection receives the complete body. Whether the protocol claims or declines,
    /// the selected handler receives the original bytes.
    #[tokio::test]
    async fn complete_body_claims_on_magic_and_replays_on_mismatch() {
        static FIRST: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(FIRST)]);
        let sniff = |uri: &str| post(uri).header("x-body-claim", "magic");
        // Match: the body begins with the protocol marker.
        assert_eq!(
            send(&app, sniff("/anywhere"), "BSF!datagram-bytes").await,
            (StatusCode::OK, "test#bodyRouting first BSF!datagram-bytes".to_owned())
        );
        // Mismatch across multiple frames: the next protocol receives the complete body.
        let frames = vec![
            Ok::<_, Error>(Frame::data(Bytes::from_static(b"{\"na"))),
            Ok(Frame::data(Bytes::from_static(b"me\":\"n\"}"))),
        ];
        let response = app
            .clone()
            .oneshot(
                sniff("/first")
                    .header("content-type", "application/json")
                    .body(Body::new(http_body_util::StreamBody::new(futures_util::stream::iter(
                        frames,
                    ))))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "aws.protocols#restJson1 first {\"name\":\"n\"}"
        );
        // A body shorter than the marker still replays to the next claimant.
        assert_eq!(
            send(&app, sniff("/first").header("content-type", "application/json"), "B").await,
            (StatusCode::OK, "aws.protocols#restJson1 first B".to_owned())
        );
    }

    /// After `Claimed` the claim is settled: an operation the body does not name is
    /// this protocol's rejection, never a fall-through to later protocols or the unclaimed
    /// response — exactly Coral's behavior once its RPC handler claims a JSON request.
    #[tokio::test]
    async fn claimed_routing_errors_never_fall_through() {
        static FIRST: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(FIRST)]);
        assert_eq!(
            send(
                &app,
                post("/anywhere").header("x-body-claim", "envelope"),
                "second\npayload"
            )
            .await,
            (StatusCode::OK, "test#bodyRouting second second\npayload".to_owned())
        );
        // `/first` names a restJson1 route and the body a bindable name for it — but the
        // envelope claim settled first, so the unknown envelope operation is terminal.
        let response = app
            .oneshot(
                post("/first")
                    .header("x-body-claim", "envelope")
                    .header("content-type", "application/json")
                    .body(Body::from_bytes(Bytes::from_static(b"unknown\nfirst")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            response.headers().contains_key("x-amzn-errortype"),
            "protocol-framed, not the unclaimed fallback"
        );
    }

    #[test]
    fn contradictory_ordering_fails_the_build() {
        static CYCLE: &[ProtocolOrder] = &[
            ProtocolOrder::Before("aws.protocols#restJson1"),
            ProtocolOrder::After("aws.protocols#restJson1"),
        ];
        static ABSENT: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restXml")];
        let error = MultiProtocolRoutingService::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(CYCLE)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .unwrap_err();
        assert!(matches!(error, RouterBuildError::ProtocolOrderCycle));
        // A constraint against an unserved protocol still orders the served ones transitively:
        // restJson1 comes before restXml (builtin chain) and restXml before bodyRouting, so
        // restJson1 precedes bodyRouting even though restXml is not served.
        assert_eq!(
            priority(&app(&WITH_BODY_ROUTING, [body_routing(ABSENT)])),
            ["aws.protocols#restJson1", "test#bodyRouting"]
        );
    }
}

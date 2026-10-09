/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shared fixtures for builder and request-routing tests.

use super::*;
use crate::body::{Body, BoxBody};
use crate::response::Response;
use crate::routing::SyncRoute;
use crate::schema::{DeserializeError, HttpModeledError, RequestBodyCollectionConfig, ServerProtocol};
use crate::schema::{
    OperationSchema, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, SelectedOperation, ServiceSchema,
};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, traits::HttpTrait, Schema, ShapeId, ShapeType};
use bytes::Bytes;
use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::time::Duration;

pub(super) static UNIT: Schema<'static> = Schema::new(shape_id!("test", "Unit"), ShapeType::Structure);
// Codegen records an operation's `@http` binding on its input schema.
pub(super) static FIRST_INPUT: Schema<'static> = Schema::new(shape_id!("test", "firstInput"), ShapeType::Structure)
    .with_http(HttpTrait::new("POST", "/first", Some(200)));
pub(super) static SECOND_INPUT: Schema<'static> = Schema::new(shape_id!("test", "secondInput"), ShapeType::Structure)
    .with_http(HttpTrait::new("POST", "/second", Some(200)));
pub(super) const FIRST_ID: ShapeId<'static> = shape_id!("test", "first");
pub(super) const SECOND_ID: ShapeId<'static> = shape_id!("test", "second");
pub(super) const SERVICE_ID: ShapeId<'static> = shape_id!("test", "Service");
pub(super) static FIRST: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_INPUT, &UNIT, &[]);
pub(super) static SECOND: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &SECOND_INPUT, &UNIT, &[]);
pub(super) static OPERATIONS: &[&OperationSchema<'static>] = &[&FIRST, &SECOND];
pub(super) static PROTOCOLS: &[ShapeId<'static>] = &[shape_id!("test", "bodyRouting")];
pub(super) static SERVICE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, PROTOCOLS, OPERATIONS);
pub(super) static REST_JSON: ServiceSchema<'static> =
    ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], OPERATIONS);
pub(super) static REST_XML: ServiceSchema<'static> =
    ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restXml")], OPERATIONS);
pub(super) static AWS_JSON_10: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("aws.protocols", "awsJson1_0")],
    OPERATIONS,
);
pub(super) static AWS_JSON_11: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("aws.protocols", "awsJson1_1")],
    OPERATIONS,
);
pub(super) static RPC: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[shape_id!("smithy.protocols", "rpcv2Cbor")],
    OPERATIONS,
);

/// The first line of this test protocol's request body names the operation.
#[derive(Debug, Default)]
pub(super) struct BodyProtocol {
    inner: crate::schema::protocol::RestJson1Protocol,
    factory_body_config: Option<RequestBodyCollectionConfig>,
}
#[derive(Debug)]
pub(super) struct BodyRouter {
    pub(super) targets: Vec<OperationTarget>,
}
pub(super) fn rejection(status: StatusCode, message: impl Into<Bytes>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .body(crate::body::from_bytes(message.into()))
        .unwrap()
}
/// Asserts a body-collection rejection and returns its message for failure-kind checks.
pub(super) async fn rejection_message(response: Response<BoxBody>) -> String {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
/// The stub's malformed-request diagnostic; the wire form comes from the protocol's
/// `serialize_routing_error`, which maps `MalformedRequest` to a `400`.
#[derive(Debug)]
pub(super) struct InvalidName;
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
pub(super) fn claim_mode(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-body-claim").and_then(|value| value.to_str().ok())
}
impl BodyProtocolRouter for BodyRouter {
    fn claim(&self, request: &Request<()>) -> BodyRouteClaim {
        match claim_mode(request.headers()) {
            Some("known-route") => BodyRouteClaim::ClaimedWithRoute(self.targets[0]),
            Some("envelope") => BodyRouteClaim::Claimed,
            Some("defer-head") => BodyRouteClaim::DeferredRejection(RoutingError::malformed(InvalidName)),
            _ => BodyRouteClaim::NeedsBodyToClaim,
        }
    }
    fn claim_with_body(&self, request: &Request<CollectedBody>) -> RouteClaim {
        if claim_mode(request.headers()) == Some("defer-body") {
            return RouteClaim::DeferredRejection(RoutingError::malformed(InvalidName));
        }
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
    fn from_build_context(ctx: &crate::schema::ProtocolBuildContext<'_>) -> Result<Self, RouterBuildError> {
        Ok(Self {
            factory_body_config: Some(ctx.config.request_body.global),
            ..Default::default()
        })
    }
    fn build_router(
        &self,
        ctx: RouterBuildContext<'_>,
    ) -> Result<impl BodyProtocolRouter + 'static + use<>, RouterBuildError> {
        if let Some(config) = self.factory_body_config {
            assert_eq!(config, ctx.config.request_body.global);
        }
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
pub(super) fn registry_of(registration: ProtocolRegistration) -> &'static ProtocolRegistry {
    Box::leak(Box::new(ProtocolRegistry::new(Box::leak(Box::new([registration])))))
}
pub(super) fn registry() -> &'static ProtocolRegistry {
    static REGISTRY: ProtocolRegistry =
        ProtocolRegistry::new(&[ProtocolRegistration::body_routed::<BodyProtocol>("test#bodyRouting")]);
    &REGISTRY
}
pub(super) fn binding(
    operation: &'static OperationSchema<'static>,
) -> (&'static OperationSchema<'static>, SyncRoute<Body>) {
    (
        operation,
        SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
            let selected = request
                .extensions()
                .get::<SelectedOperation>()
                .expect("selection before handler");
            assert!(std::ptr::eq(selected.operation(), operation));
            // Echo the frames, which also checks that trailers survive the routing helper.
            Ok::<_, Infallible>(Response::new(crate::body::boxed(request.into_body())))
        })),
    )
}
pub(super) fn service_builder(options: ProtocolOptions) -> MultiProtocolRoutingServiceBuilder {
    MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
        &SERVICE,
        [registry()],
        [binding(&SECOND), binding(&FIRST)],
        options,
    )
}
pub(super) fn service(options: ProtocolOptions) -> MultiProtocolRoutingService {
    service_builder(options).build().unwrap()
}

pub(super) fn config(bytes: usize, timeout_ms: u64) -> RequestBodyCollectionConfig {
    RequestBodyCollectionConfig {
        max_bytes: NonZeroUsize::new(bytes),
        read_timeout: Some(Duration::from_millis(timeout_ms)),
    }
}
pub(super) fn request(body: impl Into<Bytes>) -> Request<Body> {
    Request::new(Body::from_bytes(body.into()))
}

pub(super) static NAME: Schema<'static> =
    Schema::new_member(shape_id!("test", "In", "name"), ShapeType::String, "name", 0);
pub(super) static IN_MEMBERS: [&Schema<'static>; 1] = [&NAME];
// Both operations take the same input shape; each input schema carries its operation's `@http`.
pub(super) static FIRST_IN: Schema<'static> =
    Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS)
        .with_original_name("In")
        .with_http(HttpTrait::new("POST", "/first", Some(200)));
pub(super) static SECOND_IN: Schema<'static> =
    Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS)
        .with_original_name("In")
        .with_http(HttpTrait::new("POST", "/second", Some(200)));
pub(super) static FIRST_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_IN, &UNIT, &[]);
pub(super) static SECOND_OP: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &SECOND_IN, &UNIT, &[]);
pub(super) static OPS: &[&OperationSchema<'static>] = &[&FIRST_OP, &SECOND_OP];
/// Every built-in, declared in the reverse of their priority order.
pub(super) static BUILTINS: ServiceSchema<'static> = ServiceSchema::new(
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
pub(super) static WITH_BODY_ROUTING: ServiceSchema<'static> = ServiceSchema::new(
    SERVICE_ID,
    None,
    &[
        shape_id!("test", "bodyRouting"),
        shape_id!("aws.protocols", "restJson1"),
    ],
    OPS,
);

/// Answers with the selected protocol, the operation and the body the handler received.
pub(super) fn echo(
    operation: &'static OperationSchema<'static>,
) -> (&'static OperationSchema<'static>, SyncRoute<Body>) {
    (
        operation,
        SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
            let selected = request
                .extensions()
                .get::<SelectedOperation>()
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

pub(super) fn body_routing(order: &'static [ProtocolOrder]) -> &'static ProtocolRegistry {
    registry_of(ProtocolRegistration::body_routed::<BodyProtocol>("test#bodyRouting").with_order(order))
}

pub(super) fn app(
    service: &'static ServiceSchema<'static>,
    registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
) -> MultiProtocolRoutingService {
    MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
        service,
        registries,
        service.operations().iter().map(|operation| echo(operation)),
    )
    .build()
    .unwrap()
}

pub(super) fn priority(app: &MultiProtocolRoutingService) -> Vec<&'static str> {
    app.state
        .protocols
        .iter()
        .map(|route| route.protocol.protocol_id().as_str())
        .collect()
}

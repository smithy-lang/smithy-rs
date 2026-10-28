/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Selected-protocol rejection tests over schema serde and generated serde services.
#[path = "support/extractor_rejections.rs"]
mod harness;

use aws_smithy_http_server::{
    body::Body,
    request::FromParts,
    response::{IntoResponse, Response},
    runtime_error::RuntimeError,
    schema::protocol::{AwsJson1_0, AwsJson1_1, RestJson1, RestXml, RpcV2Cbor, ServerProtocol},
};
use harness::*;
use http::Request;
use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tower::{Service, ServiceExt};

type Protocols = (RestJson1, RpcV2Cbor);

fn shared_error(kind: &str) -> RuntimeError {
    match kind {
        "validation" => RuntimeError::Validation(MESSAGE.to_owned()),
        "serialization" => {
            RuntimeError::Serialization(aws_smithy_http_server::error::Error::new(MESSAGE))
        }
        "internal" => {
            RuntimeError::InternalFailure(aws_smithy_http_server::error::Error::new(MESSAGE))
        }
        "accept" => RuntimeError::NotAcceptable,
        "content-type" => RuntimeError::UnsupportedMediaType,
        other => panic!("unexpected rejection kind: {other}"),
    }
}

struct Caller;
impl<P> FromParts<P> for Caller {
    type Rejection = RuntimeError;
    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        aws_smithy_http_server::schema::SelectedOperation::get_from_extensions(&parts.extensions)
            .expect("schema routing supplies selection to FromParts extractors");
        let kind = parts.headers.get("x-rejection").unwrap().to_str().unwrap();
        if kind == "success" {
            Ok(Self)
        } else {
            Err(shared_error(kind))
        }
    }
}

macro_rules! shared_service {
    ($module:ident, $sdk:ident, $service:ident, $config:ident) => {
        mod $module {
            use super::*;
            use $sdk::{input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError, $service, $config};
            pub fn service(calls: Arc<AtomicUsize>) -> impl Clone + Service<Request<Body>, Response = Response, Error = Infallible> {
                $service::builder($config::builder().build())
                    .get_widget(move |input: GetWidgetInput, _: Caller| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async move { Ok::<_, GetWidgetError>(GetWidgetOutput { id: Some(input.id) }) }
                    })
                    .build_unchecked()
            }
        }
    };
}
shared_service!(
    shared_rest,
    compat_rest_server_sdk,
    ReviewRest,
    ReviewRestConfig
);
shared_service!(
    shared_cbor,
    compat_cbor_server_sdk,
    ReviewCbor,
    ReviewCborConfig
);
shared_service!(
    shared_json,
    compat_awsjson_server_sdk,
    ReviewAwsJson,
    ReviewAwsJsonConfig
);
shared_service!(
    shared_multi,
    compat_rest_cbor_server_sdk,
    ReviewRestCbor,
    ReviewRestCborConfig
);

macro_rules! shared_parity {
    ($test:ident, $wire:expr, $name:literal, $shared:ident, $legacy:ident) => {
        #[tokio::test]
        async fn $test() {
            let legacy_calls = Arc::new(AtomicUsize::new(0));
            let shared_calls = Arc::new(AtomicUsize::new(0));
            let legacy = $legacy::service(legacy_calls.clone());
            let schema = $shared::service(shared_calls.clone());
            for kind in REJECTIONS {
                let before = send(legacy.clone(), request($wire, $name, kind)).await;
                assert_eq!(before, observed_runtime_rejection($wire, kind));
                assert_schema_runtime_rejection(
                    send(schema.clone(), request($wire, $name, kind)).await,
                    $wire,
                    kind,
                );
            }
            assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
            assert_eq!(shared_calls.load(Ordering::SeqCst), 0);
            let before = send(legacy, request($wire, $name, "success")).await;
            assert_eq!(before.status, 200);
            assert_eq!(send(schema, request($wire, $name, "success")).await, before);
            assert_eq!(legacy_calls.load(Ordering::SeqCst), 1);
            assert_eq!(shared_calls.load(Ordering::SeqCst), 1);
        }
    };
}
shared_parity!(
    shared_rest_errors_match_generated_legacy,
    Wire::Rest,
    "ReviewRest",
    shared_rest,
    legacy_rest
);
shared_parity!(
    shared_cbor_rejections_use_schema_message_encoding,
    Wire::Cbor,
    "ReviewCbor",
    shared_cbor,
    legacy_cbor
);
shared_parity!(
    shared_aws_json_errors_match_generated_legacy,
    Wire::AwsJson,
    "ReviewAwsJson",
    shared_json,
    legacy_json
);

#[tokio::test]
async fn shared_error_uses_each_requests_protocol_on_one_service() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = shared_multi::service(calls.clone());
    for wire in [Wire::Rest, Wire::Cbor, Wire::Rest, Wire::Cbor] {
        for kind in REJECTIONS {
            assert_schema_runtime_rejection(
                send(service.clone(), request(wire, "ReviewRestCbor", kind)).await,
                wire,
                kind,
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for wire in [Wire::Cbor, Wire::Rest] {
        assert_eq!(
            send(service.clone(), request(wire, "ReviewRestCbor", "success"))
                .await
                .status,
            200
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

struct At<const INDEX: usize>;
impl<P, const INDEX: usize> FromParts<P> for At<INDEX> {
    type Rejection = RuntimeError;
    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let index: usize = parts
            .headers
            .get("x-fail-index")
            .unwrap()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        if index == INDEX {
            Err(shared_error("validation"))
        } else {
            Ok(Self)
        }
    }
}

macro_rules! tuple_test {
    ($name:ident, $arity:expr, $($parameter:ident: $index:literal),+) => {
        #[tokio::test]
        async fn $name() {
            use compat_rest_cbor_server_sdk::{ReviewRestCbor, ReviewRestCborConfig, input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError};
            async fn must_not_run(_: GetWidgetInput, $($parameter: At<$index>),+) -> Result<GetWidgetOutput, GetWidgetError> {
                panic!("tuple rejection reached handler")
            }
            let service = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build()).get_widget(must_not_run).build_unchecked();
            for index in 0..$arity {
                for wire in [Wire::Rest, Wire::Cbor] {
                    let mut req = request(wire, "ReviewRestCbor", "validation");
                    req.headers_mut().insert("x-fail-index", index.to_string().parse().unwrap());
                    assert_schema_runtime_rejection(send(service.clone(), req).await, wire, "validation");
                }
            }
        }
    };
}
tuple_test!(two_extractors_forward_selection, 2, _a: 0, _b: 1);
tuple_test!(three_extractors_forward_selection, 3, _a: 0, _b: 1, _c: 2);
tuple_test!(four_extractors_forward_selection, 4, _a: 0, _b: 1, _c: 2, _d: 3);
tuple_test!(five_extractors_forward_selection, 5, _a: 0, _b: 1, _c: 2, _d: 3, _e: 4);
tuple_test!(six_extractors_forward_selection, 6, _a: 0, _b: 1, _c: 2, _d: 3, _e: 4, _f: 5);
tuple_test!(seven_extractors_forward_selection, 7, _a: 0, _b: 1, _c: 2, _d: 3, _e: 4, _f: 5, _g: 6);
tuple_test!(eight_extractors_forward_selection, 8, _a: 0, _b: 1, _c: 2, _d: 3, _e: 4, _f: 5, _g: 6, _h: 7);

struct ContextRejection;
impl std::fmt::Display for ContextRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("context-aware rejection")
    }
}
// The context-free impl is a canary: the schema upgrade must use the contextual trait.
impl<P> IntoResponse<P> for ContextRejection {
    fn into_response(self) -> Response {
        panic!("schema upgrade fell back to the context-free conversion")
    }

    fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response {
        Response::builder()
            .status(403)
            .header("x-selected-protocol", protocol.protocol_id().to_string())
            .extension(CustomResponseMarker("context-aware"))
            .body(aws_smithy_http_server::body::to_boxed(
                "custom selected response",
            ))
            .unwrap()
    }
}
struct ContextCaller;
impl<P> FromParts<P> for ContextCaller {
    type Rejection = ContextRejection;
    fn from_parts(_: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        Err(ContextRejection)
    }
}
struct Always;
impl<P> FromParts<P> for Always {
    type Rejection = Infallible;
    fn from_parts(_: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        Ok(Self)
    }
}

#[tokio::test]
async fn custom_override_receives_selection_through_a_tuple() {
    use compat_rest_cbor_server_sdk::{
        error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, ReviewRestCbor,
        ReviewRestCborConfig,
    };
    async fn must_not_run(
        _: GetWidgetInput,
        _: Always,
        _: ContextCaller,
    ) -> Result<GetWidgetOutput, GetWidgetError> {
        panic!("custom rejection reached handler")
    }
    let service = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .get_widget(must_not_run)
        .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor, Wire::Rest] {
        let response = service
            .clone()
            .oneshot(request(wire, "ReviewRestCbor", "custom"))
            .await
            .unwrap();
        assert_eq!(
            response.extensions().get::<CustomResponseMarker>(),
            Some(&CustomResponseMarker("context-aware"))
        );
        let reply = snapshot(response).await;
        assert_eq!(reply.status, 403);
        assert_eq!(
            reply.headers.get("x-selected-protocol").unwrap(),
            match wire {
                Wire::Rest => "aws.protocols#restJson1",
                Wire::Cbor => "smithy.protocols#rpcv2Cbor",
                _ => unreachable!(),
            }
        );
        assert_eq!(reply.body, "custom selected response");
    }
}

struct TrackedBody {
    polls: Arc<AtomicUsize>,
}
impl http_body::Body for TrackedBody {
    type Data = bytes::Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        panic!("streaming extractor rejection polled the live body")
    }
}
fn streaming_request(wire: Wire, kind: &str, polls: Arc<AtomicUsize>) -> Request<Body> {
    let mut req = request(wire, "ReviewRestCbor", kind);
    *req.uri_mut() = match wire {
        Wire::Rest => "/download",
        Wire::Cbor => "/service/ReviewRestCbor/operation/DownloadBlob",
        _ => unreachable!(),
    }
    .parse()
    .unwrap();
    *req.body_mut() = Body::new(TrackedBody { polls });
    req
}

#[tokio::test]
async fn streaming_shared_rejections_use_selection_without_polling_body() {
    use compat_rest_cbor_server_sdk::{
        input::DownloadBlobInput, output::DownloadBlobOutput, ReviewRestCbor, ReviewRestCborConfig,
    };
    async fn must_not_run(_: DownloadBlobInput, _: Caller) -> DownloadBlobOutput {
        panic!("streaming rejection reached handler")
    }
    let service = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .download_blob(must_not_run)
        .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor] {
        for kind in REJECTIONS {
            let polls = Arc::new(AtomicUsize::new(0));
            assert_schema_runtime_rejection(
                send(
                    service.clone(),
                    streaming_request(wire, kind, polls.clone()),
                )
                .await,
                wire,
                kind,
            );
            assert_eq!(polls.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn streaming_custom_response_survives_default_hook_without_polling_body() {
    use compat_rest_cbor_server_sdk::{
        input::DownloadBlobInput, output::DownloadBlobOutput, ReviewRestCbor, ReviewRestCborConfig,
    };
    async fn must_not_run(_: DownloadBlobInput, _: Always, _: CustomCaller) -> DownloadBlobOutput {
        panic!("streaming custom rejection reached handler")
    }
    let service = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .download_blob(must_not_run)
        .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor] {
        let polls = Arc::new(AtomicUsize::new(0));
        assert_custom_rejection(
            service.clone(),
            streaming_request(wire, "custom", polls.clone()),
        )
        .await;
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn shared_error_without_selection_has_only_semantic_status_and_extension() {
    for kind in REJECTIONS {
        let response = IntoResponse::<Protocols>::into_response(shared_error(kind));
        let expected = observed_runtime_rejection(Wire::Rest, kind);
        assert_eq!(response.status().as_u16(), expected.status);
        assert_eq!(
            response
                .extensions()
                .get::<aws_smithy_http_server::extension::RuntimeErrorExtension>()
                .map(|e| e.to_string()),
            expected.runtime_error
        );
        assert!(response.headers().is_empty());
        use http_body_util::BodyExt;
        assert!(response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
    }
}

#[tokio::test]
async fn shared_serializer_matches_all_five_protocol_local_conversions() {
    use http_body_util::BodyExt;
    macro_rules! compare {
        ($wire:expr, $protocol:expr, $marker:ty, $error:ty) => {
            for kind in REJECTIONS {
                use $error as LegacyError;
                let legacy = match kind {
                    "validation" => LegacyError::Validation(MESSAGE.to_owned().into()),
                    "serialization" => LegacyError::Serialization(
                        aws_smithy_http_server::error::Error::new(MESSAGE),
                    ),
                    "internal" => LegacyError::InternalFailure(
                        aws_smithy_http_server::error::Error::new(MESSAGE),
                    ),
                    "accept" => LegacyError::NotAcceptable,
                    "content-type" => LegacyError::UnsupportedMediaType,
                    _ => unreachable!(),
                };
                let before: Response = IntoResponse::<$marker>::into_response(legacy);
                let after: Response = IntoResponse::<$marker>::into_response_with_protocol(
                    shared_error(kind),
                    &$protocol,
                );
                assert_eq!(after.status(), before.status(), "{kind}");
                assert_eq!(after.headers(), before.headers(), "{kind}");
                assert_eq!(
                    after
                        .extensions()
                        .get::<aws_smithy_http_server::extension::RuntimeErrorExtension>()
                        .map(|e| e.to_string()),
                    before
                        .extensions()
                        .get::<aws_smithy_http_server::extension::RuntimeErrorExtension>()
                        .map(|e| e.to_string())
                );
                assert_schema_rejection_body(
                    &after.into_body().collect().await.unwrap().to_bytes(),
                    &before.into_body().collect().await.unwrap().to_bytes(),
                    $wire,
                    kind,
                );
            }
        };
    }
    compare!(
        Wire::Rest,
        RestJson1::default(),
        RestJson1,
        aws_smithy_http_server::protocol::rest_json_1::runtime_error::RuntimeError
    );
    compare!(
        Wire::Cbor,
        RpcV2Cbor::default(),
        RpcV2Cbor,
        aws_smithy_http_server::protocol::rpc_v2_cbor::runtime_error::RuntimeError
    );
    compare!(
        Wire::AwsJson,
        AwsJson1_0::default(),
        AwsJson1_0,
        aws_smithy_http_server::protocol::aws_json::runtime_error::RuntimeError
    );
    compare!(
        Wire::AwsJson11,
        AwsJson1_1::default(),
        AwsJson1_1,
        aws_smithy_http_server::protocol::aws_json::runtime_error::RuntimeError
    );
    compare!(
        Wire::Xml,
        RestXml::default(),
        RestXml,
        aws_smithy_http_server::protocol::rest_xml::runtime_error::RuntimeError
    );
}

/// External protocol implementation: ordinary serialization delegates to restJson1,
/// while runtime-error framing is deliberately recognizable and counted.
#[derive(Debug)]
struct ExternalProtocol {
    inner: RestJson1,
    calls: Arc<AtomicUsize>,
}
impl ServerProtocol for ExternalProtocol {
    fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
        static ID: aws_smithy_schema::ShapeId<'static> =
            aws_smithy_schema::shape_id!("test", "externalProtocol");
        &ID
    }
    fn deserialize_request<'a>(
        &'a self,
        input: &aws_smithy_schema::Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
    ) -> Result<
        Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>,
        aws_smithy_http_server::schema::DeserializeError,
    > {
        self.inner.deserialize_request(input, request)
    }
    fn serialize_response(
        &self,
        output: &aws_smithy_schema::Schema<'_>,
        value: &dyn aws_smithy_schema::serde::SerializableStruct,
    ) -> Response {
        self.inner.serialize_response(output, value)
    }
    fn serialize_streaming_response(
        &self,
        output: &aws_smithy_schema::Schema<'_>,
        value: &dyn aws_smithy_schema::serde::SerializableStruct,
        member: &aws_smithy_schema::Schema<'_>,
        body: Option<aws_smithy_http_server::body::BoxBody>,
    ) -> Response {
        self.inner
            .serialize_streaming_response(output, value, member, body)
    }
    fn serialize_error(
        &self,
        error: &dyn aws_smithy_http_server::schema::HttpModeledError,
    ) -> Response {
        self.inner.serialize_error(error)
    }
    fn serialize_rejection(
        &self,
        error: aws_smithy_http_server::schema::DeserializeError,
    ) -> Response {
        self.inner.serialize_rejection(error)
    }
    fn serialize_runtime_error(&self, error: RuntimeError) -> Response {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(matches!(error, RuntimeError::Validation(ref message) if message == MESSAGE));
        Response::builder()
            .status(422)
            .header("x-external-protocol", "selected")
            .body(aws_smithy_http_server::body::to_boxed(
                "external runtime error",
            ))
            .unwrap()
    }
}

shared_service!(
    shared_json11,
    compat_awsjson11_server_sdk,
    ReviewAwsJson11,
    ReviewAwsJson11Config
);
shared_service!(
    shared_xml,
    compat_xml_server_sdk,
    ReviewXml,
    ReviewXmlConfig
);
shared_parity!(
    shared_aws_json_11_errors_match_generated_legacy,
    Wire::AwsJson11,
    "ReviewAwsJson11",
    shared_json11,
    legacy_json11
);
shared_parity!(
    shared_xml_errors_match_generated_legacy,
    Wire::Xml,
    "ReviewXml",
    shared_xml,
    legacy_xml
);

shared_service!(
    shared_all,
    compat_all_server_sdk,
    ReviewAll,
    ReviewAllConfig
);

#[tokio::test]
async fn one_service_frames_runtime_rejections_for_all_five_protocols() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = shared_all::service(calls.clone());
    for wire in [
        Wire::Rest,
        Wire::Cbor,
        Wire::AwsJson,
        Wire::AwsJson11,
        Wire::Xml,
    ] {
        for kind in REJECTIONS {
            assert_schema_runtime_rejection(
                send(service.clone(), request(wire, "ReviewAll", kind)).await,
                wire,
                kind,
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            match wire {
                Wire::Rest => 0,
                Wire::Cbor => 1,
                Wire::AwsJson => 2,
                Wire::AwsJson11 => 3,
                Wire::Xml => 4,
            }
        );
        assert_eq!(
            send(service.clone(), request(wire, "ReviewAll", "success"))
                .await
                .status,
            200
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

static REGISTERED_CALLS: std::sync::LazyLock<Arc<AtomicUsize>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicUsize::new(0)));

impl aws_smithy_http_server::schema::MetadataRoutedProtocol for ExternalProtocol {
    fn from_build_context(
        _: &aws_smithy_http_server::schema::ProtocolBuildContext<'_>,
    ) -> Result<Self, aws_smithy_http_server::schema::routing::RouterBuildError> {
        Ok(Self {
            inner: RestJson1::default(),
            calls: REGISTERED_CALLS.clone(),
        })
    }
    fn build_router(
        &self,
        ctx: aws_smithy_http_server::schema::routing::RouterBuildContext<'_>,
    ) -> Result<
        impl aws_smithy_http_server::schema::routing::MetadataProtocolRouter + 'static + use<>,
        aws_smithy_http_server::schema::routing::RouterBuildError,
    > {
        aws_smithy_http_server::schema::MetadataRoutedProtocol::build_router(&self.inner, ctx)
    }
}

#[tokio::test]
async fn registered_external_protocol_routes_to_the_generated_operation_upgrade() {
    use aws_smithy_http_server::{
        operation::{DynUpgradePlugin, SchemaOperationShape},
        plugin::Plugin,
        routing::SyncRoute,
        schema::routing::MultiProtocolRoutingServiceBuilder,
        schema::{ProtocolOrder, ProtocolRegistration, ProtocolRegistry, ServiceSchema},
    };
    use compat_rest_server_sdk::{
        error::GetWidgetError, input::GetWidgetInput, operation_shape::GetWidget,
        output::GetWidgetOutput, ReviewRest,
    };
    static SCHEMA: ServiceSchema<'static> = ServiceSchema::new(
        aws_smithy_schema::shape_id!("test", "ExternalService"),
        Some("2024-01-01"),
        &[aws_smithy_schema::shape_id!("test", "externalProtocol")],
        &[<GetWidget as SchemaOperationShape>::SCHEMA],
    );
    static REGISTRY: ProtocolRegistry =
        ProtocolRegistry::new(&[ProtocolRegistration::metadata_routed::<ExternalProtocol>(
            "test#externalProtocol",
        )
        .with_order(&[ProtocolOrder::Before("aws.protocols#restJson1")])]);
    let handler = tower::service_fn(|_: (GetWidgetInput, (Caller,))| async {
        panic!("registered protocol rejection reached handler");
        #[allow(unreachable_code)]
        Ok::<_, GetWidgetError>(GetWidgetOutput { id: None })
    });
    let upgrade = <DynUpgradePlugin<(Caller,)> as Plugin<ReviewRest, GetWidget, _>>::apply(
        &DynUpgradePlugin::new(),
        handler,
    );
    let before = REGISTERED_CALLS.load(Ordering::SeqCst);
    let service = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
        &SCHEMA,
        [&REGISTRY],
        [(
            <GetWidget as SchemaOperationShape>::SCHEMA,
            SyncRoute::new(upgrade),
        )],
    )
    .build()
    .unwrap();
    let reply = send(
        service,
        request(Wire::Rest, "ExternalService", "validation"),
    )
    .await;
    assert_eq!(REGISTERED_CALLS.load(Ordering::SeqCst), before + 1);
    assert_eq!(reply.status, 422);
    assert_eq!(
        reply.headers.get("x-external-protocol").unwrap(),
        "selected"
    );
    assert_eq!(reply.body, "external runtime error");
    assert!(!reply.headers.contains_key("x-amzn-errortype"));
}

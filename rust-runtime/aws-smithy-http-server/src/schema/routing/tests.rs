/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use super::test_helpers::*;
use super::*;
use crate::body::{Body, BoxBody};
use crate::config::RequestBodyCollectionConfig;
use crate::error::Error;
use crate::protocol::rpc_v2_cbor::SMITHY_PROTOCOL_HEADER;
use crate::response::Response;
use crate::routing::SyncRoute;
use crate::schema::{DeserializeError, HttpModeledError};
use crate::schema::{OperationSchema, ProtocolOrder, SelectedOperation, ServiceSchema};
use aws_smithy_schema::{shape_id, traits::HttpTrait, Schema, ShapeType};
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

fn strict_context(targets: &[OperationTarget]) -> RouterBuildContext<'_> {
    static CONFIG: std::sync::LazyLock<crate::config::ServiceConfig> = std::sync::LazyLock::new(Default::default);
    RouterBuildContext {
        service: &REST_JSON_AND_RPC_V2_CBOR,
        targets,
        config: &CONFIG,
        protocol_settings: None,
    }
}

async fn assert_wire_response(actual: Response<BoxBody>, expected: Response<BoxBody>) {
    assert_eq!(actual.status(), expected.status());
    assert_eq!(actual.headers(), expected.headers());
    assert_eq!(
        actual.into_body().collect().await.unwrap().to_bytes(),
        expected.into_body().collect().await.unwrap().to_bytes(),
    );
}

#[test]
fn external_routers_inspect_service_protocols_from_the_schema() {
    use crate::schema::ProtocolRegistration;
    use aws_smithy_types::Document;

    static BODY_SERVICE: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("test", "bodyRouting"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        OPERATIONS,
    );
    static METADATA_SERVICE: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("test", "metadataRouting"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        OPERATIONS,
    );
    static ORDER: &[ProtocolOrder] = &[
        ProtocolOrder::Before("aws.protocols#restJson1"),
        ProtocolOrder::After("smithy.protocols#rpcv2Cbor"),
    ];

    for (service, id, registration) in [
        (
            &BODY_SERVICE,
            shape_id!("test", "bodyRouting"),
            ProtocolRegistration::body_routed::<BodyProtocol>("test#bodyRouting"),
        ),
        (
            &METADATA_SERVICE,
            shape_id!("test", "metadataRouting"),
            ProtocolRegistration::metadata_routed::<MetadataProtocol>("test#metadataRouting"),
        ),
    ] {
        let expected = Document::Array(
            ["aws.protocols#restJson1", id.as_str(), "smithy.protocols#rpcv2Cbor"]
                .into_iter()
                .map(|id| Document::String(id.into()))
                .collect(),
        );
        let options = ProtocolOptions::default().with_protocol_settings(HashMap::from([(
            id,
            Document::Object(HashMap::from([("expectedServiceProtocols".into(), expected)])),
        )]));
        MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
            service,
            [registry_of(registration.with_order(ORDER))],
            OPERATIONS.iter().map(|operation| echo(operation)),
            options,
        )
        .build()
        .unwrap();
    }
}

#[tokio::test]
async fn sole_aws_json_claims_preserve_native_uri_method_and_target_rejections() {
    use crate::protocol::aws_json::router::AwsJsonRouter;
    use crate::protocol::aws_json_10::AwsJson1_0;
    use crate::protocol::aws_json_11::AwsJson1_1;
    use crate::response::IntoResponse;
    use crate::routing::Router;

    let legacy = AwsJsonRouter::from_owned([("Service.first".to_owned(), ())]);
    for schema in [&AWS_JSON_10, &AWS_JSON_11] {
        let app = app(schema, []);
        for (method, uri, target) in [
            ("POST", "/?foo=bar", Some(HeaderValue::from_static("Service.first"))),
            ("GET", "/", Some(HeaderValue::from_static("Service.first"))),
            ("POST", "/", None),
            ("POST", "/", Some(HeaderValue::from_bytes(b"\xff").unwrap())),
            ("POST", "/", Some(HeaderValue::from_static("Service.unknown"))),
        ] {
            let mut request = Request::builder().method(method).uri(uri).body(()).unwrap();
            if let Some(target) = target {
                request.headers_mut().insert("x-amz-target", target);
            }
            let SharedProtocolRouter::Metadata(router) = &app.state.protocols[0].router else {
                unreachable!()
            };
            assert!(matches!(router.claim(&request), RouteClaim::DeferredRejection(_)));
            let error = legacy.match_route(&request).unwrap_err();
            let expected = if std::ptr::eq(schema, &AWS_JSON_10) {
                IntoResponse::<AwsJson1_0>::into_response(error)
            } else {
                IntoResponse::<AwsJson1_1>::into_response(error)
            };
            let (parts, ()) = request.into_parts();
            let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
                |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> { panic!("routing rejection read its body") },
            )));
            let actual = app.clone().oneshot(Request::from_parts(parts, body)).await.unwrap();
            assert_wire_response(actual, expected).await;
        }
        for content_type in [None, Some("text/plain")] {
            let mut builder = Request::builder()
                .method("POST")
                .uri("/")
                .header("x-amz-target", "Service.first");
            if let Some(content_type) = content_type {
                builder = builder.header("content-type", content_type);
            }
            assert_eq!(
                app.clone()
                    .oneshot(builder.body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK,
            );
        }
    }
}

#[tokio::test]
async fn sole_cbor_claims_serialize_native_header_rejections() {
    use crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter;
    use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
    use crate::response::IntoResponse;
    use crate::routing::Router;

    let legacy = RpcV2CborRouter::from_owned([("Service.first".to_owned(), ())]);
    let app = app(&RPC, []);
    for (header, forbidden) in [
        (None, None),
        (Some(HeaderValue::from_static("malformed")), None),
        (Some(HeaderValue::from_static("rpc-v2-json")), None),
        (Some(HeaderValue::from_bytes(b"\xff").unwrap()), None),
        (Some(HeaderValue::from_static("rpc-v2-cbor")), Some("x-amz-target")),
        (Some(HeaderValue::from_static("rpc-v2-cbor")), Some("x-amzn-target")),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/service/Service/operation/first")
            .body(())
            .unwrap();
        if let Some(header) = header {
            request.headers_mut().insert(SMITHY_PROTOCOL_HEADER.clone(), header);
        }
        if let Some(forbidden) = forbidden {
            request
                .headers_mut()
                .insert(forbidden, HeaderValue::from_static("Service.first"));
        }
        let SharedProtocolRouter::Metadata(router) = &app.state.protocols[0].router else {
            unreachable!()
        };
        let RouteClaim::DeferredRejection(error) = router.claim(&request) else {
            panic!("sole CBOR must defer native header rejections");
        };
        let native_error = legacy.match_route(&request).unwrap_err();
        let expected = IntoResponse::<RpcV2Cbor>::into_response(native_error);
        let native: RoutingError = legacy.match_route(&request).unwrap_err().into();
        assert_eq!(error.kind(), native.kind());
        // Generated serde services return 404 with only content-type and an empty body.
        assert_eq!(expected.status(), StatusCode::NOT_FOUND);
        assert_eq!(expected.headers().len(), 1);
        assert_eq!(expected.headers()["content-type"], "application/cbor");
        let (parts, ()) = request.into_parts();
        let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> { panic!("CBOR rejection read its body") },
        )));
        assert_wire_response(
            app.clone().oneshot(Request::from_parts(parts, body)).await.unwrap(),
            expected,
        )
        .await;
    }
}

#[tokio::test]
async fn sole_rest_rejections_preserve_legacy_responses() {
    use crate::protocol::rest::router::Error as RestError;
    use crate::protocol::rest_json_1::RestJson1;
    use crate::protocol::rest_xml::RestXml;
    use crate::response::IntoResponse;

    for schema in [&REST_JSON, &REST_XML] {
        for (method, uri, error) in [
            ("POST", "/unknown", RestError::NotFound),
            ("GET", "/first", RestError::MethodNotAllowed),
        ] {
            let expected = if std::ptr::eq(schema, &REST_JSON) {
                IntoResponse::<RestJson1>::into_response(error)
            } else {
                IntoResponse::<RestXml>::into_response(error)
            };
            let actual = app(schema, [])
                .oneshot(Request::builder().method(method).uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_wire_response(actual, expected).await;
        }
    }
}

#[tokio::test]
async fn sole_custom_metadata_router_retains_control_of_claims() {
    use crate::schema::routing::service::ProtocolAndRouter;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CustomRouter {
        claims: Arc<AtomicUsize>,
        selected: Option<OperationTarget>,
    }
    impl MetadataProtocolRouter for CustomRouter {
        fn route(&self, _: &Request<()>) -> Result<OperationTarget, RoutingError> {
            panic!("NoClaim and ClaimedWithRoute must never invoke route")
        }
        fn claim(&self, _: &Request<()>) -> RouteClaim {
            self.claims.fetch_add(1, Ordering::SeqCst);
            match self.selected {
                Some(target) => RouteClaim::ClaimedWithRoute(target),
                None => RouteClaim::NoClaim,
            }
        }
    }
    for selected in [None, Some(OperationTarget::new(0, &FIRST))] {
        let mut app = app(&REST_JSON, []);
        let claims = Arc::new(AtomicUsize::new(0));
        let protocol = app.state.protocols[0].protocol.clone();
        Arc::get_mut(&mut app.state).unwrap().protocols = Box::from([ProtocolAndRouter {
            protocol,
            router: SharedProtocolRouter::new(CustomRouter {
                claims: claims.clone(),
                selected,
            }),
        }]);
        let response = app.oneshot(Request::new(Body::empty())).await.unwrap();
        assert_eq!(claims.load(Ordering::SeqCst), 1);
        assert_eq!(
            response.status(),
            if selected.is_some() {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        if selected.is_none() {
            assert!(response.headers().is_empty());
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "<UnknownOperationException/>\n"
            );
        }
    }
}

#[test]
fn streaming_recognition_agrees_with_single_and_multiple_enabled_protocols() {
    static EVENT: Schema<'static> = Schema::new_member(
        shape_id!("test", "EventsInput", "events"),
        ShapeType::Union,
        "events",
        0,
    )
    .with_streaming()
    .with_http_payload();
    static INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "EventsInput"), ShapeType::Structure, &[&EVENT])
            .with_http(HttpTrait::new("POST", "/events", Some(200)));
    static OPERATION: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &INPUT, &UNIT, &[]);
    let targets = [OperationTarget::new(0, &OPERATION)];
    for service in [&REST_JSON_AND_RPC_V2_CBOR, &REST_JSON] {
        let mut context = strict_context(&targets);
        context.service = service;
        let is_sole_protocol = context.service_protocols().len() == 1;
        let rest = rest_router(&context, "application/json", &[]).unwrap();
        let aws = aws_json_router(&context, "application/x-amz-json-1.1").unwrap();
        let cbor = rpc_v2_cbor_router(&context).unwrap();
        for content_type in [None, Some("text/plain"), Some("application/vnd.amazon.eventstream")] {
            for (router, uri, target) in [
                (&rest as &dyn MetadataProtocolRouter, "/events", None),
                (&aws as &dyn MetadataProtocolRouter, "/", Some("Service.first")),
            ] {
                let mut builder = Request::builder().method("POST").uri(uri);
                if let Some(target) = target {
                    builder = builder.header("x-amz-target", target);
                }
                if let Some(content_type) = content_type {
                    builder = builder.header("content-type", content_type);
                }
                let request = builder.body(()).unwrap();
                let expected = is_sole_protocol || content_type == Some("application/vnd.amazon.eventstream");
                assert_eq!(router.recognizes_streaming_input(&request), expected);
                assert_eq!(
                    matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)),
                    expected
                );
            }
        }
        for protocol_header in [None, Some("wrong"), Some("rpc-v2-cbor")] {
            let mut builder = Request::builder()
                .method("POST")
                .uri("/service/Service/operation/first");
            if let Some(protocol_header) = protocol_header {
                builder = builder.header(SMITHY_PROTOCOL_HEADER, protocol_header);
            }
            let request = builder.body(()).unwrap();
            assert_eq!(
                cbor.recognizes_streaming_input(&request),
                protocol_header == Some("rpc-v2-cbor")
            );
            match (is_sole_protocol, protocol_header) {
                (false, Some("rpc-v2-cbor")) => {
                    assert!(matches!(cbor.claim(&request), RouteClaim::Claimed));
                }
                (true, Some("rpc-v2-cbor")) => {
                    assert!(matches!(cbor.claim(&request), RouteClaim::ClaimedWithRoute(_)));
                }
                (false, _) => assert!(matches!(cbor.claim(&request), RouteClaim::NoClaim)),
                (true, _) => {
                    assert!(matches!(cbor.claim(&request), RouteClaim::DeferredRejection(_)));
                }
            }
        }
    }
}

#[tokio::test]
async fn separately_built_layers_leave_existing_services_and_requests_unchanged() {
    let mut original = service(ProtocolOptions::default());
    let cloned = original.clone();
    assert!(Arc::ptr_eq(&original.state, &cloned.state));
    let pending = original.call(request("first\npayload"));
    let layered = service_builder(ProtocolOptions::default())
        .layer(tower::layer::layer_fn(|route: SyncRoute<Body>| {
            tower::util::MapResponse::new(route, |mut response: Response<BoxBody>| {
                response
                    .headers_mut()
                    .insert("x-layer", HeaderValue::from_static("applied"));
                response
            })
        }))
        .build()
        .unwrap();
    assert!(!Arc::ptr_eq(&original.state, &layered.state));
    let response = layered.oneshot(request("first\npayload")).await.unwrap();
    assert_eq!(response.headers()["x-layer"], "applied");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "first\npayload"
    );
    for response in [
        pending.await.unwrap(),
        original.oneshot(request("first\npayload")).await.unwrap(),
    ] {
        assert!(!response.headers().contains_key("x-layer"));
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "first\npayload"
        );
    }
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
    let response = service(ProtocolOptions::default())
        .oneshot(Request::new(body))
        .await
        .unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.trailers(), Some(&trailers));
    assert_eq!(collected.to_bytes(), "first\n123");
    let response = service(ProtocolOptions::default())
        .oneshot(request("second\npayload"))
        .await
        .unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "second\npayload"
    );
}

/// The generated serde router enums classify onto the three standard kinds; in particular the rpcv2
/// wire-format errors split — a missing or unsupported `smithy-protocol` header means the
/// request never identified the protocol, while an
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
        let response = service(ProtocolOptions::default())
            .oneshot(request(body))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn provisional_maximum_caps_collection_and_is_the_only_routing_limit() {
    let mut options = ProtocolOptions::default();
    options.service_config.request_body.global = config(8, 1000);
    options
        .service_config
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), config(64, 1000));
    options
        .service_config
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
    let mut options = ProtocolOptions::default();
    options.service_config.request_body.global = config(8, 1000);
    options
        .service_config
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), RequestBodyCollectionConfig::default());
    // The provisional allowance the router was built with: an explicit unlimited override
    // dominates the global limit on both axes.
    let provisional = options.service_config.request_body.for_routing();
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

#[test]
fn routing_poll_budget_uses_the_largest_override_and_none_disables_it() {
    let global = RequestBodyCollectionConfig::default().with_read_poll_budget(NonZeroUsize::new(3));
    let mut config = crate::config::ServiceRequestBodyConfig::default().with_global(global);
    assert_eq!(config.for_operation(FIRST.shape_id()), global);
    config.per_operation.insert(
        FIRST.shape_id().to_string(),
        RequestBodyCollectionConfig::default().with_read_poll_budget(NonZeroUsize::new(7)),
    );
    assert_eq!(config.for_routing().read_poll_budget, NonZeroUsize::new(7));
    assert_eq!(
        config.for_operation(FIRST.shape_id()).read_poll_budget,
        NonZeroUsize::new(7)
    );
    assert_eq!(config.for_operation(SECOND.shape_id()), global);
    config.per_operation.insert(
        FIRST.shape_id().to_string(),
        RequestBodyCollectionConfig::default().with_read_poll_budget(None),
    );
    assert_eq!(config.for_routing().read_poll_budget, None);
}

#[tokio::test(start_paused = true)]
async fn collection_enforces_the_provisional_maximum_timeout() {
    let mut options = ProtocolOptions::default();
    options.service_config.request_body.global = config(64, 50);
    options
        .service_config
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), config(64, 500));
    assert_eq!(
        options.service_config.request_body.for_routing().read_timeout,
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
        service(ProtocolOptions::default())
            .oneshot(Request::new(body))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
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
            .map(|op| (*op, SyncRoute::new(crate::operation::SchemaMissingFailure)));
        let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(schema, [], bindings)
            .build()
            .unwrap();
        let expected = app.state.protocols[0]
            .protocol
            .serialize_rejection(DeserializeError::InternalFailure(Error::new(String::from(
                "the operation has not been set",
            ))));
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor");
        if let Some(target) = target {
            req = req.header("x-amz-target", target).header(
                "content-type",
                if std::ptr::eq(schema, &AWS_JSON_10) {
                    "application/x-amz-json-1.0"
                } else {
                    "application/x-amz-json-1.1"
                },
            );
        } else if std::ptr::eq(schema, &REST_JSON) {
            req = req.header("content-type", "application/json");
        } else if std::ptr::eq(schema, &REST_XML) {
            req = req.header("content-type", "application/xml");
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
        let options = ProtocolOptions {
            protocol_settings: HashMap::from([(
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                crate::schema::settings::parse_settings_json(settings.as_bytes()),
            )]),
            ..Default::default()
        };
        let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
            &RPC,
            [],
            [binding(&SECOND), binding(&FIRST)],
            options,
        )
        .build()
        .unwrap();
        for (path, status) in [
            ("/service/Service/operation/First", capitalized_status),
            ("/service/Service/operation/first", StatusCode::OK),
            (
                "/prefix/service/com.example.Service/operation/first?trace=true",
                StatusCode::OK,
            ),
            (
                "/prefix/service/com.example.Service/operation/First",
                capitalized_status,
            ),
            ("/service/Other/operation/first", StatusCode::NOT_FOUND),
            ("/service/Service/operation/first/", StatusCode::NOT_FOUND),
        ] {
            let req = Request::builder()
                .method("POST")
                .uri(path)
                .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
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

#[tokio::test]
async fn typed_request_body_limits_and_overrides_are_applied() {
    let oversized = || request(format!("first\n{}", "x".repeat(100)));
    let options = || {
        ProtocolOptions::default().with_service_config(
            crate::config::ServiceConfig::default().with_request_body(
                crate::config::ServiceRequestBodyConfig::default()
                    .with_global(RequestBodyCollectionConfig::default().with_max_bytes(NonZeroUsize::new(8))),
            ),
        )
    };
    let response = service(options()).oneshot(oversized()).await.unwrap();
    assert!(rejection_message(response)
        .await
        .contains("exceeded the configured maximum"));

    // Replacing the configuration with its default explicitly allows any body size.
    let unlimited = options().with_request_body(crate::config::ServiceRequestBodyConfig::default());
    let response = service(unlimited).oneshot(oversized()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut larger = options();
    larger.service_config.request_body.global = config(1024, 1000);
    let response = service(larger).oneshot(oversized()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // An operation override replaces the whole record, including the inherited limit.
    let mut per_operation = options();
    per_operation
        .service_config
        .request_body
        .per_operation
        .insert(FIRST.shape_id().to_string(), RequestBodyCollectionConfig::default());
    let response = service(per_operation).oneshot(oversized()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn layers_see_selection_and_do_not_observe_routing_rejections() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let layer = tower::layer::layer_fn(move |inner: SyncRoute<Body>| {
        let counter = counter.clone();
        tower::service_fn(move |request: Request<Body>| {
            assert!(SelectedOperation::get_from_request(&request).is_some());
            counter.fetch_add(1, Ordering::SeqCst);
            inner.clone().oneshot(request)
        })
    });
    let app = service_builder(ProtocolOptions::default())
        .layer(layer)
        .build()
        .unwrap();
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
        for budget in [NonZeroUsize::new(32), NonZeroUsize::new(3), None] {
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
            let mut options = ProtocolOptions::default();
            options.service_config.request_body.global = config(limit, 1000).with_read_poll_budget(budget);
            let mut app = service(options);
            let mut future = Box::pin(app.call(Request::new(Body::new(http_body_util::StreamBody::new(frames)))));
            let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
            let waker = std::task::Waker::from(wakes.clone());
            let first_poll = future.as_mut().poll(&mut Context::from_waker(&waker));
            let response = if let Some(budget) = budget {
                assert!(first_poll.is_pending());
                assert_eq!(reads.load(Ordering::SeqCst), budget.get());
                assert_eq!(wakes.0.load(Ordering::SeqCst), 1, "yield must schedule another poll");
                future.await.unwrap()
            } else {
                assert_eq!(reads.load(Ordering::SeqCst), 1002);
                assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
                let Poll::Ready(response) = first_poll else {
                    panic!("disabled budget must not force a yield");
                };
                response.unwrap()
            };
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}

#[tokio::test]
async fn claim_with_complete_body_can_defer_to_routing() {
    for (body, status) in [
        ("first\npayload", StatusCode::OK),
        ("unknown\npayload", StatusCode::NOT_FOUND),
        ("", StatusCode::NOT_FOUND),
    ] {
        let response = service(ProtocolOptions::default())
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
    let mut service = service(ProtocolOptions::default());
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
    let router = rest_router(&strict_context(&targets), "application/json", &[]).unwrap();
    let shared = SharedProtocolRouter::new(rest_router(&strict_context(&targets), "application/json", &[]).unwrap());
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

    let config = ProtocolOptions::default();
    for (operation, input, output, blob, event_input) in [
        (&FIRST, false, false, false, false),
        (&INPUT_BLOB, true, false, true, false),
        (&OUTPUT_BLOB, false, true, true, false),
        (&INPUT_EVENT, true, false, false, true),
        (&OUTPUT_EVENT, false, true, false, false),
        (&BOTH, true, true, true, true),
    ] {
        // Metadata and CBOR eligibility must not depend on a dense handler-index table.
        let target = OperationTarget::new(usize::MAX, operation);
        assert_eq!(target.index(), usize::MAX);
        assert!(std::ptr::eq(target.operation(), operation));
        assert_eq!(target.has_streaming_input(), input);
        assert_eq!(target.has_streaming_output(), output);
        assert_eq!(target.has_streaming_blob(), blob);
        let blob_input = input && blob && !event_input;
        assert_eq!(target.has_streaming_blob_input(), blob_input);

        for service in [&REST_JSON_AND_RPC_V2_CBOR, &RPC] {
            let router = rpc_v2_cbor_router(&RouterBuildContext {
                service,
                targets: &[target],
                config: &config.service_config,
                protocol_settings: None,
            })
            .unwrap();
            let request = Request::builder()
                .method("POST")
                .uri("/service/Service/operation/first")
                .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                .body(())
                .unwrap();
            assert_eq!(router.recognizes_streaming_input(&request), input);
            assert_eq!(router.route(&request).unwrap().index(), usize::MAX);
            if service.protocols().len() > 1 {
                assert!(matches!(router.claim(&request), RouteClaim::Claimed));
            } else {
                assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
            }
        }
    }
}

#[test]
fn cbor_claim_requires_the_header_post_and_a_known_service_operation() {
    let options = ProtocolOptions::default();
    let targets = [OperationTarget::new(0, &FIRST)];
    let router = rpc_v2_cbor_router(&RouterBuildContext {
        service: &REST_JSON_AND_RPC_V2_CBOR,
        targets: &targets,
        config: &options.service_config,
        protocol_settings: None,
    })
    .unwrap();
    for (method, path, expected) in [
        (
            "GET",
            "/service/Service/operation/first",
            RoutingErrorKind::MethodNotAllowed,
        ),
        (
            "POST",
            "/service/Other/operation/first",
            RoutingErrorKind::UnknownOperation,
        ),
        (
            "POST",
            "/service/Service/operation/unknown",
            RoutingErrorKind::UnknownOperation,
        ),
        ("POST", "/invalid", RoutingErrorKind::UnknownOperation),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
            .body(())
            .unwrap();
        let RouteClaim::DeferredRejection(error) = router.claim(&request) else {
            panic!("{method} {path} must defer rather than claim");
        };
        assert_eq!(error.kind(), expected);
    }
    let request = Request::builder()
        .method("POST")
        .uri("/service/Service/operation/first")
        .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
        .body(())
        .unwrap();
    assert!(matches!(router.claim(&request), RouteClaim::Claimed));
    for value in [None, Some("rpc-v2-json"), Some("invalid")] {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/service/Service/operation/first");
        if let Some(value) = value {
            builder = builder.header(SMITHY_PROTOCOL_HEADER, value);
        }
        assert!(matches!(router.claim(&builder.body(()).unwrap()), RouteClaim::NoClaim));
    }
}

#[tokio::test]
async fn sole_metadata_protocols_return_native_rejections_without_reading_the_body() {
    for schema in [&RPC, &AWS_JSON_10, &REST_JSON] {
        let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            schema,
            [],
            schema.operations().iter().map(|operation| binding(operation)),
        )
        .build()
        .unwrap();
        let SharedProtocolRouter::Metadata(router) = &app.state.protocols[0].router else {
            unreachable!()
        };
        let error = router
            .route(&Request::builder().method("GET").uri("/unknown").body(()).unwrap())
            .unwrap_err();
        let expected = app.state.protocols[0].protocol.serialize_routing_error(&error);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/unknown")
                    .body(Body::new(http_body_util::StreamBody::new(
                        futures_util::stream::poll_fn(|_| -> Poll<Option<Result<Frame<Bytes>, Error>>> {
                            panic!("unclaimed request read its body")
                        }),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected.status());
        assert_eq!(response.headers(), expected.headers());
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            expected.into_body().collect().await.unwrap().to_bytes()
        );
    }
    let response = service(ProtocolOptions::default())
        .oneshot(
            Request::builder()
                .header("x-body-claim", "defer-head")
                .body(Body::new(http_body_util::StreamBody::new(
                    futures_util::stream::poll_fn(|_| -> Poll<Option<Result<Frame<Bytes>, Error>>> {
                        panic!("head deferred rejection read its body")
                    }),
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[cfg(debug_assertions)]
#[tokio::test]
#[should_panic(expected = "router index belongs to a different operation")]
async fn inconsistent_operation_identity_is_detected_before_handler_dispatch() {
    use crate::schema::routing::service::ProtocolAndRouter;

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
    let mut app = service(ProtocolOptions::default()); // Index zero belongs to SECOND.
    Arc::get_mut(&mut app.state).unwrap().protocols = Box::from([ProtocolAndRouter {
        router: SharedProtocolRouter::new(IncorrectRouter),
        protocol: app.state.protocols[0].protocol.clone(),
    }]);
    let _ = app.oneshot(request("first\n")).await;
}

#[test]
fn routing_response_future_preserves_send_and_unpin() {
    fn assert_unpin<T: Unpin>() {}
    assert_unpin::<MultiProtocolRoutingFuture>();
    crate::test_helpers::assert_send::<MultiProtocolRoutingFuture>();
}

#[tokio::test]
async fn cancelling_pending_handler_readiness_releases_shared_routing_and_handler() {
    #[derive(Clone)]
    struct PendingHandler(Arc<()>);
    impl Service<Request<Body>> for PendingHandler {
        type Response = Response<BoxBody>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Infallible>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            // Keep the selected handler alive while readiness remains pending.
            let _ = &self.0;
            Poll::Pending
        }

        fn call(&mut self, _: Request<Body>) -> Self::Future {
            panic!("handler called before readiness");
        }
    }

    let handler = Arc::new(());
    let handler_lifetime = Arc::downgrade(&handler);
    let mut app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
        &SERVICE,
        [registry()],
        OPERATIONS
            .iter()
            .map(|operation| (*operation, SyncRoute::new(PendingHandler(handler.clone())))),
    )
    .build()
    .unwrap();
    drop(handler);
    let routing_lifetime = Arc::downgrade(&app.state);
    let request = Request::builder()
        .method("POST")
        .uri("/first")
        .header("content-type", "application/json")
        .body(Body::from_bytes(Bytes::from_static(b"first\n")))
        .unwrap();
    let mut future = Box::pin(app.call(request));
    let waker = futures_util::task::noop_waker();
    assert!(future.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());

    // The boxed async implementation retains its service clone across handler readiness.
    drop(app);
    assert!(routing_lifetime.upgrade().is_some());
    assert!(handler_lifetime.upgrade().is_some());
    drop(future);
    assert!(routing_lifetime.upgrade().is_none());
    assert!(handler_lifetime.upgrade().is_none());
}

#[tokio::test]
async fn completed_routing_releases_shared_state_and_handler_before_future_drop() {
    let handler = Arc::new(());
    let handler_lifetime = Arc::downgrade(&handler);
    let mut app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
        &SERVICE,
        [registry()],
        OPERATIONS.iter().map(|operation| {
            let handler = handler.clone();
            let route = SyncRoute::new(tower::service_fn(move |_: Request<Body>| {
                let _ = &handler;
                std::future::ready(Ok::<_, Infallible>(Response::new(crate::body::empty())))
            }));
            (*operation, route)
        }),
    )
    .build()
    .unwrap();
    drop(handler);
    let routing_lifetime = Arc::downgrade(&app.state);
    let mut future = Box::pin(app.call(request("first\n")));
    drop(app);
    assert!(routing_lifetime.upgrade().is_some());
    let waker = futures_util::task::noop_waker();
    assert!(matches!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(_))
    ));
    assert!(routing_lifetime.upgrade().is_none());
    assert!(handler_lifetime.upgrade().is_none());
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
            (
                *op,
                SyncRoute::new(Handler {
                    clones: clones.clone(),
                    ready: AtomicBool::new(false),
                }),
            )
        });
        let mut app =
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(schema, [registry()], bindings)
                .build()
                .unwrap();
        clones.store(0, Ordering::SeqCst);
        let req = || {
            Request::builder()
                .method("POST")
                .uri("/first")
                .header("content-type", "application/json")
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
        let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            schema,
            [registry()],
            [binding(schema.operations()[0])],
        )
        .build()
        .expect("a streaming operation does not fail the build");
        let response = app.oneshot(request("first\n")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    for schema in [&META_INPUT, &META_OUTPUT] {
        assert!(MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            schema,
            [],
            [binding(schema.operations()[0])]
        )
        .build()
        .is_ok());
    }
}

#[tokio::test]
async fn buffered_content_is_reused_and_replacements_and_wrappers_are_read() {
    let original = Bytes::from_static(b"first\npayload");
    let mut app = service_builder(ProtocolOptions::default())
        .layer(tower::layer::layer_fn(move |_inner: SyncRoute<Body>| {
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
        }))
        .build()
        .unwrap();
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

    #[tokio::test]
    async fn deferred_rest_claims_yield_to_definite_claims_and_replay_collected_bodies() {
        static AFTER: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(AFTER)]);
        // A later body protocol definitely selects another operation.
        assert_eq!(
            send(&app, post("/first"), "second\npayload").await,
            (StatusCode::OK, "test#bodyRouting second second\npayload".to_owned()),
        );
        // A later decline leaves the first deferred route intact and replays the body.
        for mode in [
            None,
            Some("deferred-head"),
            Some("deferred-body"),
            Some("defer-head"),
            Some("defer-body"),
        ] {
            let mut request = post("/first");
            if let Some(mode) = mode {
                request = request.header("x-body-claim", mode);
            }
            assert_eq!(
                send(&app, request, "unknown\npayload").await,
                (
                    StatusCode::OK,
                    "aws.protocols#restJson1 first unknown\npayload".to_owned()
                ),
                "{mode:?}",
            );
        }
    }

    #[tokio::test]
    async fn earlier_body_deferred_claims_win_until_a_definite_rest_claim() {
        static BEFORE: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(BEFORE)]);
        for mode in ["deferred-head", "deferred-body"] {
            assert_eq!(
                send(&app, post("/first").header("x-body-claim", mode), "payload").await,
                (StatusCode::OK, "test#bodyRouting first payload".to_owned()),
            );
            assert_eq!(
                send(
                    &app,
                    post("/first")
                        .header("x-body-claim", mode)
                        .header("content-type", "application/json"),
                    "payload",
                )
                .await,
                (StatusCode::OK, "aws.protocols#restJson1 first payload".to_owned()),
            );
        }
    }

    #[tokio::test]
    async fn deferred_rest_claims_preserve_headerless_requests_when_rpc_protocols_are_added() {
        static LABEL: Schema<'static> =
            Schema::new_member(shape_id!("test", "Input", "id"), ShapeType::String, "id", 0).with_http_label();
        static GET: Schema<'static> = Schema::new_struct(shape_id!("test", "Get"), ShapeType::Structure, &[&LABEL])
            .with_original_name("Get")
            .with_http(HttpTrait::new("GET", "/widgets/{id}", Some(200)));
        static DELETE: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Delete"), ShapeType::Structure, &[&LABEL])
                .with_original_name("Delete")
                .with_http(HttpTrait::new("DELETE", "/widgets/{id}", Some(200)));
        static PING: Schema<'static> = Schema::new_struct(shape_id!("test", "PingInput"), ShapeType::Structure, &[])
            .with_original_name("PingInput")
            .with_http(HttpTrait::new("POST", "/ping", Some(200)));
        static GET_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &GET, &UNIT, &[]);
        static DELETE_OP: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &DELETE, &UNIT, &[]);
        static PING_OP: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "Ping"), &PING, &UNIT, &[]);
        static REST: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "restJson1")],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        static CBOR: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("smithy.protocols", "rpcv2Cbor"),
            ],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        static JSON: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "awsJson1_1"),
            ],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        static XML: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "restXml")],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        static XML_CBOR: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restXml"),
                shape_id!("smithy.protocols", "rpcv2Cbor"),
            ],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        static XML_JSON: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restXml"),
                shape_id!("aws.protocols", "awsJson1_1"),
            ],
            &[&GET_OP, &DELETE_OP, &PING_OP],
        );
        for (schema, protocol) in [
            (&REST, "aws.protocols#restJson1"),
            (&CBOR, "aws.protocols#restJson1"),
            (&JSON, "aws.protocols#restJson1"),
            (&XML, "aws.protocols#restXml"),
            (&XML_CBOR, "aws.protocols#restXml"),
            (&XML_JSON, "aws.protocols#restXml"),
        ] {
            let app = app(schema, []);
            for (method, uri, operation) in [
                ("GET", "/widgets/123", "first"),
                ("DELETE", "/widgets/123", "second"),
                ("POST", "/ping", "Ping"),
            ] {
                assert_eq!(
                    send(&app, Request::builder().method(method).uri(uri), "").await,
                    (StatusCode::OK, format!("{protocol} {operation} ")),
                    "{method} {uri} with protocols {:?}",
                    schema.protocols(),
                );
            }
        }
        assert_eq!(
            send(
                &app(&CBOR, []),
                post("/service/Service/operation/first").header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor"),
                "",
            )
            .await,
            (StatusCode::OK, "smithy.protocols#rpcv2Cbor first ".to_owned()),
        );
    }

    #[tokio::test]
    async fn each_builtin_claims_by_its_identifying_characteristics() {
        let app = app(&BUILTINS, []);
        let cases = [
            (
                post("/service/Service/operation/first").header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor"),
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
            (
                post("/first").header("content-type", "application/json; charset=utf-8"),
                "aws.protocols#restJson1 first ",
            ),
            (
                post("/second").header("content-type", "application/xml"),
                "aws.protocols#restXml second ",
            ),
            // No definite protocol identification: the first deferred REST claim wins.
            (post("/first"), "aws.protocols#restJson1 first "),
        ];
        for (request, expected) in cases {
            assert_eq!(send(&app, request, "").await, (StatusCode::OK, expected.to_owned()));
        }
    }

    #[tokio::test]
    async fn aws_json_uri_validation_matches_single_protocol_routing() {
        let multi = app(&BUILTINS, []);
        for (schema, content_type) in [
            (&AWS_JSON_10, "application/x-amz-json-1.0"),
            (&AWS_JSON_11, "application/x-amz-json-1.1"),
        ] {
            let single = app(schema, []);
            for (uri, status) in [
                ("/?times=2", StatusCode::NOT_FOUND),
                ("/?foo=bar&baz=qux", StatusCode::NOT_FOUND),
                ("/", StatusCode::OK),
                ("/?", StatusCode::OK),
            ] {
                let request = || {
                    post(uri)
                        .header("content-type", content_type)
                        .header("x-amz-target", "Service.first")
                        .body(Body::empty())
                        .unwrap()
                };
                let expected = single.clone().oneshot(request()).await.unwrap();
                assert_eq!(expected.status(), status, "{uri}");
                let actual = multi.clone().oneshot(request()).await.unwrap();
                assert_wire_response(actual, expected).await;
            }
        }
    }

    #[tokio::test]
    async fn unclaimed_requests_get_the_generic_unknown_operation_response() {
        let app = app(&BUILTINS, []);
        let unclaimed = [
            // A REST route whose `Content-Type` no REST protocol derives.
            post("/first").header("content-type", "text/plain"),
            // awsJson without its media type.
            post("/").header("x-amz-target", "Service.first"),
            Request::builder().method("GET").uri("/nowhere"),
        ];
        for request in unclaimed {
            let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            // The generic unknown-operation response has no `Content-Type`.
            assert!(response.headers().is_empty());
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "<UnknownOperationException/>\n"
            );
        }
    }

    /// Built-in protocols preserve the generated serde routing status, headers, and empty bodies.
    #[tokio::test]
    async fn routing_rejections_preserve_legacy_responses() {
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
        assert!(response.headers().is_empty());
        assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
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
        assert!(body.is_empty(), "{body}");
    }

    #[tokio::test]
    async fn rpc_v2_cbor_defers_unknown_routes_without_reading_body() {
        let app = app(&BUILTINS, []);
        for path in ["/service/Service/operation/unknown", "/unknown"] {
            // No later protocol claims, so the first deferred rejection (CBOR) wins.
            let response = app
                .clone()
                .oneshot(
                    post(path)
                        .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
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
    async fn a_later_rest_claim_supersedes_cbor_deferred_rejection() {
        let app = app(&BUILTINS, []);
        assert_eq!(
            send(
                &app,
                post("/first")
                    .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                    .header("content-type", "application/json"),
                "payload",
            )
            .await,
            (StatusCode::OK, "aws.protocols#restJson1 first payload".into()),
        );
    }

    #[tokio::test]
    async fn a_later_rest_get_claim_supersedes_cbor_method_rejection() {
        static GET_INPUT: Schema<'static> =
            Schema::new_struct(shape_id!("test", "GetInput"), ShapeType::Structure, &IN_MEMBERS)
                .with_http(HttpTrait::new("GET", "/first", Some(200)));
        static GET_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &GET_INPUT, &UNIT, &[]);
        static BOTH: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                shape_id!("aws.protocols", "restJson1"),
            ],
            &[&GET_OP],
        );
        let (status, body) = send(
            &app(&BOTH, []),
            Request::builder()
                .method("GET")
                .uri("/first")
                .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                .header("content-type", "application/json"),
            "payload",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "aws.protocols#restJson1 first payload");
    }

    #[tokio::test]
    async fn deferred_rejections_use_precision_order_and_body_claims_can_defer() {
        static MIXED: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                shape_id!("test", "bodyRouting"),
                shape_id!("aws.protocols", "restJson1"),
            ],
            OPS,
        );
        for (order, status) in [
            (
                ProtocolOrder::Before("smithy.protocols#rpcv2Cbor"),
                StatusCode::BAD_REQUEST,
            ),
            (
                ProtocolOrder::After("aws.protocols#restJson1"),
                StatusCode::METHOD_NOT_ALLOWED,
            ),
        ] {
            let registry = body_routing(Box::leak(Box::new([order])));
            for mode in ["defer-head", "defer-body"] {
                let body = if mode == "defer-head" {
                    untouchable_body()
                } else {
                    Body::empty()
                };
                let response = app(&MIXED, [registry])
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri("/unknown")
                            .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                            .header("content-type", "application/json")
                            .header("x-body-claim", mode)
                            .body(body)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status, "{order:?} {mode}");
                if status == StatusCode::BAD_REQUEST {
                    assert_eq!(
                        response.into_body().collect().await.unwrap().to_bytes(),
                        "malformed request"
                    );
                }
            }
            // A successful later REST claim overrides all deferred rejections, and
            // a body collected while considering a claim is replayed intact.
            for mode in ["defer-head", "defer-body"] {
                assert_eq!(
                    send(
                        &app(&MIXED, [registry]),
                        post("/first")
                            .header("content-type", "application/json")
                            .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                            .header("x-body-claim", mode),
                        "payload",
                    )
                    .await,
                    (StatusCode::OK, "aws.protocols#restJson1 first payload".into())
                );
            }
        }
    }

    #[tokio::test]
    async fn rpc_v2_cbor_method_mismatch_defaults_to_405_and_can_return_404() {
        for service in [&RPC, &BUILTINS] {
            for (settings, expected) in [
                (r#"{}"#, StatusCode::METHOD_NOT_ALLOWED),
                (
                    r#"{"methodNotAllowedAsNotFound":false}"#,
                    StatusCode::METHOD_NOT_ALLOWED,
                ),
                (r#"{"methodNotAllowedAsNotFound":true}"#, StatusCode::NOT_FOUND),
            ] {
                let options = ProtocolOptions::default().with_protocol_settings(HashMap::from([(
                    shape_id!("smithy.protocols", "rpcv2Cbor"),
                    crate::schema::settings::parse_settings_json(settings.as_bytes()),
                )]));
                let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
                    service,
                    [],
                    service.operations().iter().map(|operation| echo(operation)),
                    options,
                )
                .build()
                .unwrap();
                for method in ["GET", "PUT", "DELETE", "HEAD", "OPTIONS"] {
                    // The method is checked before the path, as in the generated serde router.
                    for path in [
                        "/service/Service/operation/first",
                        "/service/Service/operation/unknown",
                        "/first",
                    ] {
                        let response = app
                            .clone()
                            .oneshot(
                                Request::builder()
                                    .method(method)
                                    .uri(path)
                                    .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                                    .header("content-type", "application/json")
                                    .body(untouchable_body())
                                    .unwrap(),
                            )
                            .await
                            .unwrap();
                        assert_eq!(response.status(), expected, "{settings} {method} {path}");
                        if expected == StatusCode::METHOD_NOT_ALLOWED {
                            assert!(response.headers().is_empty());
                            assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
                        } else {
                            assert_eq!(response.headers()[http::header::CONTENT_TYPE], "application/cbor");
                            assert_eq!(response.headers()[SMITHY_PROTOCOL_HEADER], "rpc-v2-cbor");
                            let body = response.into_body().collect().await.unwrap().to_bytes();
                            assert!(body
                                .windows(b"UnknownOperationException".len())
                                .any(|window| window == b"UnknownOperationException"));
                        }
                    }
                }
                // POST continues to dispatch normally with either setting.
                let (status, _) = send(
                    &app,
                    post("/service/Service/operation/first").header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor"),
                    "",
                )
                .await;
                assert_eq!(status, StatusCode::OK);
            }
        }
    }

    #[tokio::test]
    async fn rpc_v2_cbor_rejects_a_claimed_request_it_cannot_serve() {
        let app = app(&BUILTINS, []);
        // `x-amz-target` is forbidden on rpcv2Cbor, which claimed the request first. It is
        // answered like any other unroutable request: the CBOR unknown-operation `404`.
        let response = app
            .clone()
            .oneshot(
                post("/service/Service/operation/first")
                    .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                    .header("x-amz-target", "Service.first")
                    .header("content-type", "application/x-amz-json-1.0")
                    .body(Body::from_bytes(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/cbor"
        );
        assert!(!response.headers().contains_key(http::header::CONNECTION));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            body.as_ref(),
            b"\xbf\x66__type\x78\x2asmithy.framework#UnknownOperationException\xff".as_slice()
        );
    }

    #[tokio::test]
    async fn rpc_v2_cbor_and_rest_route_blob_inputs() {
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
        let rpc = || post("/service/Service/operation/first").header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor");
        for service in [&BOTH, &CBOR_ONLY] {
            let (status, text) = send(&app(service, []), rpc(), "").await;
            assert_eq!(
                (status, text.as_str()),
                (StatusCode::OK, "smithy.protocols#rpcv2Cbor first ")
            );
        }
        let (status, text) = send(
            &app(&BOTH, []),
            post("/first").header("content-type", "application/octet-stream"),
            "",
        )
        .await;
        assert_eq!(
            (status, text.as_str()),
            (StatusCode::OK, "aws.protocols#restJson1 first ")
        );
    }

    /// Headerless REST requests use protocol priority; a present Content-Type identifies the codec.
    #[tokio::test]
    async fn rest_json_plus_rest_xml_without_content_type_uses_protocol_priority() {
        static ID_LABEL: Schema<'static> =
            Schema::new_member(shape_id!("test", "Widget", "id"), ShapeType::String, "id", 0).with_http_label();
        static WIDGET: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Widget"), ShapeType::Structure, &[&ID_LABEL])
                .with_original_name("Widget")
                .with_http(HttpTrait::new("GET", "/widgets/{id}", Some(200)));
        static GET_WIDGET: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &WIDGET, &UNIT, &[]);
        static PING_INPUT: Schema<'static> = Schema::new_struct(shape_id!("test", "Ping"), ShapeType::Structure, &[])
            .with_http(HttpTrait::new("POST", "/ping", Some(200)));
        static PING: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &PING_INPUT, &UNIT, &[]);
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
            ],
            &[&GET_WIDGET, &PING],
        );
        static REVERSED_SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restXml"),
                shape_id!("aws.protocols", "restJson1"),
            ],
            &[&GET_WIDGET, &PING],
        );
        for schema in [&SERVICE, &REVERSED_SERVICE] {
            for (method, uri, operation) in [("GET", "/widgets/123", "first"), ("POST", "/ping", "second")] {
                assert_eq!(
                    send(&app(schema, []), Request::builder().method(method).uri(uri), "").await,
                    (StatusCode::OK, format!("aws.protocols#restJson1 {operation} ")),
                );
            }
        }
        let app = app(&SERVICE, []);
        for (content_type, expected) in [
            ("application/json", "aws.protocols#restJson1 first "),
            ("application/xml; charset=utf-8", "aws.protocols#restXml first "),
        ] {
            assert_eq!(
                send(
                    &app,
                    Request::builder()
                        .method("GET")
                        .uri("/widgets/123")
                        .header("content-type", content_type),
                    "",
                )
                .await,
                (StatusCode::OK, expected.to_owned()),
            );
        }
        // Unmatched routes retain the generic XML response without reading the request body.
        for method in ["GET", "POST"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/no-such-route")
                        .body(untouchable_body())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(response.headers().is_empty());
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "<UnknownOperationException/>\n",
            );
        }
        assert_eq!(
            send(
                &app,
                Request::builder()
                    .method("GET")
                    .uri("/widgets/123")
                    .header("content-type", "text/plain"),
                "",
            )
            .await
            .0,
            StatusCode::NOT_FOUND,
        );
    }

    #[tokio::test]
    async fn deferred_rest_protocol_claims_yield_to_later_definite_claims() {
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
                shape_id!("test", "bodyRouting"),
            ],
            OPERATIONS,
        );
        static AFTER: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restXml")];
        assert_eq!(
            send(
                &app(&SERVICE, [body_routing(AFTER)]),
                post("/first").header("x-body-claim", "known-route"),
                "payload",
            )
            .await,
            (StatusCode::OK, "test#bodyRouting first payload".to_owned()),
        );
    }

    /// Both buffered and streaming blob payloads derive application/octet-stream.
    #[tokio::test]
    async fn sole_rest_routes_blob_payloads_without_content_type_admission() {
        static DATA: Schema<'static> =
            Schema::new_member(shape_id!("test", "Upload", "data"), ShapeType::Blob, "data", 0)
                .with_streaming()
                .with_http_payload();
        static UPLOAD: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Upload"), ShapeType::Structure, &[&DATA])
                .with_original_name("Upload")
                .with_http(HttpTrait::new("POST", "/first", Some(200)));
        static STREAMING: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &UPLOAD, &UNIT, &[]);
        static RAW: Schema<'static> =
            Schema::new_member(shape_id!("test", "Put", "data"), ShapeType::Blob, "data", 0).with_http_payload();
        static PUT: Schema<'static> = Schema::new_struct(shape_id!("test", "Put"), ShapeType::Structure, &[&RAW])
            .with_original_name("Put")
            .with_http(HttpTrait::new("POST", "/second", Some(200)));
        static BUFFERED: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &PUT, &UNIT, &[]);
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "restJson1")],
            &[&STREAMING, &BUFFERED],
        );
        let app = app(&SERVICE, []);
        for (uri, operation) in [("/first", "first"), ("/second", "second")] {
            assert_eq!(
                send(&app, post(uri).header("content-type", "application/octet-stream"), "").await,
                (StatusCode::OK, format!("aws.protocols#restJson1 {operation} ")),
            );
            for request in [
                post(uri),
                post(uri).header("content-type", "video/mp4"),
                post(uri).header("content-type", "application/json"),
            ] {
                assert_eq!(send(&app, request, "").await.0, StatusCode::OK);
            }
        }
    }

    /// A modeled Content-Type header permits a custom value; an empty input uses the
    /// protocol default during strict arbitration. A sole protocol routes either input.
    #[tokio::test]
    async fn sole_rest_routes_custom_headers_and_empty_inputs_without_content_type_admission() {
        static CONTENT_TYPE: Schema<'static> = Schema::new_member(
            shape_id!("test", "Upload", "contentType"),
            ShapeType::String,
            "contentType",
            0,
        )
        .with_http_header("Content-Type");
        static DATA: Schema<'static> =
            Schema::new_member(shape_id!("test", "Upload", "data"), ShapeType::Blob, "data", 1).with_http_payload();
        static UPLOAD: Schema<'static> = Schema::new_struct(
            shape_id!("test", "Upload"),
            ShapeType::Structure,
            &[&CONTENT_TYPE, &DATA],
        )
        .with_original_name("Upload")
        .with_http(HttpTrait::new("POST", "/first", Some(200)));
        static WITH_HEADER: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &UPLOAD, &UNIT, &[]);
        static EMPTY: Schema<'static> = Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[])
            .with_original_name("Empty")
            .with_http(HttpTrait::new("POST", "/second", Some(200)));
        static MODELED_EMPTY: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &EMPTY, &UNIT, &[]);
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "restJson1")],
            &[&WITH_HEADER, &MODELED_EMPTY],
        );
        let app = app(&SERVICE, []);
        let (status, text) = send(&app, post("/first").header("content-type", "video/mp4"), "").await;
        assert_eq!(
            (status, text.as_str()),
            (StatusCode::OK, "aws.protocols#restJson1 first ")
        );
        let (status, text) = send(&app, post("/second").header("content-type", "application/json"), "").await;
        assert_eq!(
            (status, text.as_str()),
            (StatusCode::OK, "aws.protocols#restJson1 second ")
        );
        for request in [
            post("/first"),
            post("/second"),
            post("/second").header("content-type", "text/plain"),
        ] {
            assert_eq!(send(&app, request, "").await.0, StatusCode::OK);
        }
    }

    /// Event streams retain their own derived content type for identification.
    #[tokio::test]
    async fn sole_rest_event_stream_claims_and_recognition_use_native_routing() {
        static REST_STREAM: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[shape_id!("aws.protocols", "restJson1")],
            &[&STREAM_OP],
        );
        let app = app(&REST_STREAM, []);
        let SharedProtocolRouter::Metadata(router) = &app.state.protocols[0].router else {
            unreachable!()
        };
        for builder in [
            post("/stream"),
            post("/stream").header("content-type", "application/json"),
        ] {
            let request = builder.body(()).unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
            assert!(router.recognizes_streaming_input(&request));
        }
        let request = post("/stream")
            .header("content-type", "application/vnd.amazon.eventstream")
            .body(())
            .unwrap();
        assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
        assert!(router.recognizes_streaming_input(&request));
        // A sole REST protocol dispatches event streams without Content-Type admission.
        let (status, _) = send(&app, post("/stream"), "").await;
        assert_eq!(status, StatusCode::OK);
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

    fn streaming_builder() -> MultiProtocolRoutingServiceBuilder {
        static BEFORE: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#awsJson1_1")];
        MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &STREAM_SERVICE,
            [body_routing(BEFORE)],
            STREAM_SERVICE.operations().iter().map(|operation| {
                (
                    *operation,
                    SyncRoute::new(tower::service_fn(move |request: Request<Body>| async move {
                        let selected = SelectedOperation::get_from_request(&request).unwrap().clone();
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
    }
    fn streaming_app() -> MultiProtocolRoutingService {
        streaming_builder().build().unwrap()
    }

    #[tokio::test]
    async fn aws_json_streaming_input_skips_body_claimants_without_polling() {
        for content_type in [
            "application/x-amz-json-1.1; charset=UTF-8",
            "application/vnd.amazon.eventstream",
            "application/vnd.amazon.eventstream; charset=UTF-8",
        ] {
            let app = streaming_app();
            assert_eq!(app.state.metadata_routers.as_deref(), Some(&[1][..]));
            let response = app
                .oneshot(
                    post("/")
                        .header("content-type", content_type)
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
    }

    #[tokio::test]
    async fn aws_json_event_stream_claims_only_matching_input_streams_and_uses_protocol_priority() {
        static BLOB_MEMBER: Schema<'static> =
            Schema::new_member(shape_id!("test", "BlobInput", "data"), ShapeType::Blob, "data", 0).with_streaming();
        static BLOB_INPUT: Schema<'static> =
            Schema::new_struct(shape_id!("test", "BlobInput"), ShapeType::Structure, &[&BLOB_MEMBER]);
        static BLOB_OP: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "blob"), &BLOB_INPUT, &UNIT, &[]);
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "awsJson1_1"),
                shape_id!("aws.protocols", "awsJson1_0"),
            ],
            &[&STREAM_OP, &SECOND_OP, &OUTPUT_OP, &BLOB_OP],
        );
        let app = app(&SERVICE, []);
        for route in app.state.protocols.iter() {
            let SharedProtocolRouter::Metadata(router) = &route.router else {
                unreachable!()
            };
            for target in ["Service.second", "Service.output", "Service.unknown", "Service.blob"] {
                let request = post("/")
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .header("x-amz-target", target)
                    .body(())
                    .unwrap();
                assert!(matches!(router.claim(&request), RouteClaim::NoClaim));
                assert!(!router.recognizes_streaming_input(&request));
            }
            let request = post("/")
                .header("content-type", "application/vnd.amazon.eventstream")
                .header("x-amz-target", "Service.first")
                .body(())
                .unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
            assert!(router.recognizes_streaming_input(&request));
            for builder in [post("/other"), Request::builder().method("GET").uri("/")] {
                let request = builder
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .header("x-amz-target", "Service.first")
                    .body(())
                    .unwrap();
                assert!(matches!(router.claim(&request), RouteClaim::NoClaim));
                assert!(!router.recognizes_streaming_input(&request));
            }
        }
        assert_eq!(
            send(
                &app,
                post("/")
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .header("x-amz-target", "Service.first"),
                ""
            )
            .await,
            (StatusCode::OK, "aws.protocols#awsJson1_0 first ".into())
        );
    }

    #[tokio::test]
    async fn media_type_alone_does_not_skip_body_claimants() {
        static BEFORE: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson1")];
        let app = app(&WITH_BODY_ROUTING, [body_routing(BEFORE)]);
        assert!(app.state.metadata_routers.is_none());
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
        assert_eq!(all_app.state.metadata_routers.as_ref().unwrap().len(), 5);
        for route in all_app.state.protocols.iter() {
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
                    post("/service/Service/operation/first").header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
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
        assert!(output.state.metadata_routers.is_none());
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
    type AdvisoryApp = (
        MultiProtocolRoutingService,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::Mutex<Vec<usize>>>,
    );

    fn advisory_app(reject: bool, streaming: bool) -> AdvisoryApp {
        advisory_app_with_layer(reject, streaming, tower::layer::util::Identity::new())
    }
    fn advisory_app_with_layer<L>(reject: bool, streaming: bool, layer: L) -> AdvisoryApp
    where
        L: tower::Layer<SyncRoute<Body>>,
        L::Service:
            Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        let mut app = streaming_builder().layer(layer).build().unwrap();
        let checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let claims = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let state = Arc::get_mut(&mut app.state).unwrap();
        let mut routes = std::mem::take(&mut state.protocols).into_vec();
        routes[1].router = SharedProtocolRouter::new(AdvisoryRouter {
            checks: checks.clone(),
            claims: claims.clone(),
            reject,
            streaming,
        });
        let tracked_router = |index| {
            SharedProtocolRouter::new_body_routed(TrackedBodyRouter {
                router: BodyRouter {
                    targets: if index == 0 {
                        vec![]
                    } else {
                        vec![OperationTarget::new(1, &SECOND_OP)]
                    },
                },
                calls: calls.clone(),
                index,
            })
        };
        routes[0].router = tracked_router(0);
        let second_body = super::service::ProtocolAndRouter {
            router: tracked_router(2),
            protocol: routes[0].protocol.clone(),
        };
        routes.push(second_body);
        state.protocols = routes.into();
        (app, checks, claims, calls)
    }

    #[tokio::test]
    async fn multi_protocol_claims_stay_lazy_and_unpolled_cancellation_releases_request() {
        struct DropMarker(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let (mut app, checks, claims, calls) = advisory_app(false, true);
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = DropMarker(dropped.clone());
        let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            move |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> {
                let _ = &marker;
                panic!("unpolled routing future read its request body");
            },
        )));
        let future = app.call(post("/").body(body).unwrap());

        // Creating the old boxed async future did not execute its claim walk.
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
        drop(future);
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(calls.lock().unwrap().is_empty());

        let mut future = Box::pin(app.call(post("/").body(untouchable_body()).unwrap()));
        assert!(calls.lock().unwrap().is_empty());
        let waker = futures_util::task::noop_waker();
        let response = match future.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Ready(Ok(response)) => response,
            _ => panic!("streaming-aware routing should reject synchronously on first poll"),
        };
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(*calls.lock().unwrap(), vec![0, 2]);
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn streaming_skips_body_claimants_and_ordinary_requests_preserve_order_and_replay() {
        for streaming in [true, false] {
            let (app, checks, claims, calls) = advisory_app(false, streaming);
            if streaming {
                let response = app.oneshot(post("/").body(untouchable_body()).unwrap()).await.unwrap();
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
                assert_eq!(
                    response.into_body().collect().await.unwrap().to_bytes(),
                    "<UnknownOperationException/>\n"
                );
                assert_eq!(*calls.lock().unwrap(), vec![0, 2]);
                assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
                assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 1);
                continue;
            }
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
    async fn declined_body_claim_preserves_extensions_and_trailers_for_the_next_router() {
        #[derive(Clone, Debug, PartialEq)]
        struct Marker(&'static str);

        let (mut app, _, _, _) = advisory_app_with_layer(
            false,
            false,
            tower::layer::layer_fn(|_: SyncRoute<Body>| {
                tower::service_fn(|request: Request<Body>| async move {
                    assert_eq!(request.extensions().get::<Marker>(), Some(&Marker("retained")));
                    assert_eq!(request.headers()["x-original"], "retained");
                    Ok::<_, Infallible>(Response::new(crate::body::boxed(request.into_body())))
                })
            }),
        );
        let mut trailers = HeaderMap::new();
        trailers.insert("checksum", HeaderValue::from_static("abc"));
        let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transport_polls = polls.clone();
        let mut frames = vec![
            Ok::<_, Error>(Frame::data(Bytes::from_static(b"second\npayload"))),
            Ok(Frame::trailers(trailers.clone())),
        ]
        .into_iter();
        let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
            move |_| {
                let count = transport_polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert!(count < 3, "transport polled after collection finished");
                Poll::Ready(frames.next())
            },
        )));
        let request = post("/")
            .header("x-original", "retained")
            .extension(Marker("retained"))
            .body(body)
            .unwrap();
        let future: MultiProtocolRoutingFuture = app.call(request);
        let response = future.await.unwrap();
        let collected = response.into_body().collect().await.unwrap();
        assert_eq!(collected.trailers(), Some(&trailers));
        assert_eq!(collected.to_bytes(), "second\npayload");
        assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn metadata_rejection_follows_skipped_body_claim() {
        let (app, checks, claims, calls) = advisory_app(true, true);
        let response = app.oneshot(post("/").body(untouchable_body()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(*calls.lock().unwrap(), vec![0]);
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn head_claims_keep_priority_over_streaming_recognition() {
        for mode in ["known-route", "envelope"] {
            let app = streaming_app();
            let response = app
                .oneshot(
                    post("/")
                        .header("content-type", "application/x-amz-json-1.1")
                        .header("x-amz-target", "Service.first")
                        .header("x-body-claim", mode)
                        .body(Body::from("second\npayload"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "test#bodyRouting second second\npayload"
            );
        }

        // A terminal head claim requires no streaming recognition, even when routing rejects.
        let (app, checks, claims, calls) = advisory_app(false, true);
        let response = app
            .oneshot(
                post("/")
                    .header("x-body-claim", "envelope")
                    .body(Body::from("second\npayload"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(claims.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(*calls.lock().unwrap(), vec![0]);
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
    /// response.
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
}

#[test]
fn rest_router_defers_routing_rejections_for_configured_content_type_aliases() {
    let targets = [OperationTarget::new(0, &FIRST)];
    for aliases in [&["text/xml"][..], &[][..]] {
        let router = rest_router(&strict_context(&targets), "application/xml", aliases).unwrap();
        for (method, path, expected) in [
            ("POST", "/unknown", RoutingErrorKind::UnknownOperation),
            ("GET", "/first", RoutingErrorKind::MethodNotAllowed),
        ] {
            for content_type in [
                Some("application/xml"),
                Some("application/xml; charset=utf-8"),
                Some("text/xml"),
                Some("text/xml; charset=utf-8"),
                Some("application/json"),
                Some("invalid"),
                None,
            ] {
                let mut request = Request::builder().method(method).uri(path);
                if let Some(content_type) = content_type {
                    request = request.header("content-type", content_type);
                }
                let claim = router.claim(&request.body(()).unwrap());
                match content_type {
                    Some("application/xml" | "application/xml; charset=utf-8") => {
                        let RouteClaim::DeferredRejection(error) = claim else {
                            panic!("{method} {path} with {content_type:?} must defer a rejection");
                        };
                        assert_eq!(error.kind(), expected);
                    }
                    Some("text/xml" | "text/xml; charset=utf-8") if !aliases.is_empty() => {
                        let RouteClaim::DeferredRejection(error) = claim else {
                            panic!("{method} {path} with {content_type:?} must defer a rejection");
                        };
                        assert_eq!(error.kind(), expected);
                    }
                    _ => assert!(matches!(claim, RouteClaim::NoClaim)),
                }
            }
        }
    }
}

#[test]
fn rest_router_claims_the_codec_content_type_and_its_aliases() {
    static NOTE: Schema<'static> =
        Schema::new_member(shape_id!("test", "noteInput", "note"), ShapeType::String, "note", 0);
    static NOTE_INPUT: Schema<'static> = Schema::new_struct(
        shape_id!("test", "noteInput"),
        ShapeType::Structure,
        &[&NOTE],
    )
    .with_http(HttpTrait::new("POST", "/note", Some(200)));
    static NOTE_OPERATION: OperationSchema<'static> =
        OperationSchema::new(shape_id!("test", "note"), &NOTE_INPUT, &UNIT, &[]);

    let targets = [OperationTarget::new(0, &NOTE_OPERATION)];
    let claims = |aliases: &'static [&'static str], content_type: &str| {
        let router = rest_router(&strict_context(&targets), "application/xml", aliases).unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/note")
            .header("content-type", content_type)
            .header("content-length", "10")
            .body(())
            .unwrap();
        matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_))
    };

    for content_type in ["application/xml", "text/xml", "text/xml; charset=utf-8"] {
        assert!(claims(&["text/xml"], content_type), "{content_type}");
    }
    // Another protocol's media type, or one nothing accepts, is left for the next protocol.
    for content_type in ["application/json", "text/plain"] {
        assert!(!claims(&["text/xml"], content_type), "{content_type}");
    }
    // Without the alias only the codec's own media type is claimed.
    assert!(claims(&[], "application/xml"));
    assert!(!claims(&[], "text/xml"));
}

#[test]
fn rest_payload_claims_respect_modeled_media_types_and_shape_defaults() {
    static TEXT: Schema<'static> =
        Schema::new_member(shape_id!("test", "Text", "value"), ShapeType::String, "value", 0).with_http_payload();
    static MEDIA: Schema<'static> =
        Schema::new_member(shape_id!("test", "Media", "value"), ShapeType::Blob, "value", 0)
            .with_http_payload()
            .with_streaming()
            .with_media_type("video/mp4");
    static DOCUMENT: Schema<'static> =
        Schema::new_member(shape_id!("test", "Document", "value"), ShapeType::Structure, "value", 0)
            .with_http_payload();
    static TEXT_INPUT: Schema<'static> = Schema::new_struct(shape_id!("test", "Text"), ShapeType::Structure, &[&TEXT])
        .with_http(HttpTrait::new("POST", "/text", Some(200)));
    static MEDIA_INPUT: Schema<'static> = Schema::new_struct(
        shape_id!("test", "Media"),
        ShapeType::Structure,
        &[&MEDIA],
    )
    .with_http(HttpTrait::new("POST", "/media", Some(200)));
    static DOCUMENT_INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Document"), ShapeType::Structure, &[&DOCUMENT])
            .with_http(HttpTrait::new("POST", "/document", Some(200)));
    static TEXT_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &TEXT_INPUT, &UNIT, &[]);
    static MEDIA_OP: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &MEDIA_INPUT, &UNIT, &[]);
    static DOCUMENT_OP: OperationSchema<'static> =
        OperationSchema::new(shape_id!("test", "document"), &DOCUMENT_INPUT, &UNIT, &[]);
    let targets = [
        OperationTarget::new(0, &TEXT_OP),
        OperationTarget::new(1, &MEDIA_OP),
        OperationTarget::new(2, &DOCUMENT_OP),
    ];
    for codec_content_type in ["application/json", "application/xml"] {
        let router = rest_router(&strict_context(&targets), codec_content_type, &[]).unwrap();
        for (uri, expected) in [
            ("/text", "text/plain"),
            ("/media", "video/mp4"),
            ("/document", codec_content_type),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", expected)
                .body(())
                .unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
            let request = Request::builder().method("POST").uri(uri).body(()).unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::NoClaim));
            let request = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/unknown")
                .body(())
                .unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::NoClaim));
        }
    }
}

#[test]
fn rest_defers_claims_for_headerless_synthetic_empty_inputs() {
    let targets = [OperationTarget::new(0, &FIRST)];
    for codec_content_type in ["application/json", "application/xml"] {
        let router = rest_router(&strict_context(&targets), codec_content_type, &[]).unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/first")
            .header("content-type", codec_content_type)
            .body(())
            .unwrap();
        assert!(matches!(router.claim(&request), RouteClaim::ClaimedWithRoute(_)));
        let request = Request::builder().method("POST").uri("/first").body(()).unwrap();
        assert!(matches!(router.claim(&request), RouteClaim::DeferredClaim(_)));
    }
}

#[tokio::test]
async fn headerless_rest_claims_depend_on_required_body_members_and_non_null_defaults() {
    use aws_smithy_schema::{AnnotationTrait, DocumentTrait, TraitMap};
    use aws_smithy_types::Document;
    use std::sync::LazyLock;

    static REQUIRED: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut traits = TraitMap::new();
        traits.insert(Box::new(AnnotationTrait::new(shape_id!("smithy.api", "required"))));
        traits
    });
    static DEFAULTED: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut traits = TraitMap::new();
        traits.insert(Box::new(AnnotationTrait::new(shape_id!("smithy.api", "required"))));
        traits.insert(Box::new(DocumentTrait::new(
            shape_id!("smithy.api", "default"),
            Document::String("fallback".into()),
        )));
        traits
    });
    static NULL_DEFAULT: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut traits = TraitMap::new();
        traits.insert(Box::new(AnnotationTrait::new(shape_id!("smithy.api", "required"))));
        traits.insert(Box::new(DocumentTrait::new(
            shape_id!("smithy.api", "default"),
            Document::Null,
        )));
        traits
    });
    static REQUIRED_VALUE: Schema<'static> =
        Schema::new_member(shape_id!("test", "Required", "value"), ShapeType::String, "value", 0)
            .with_traits(&REQUIRED);
    static DEFAULT_VALUE: Schema<'static> =
        Schema::new_member(shape_id!("test", "Defaulted", "value"), ShapeType::String, "value", 0)
            .with_traits(&DEFAULTED);
    static NULL_VALUE: Schema<'static> =
        Schema::new_member(shape_id!("test", "Null", "value"), ShapeType::String, "value", 0)
            .with_traits(&NULL_DEFAULT);
    static LABEL: Schema<'static> = Schema::new_member(shape_id!("test", "Bound", "id"), ShapeType::String, "id", 0)
        .with_traits(&REQUIRED)
        .with_http_label();
    static HEADER: Schema<'static> =
        Schema::new_member(shape_id!("test", "Bound", "header"), ShapeType::String, "header", 1)
            .with_traits(&REQUIRED)
            .with_http_header("x-required");
    static QUERY: Schema<'static> =
        Schema::new_member(shape_id!("test", "Bound", "query"), ShapeType::String, "query", 2)
            .with_traits(&REQUIRED)
            .with_http_query("required");
    static REQUIRED_INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Required"), ShapeType::Structure, &[&REQUIRED_VALUE])
            .with_http(HttpTrait::new("POST", "/required", Some(200)));
    static DEFAULT_INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Defaulted"), ShapeType::Structure, &[&DEFAULT_VALUE])
            .with_http(HttpTrait::new("POST", "/defaulted", Some(200)));
    static NULL_INPUT: Schema<'static> = Schema::new_struct(
        shape_id!("test", "Null"),
        ShapeType::Structure,
        &[&NULL_VALUE],
    )
    .with_http(HttpTrait::new("POST", "/null", Some(200)));
    static BOUND_INPUT: Schema<'static> = Schema::new_struct(
        shape_id!("test", "Bound"),
        ShapeType::Structure,
        &[&LABEL, &HEADER, &QUERY],
    )
    .with_http(HttpTrait::new("GET", "/bound/{id}", Some(200)));
    static REQUIRED_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &REQUIRED_INPUT, &UNIT, &[]);
    static DEFAULT_OP: OperationSchema<'static> = OperationSchema::new(SECOND_ID, &DEFAULT_INPUT, &UNIT, &[]);
    static NULL_OP: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "null"), &NULL_INPUT, &UNIT, &[]);
    static BOUND_OP: OperationSchema<'static> =
        OperationSchema::new(shape_id!("test", "bound"), &BOUND_INPUT, &UNIT, &[]);
    let targets = [
        OperationTarget::new(0, &REQUIRED_OP),
        OperationTarget::new(1, &DEFAULT_OP),
        OperationTarget::new(2, &NULL_OP),
        OperationTarget::new(3, &BOUND_OP),
    ];
    for codec in ["application/json", "application/xml"] {
        let router = rest_router(&strict_context(&targets), codec, &[]).unwrap();
        for uri in ["/required", "/null"] {
            let request = Request::builder().method("POST").uri(uri).body(()).unwrap();
            let RouteClaim::DeferredRejection(error) = router.claim(&request) else {
                panic!("required body input must defer a rejection");
            };
            assert_eq!(error.kind(), RoutingErrorKind::MissingContentType);
            assert_eq!(error.expected_content_type().unwrap(), codec);
        }
        for (method, uri) in [("POST", "/defaulted"), ("GET", "/bound/123")] {
            let request = Request::builder().method(method).uri(uri).body(()).unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::DeferredClaim(_)));
        }
    }
    static REST_JSON: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        &[&REQUIRED_OP],
    );
    static REST_XML: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restXml"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        &[&REQUIRED_OP],
    );
    static BOTH_REST: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("aws.protocols", "restXml"),
        ],
        &[&REQUIRED_OP],
    );
    let mut context = strict_context(&targets);
    context.service = &BOTH_REST;
    for codec in ["application/json", "application/xml"] {
        let router = rest_router(&context, codec, &[]).unwrap();
        for uri in ["/required", "/null"] {
            let request = Request::builder().method("POST").uri(uri).body(()).unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::DeferredRejection(_)));
        }
        for (method, uri) in [("POST", "/defaulted"), ("GET", "/bound/123")] {
            let request = Request::builder().method(method).uri(uri).body(()).unwrap();
            assert!(matches!(router.claim(&request), RouteClaim::DeferredClaim(_)));
        }
    }
    for (schema, codec) in [
        (&REST_JSON, "application/json"),
        (&REST_XML, "application/xml"),
        (&BOTH_REST, "application/json"),
    ] {
        let app = app(schema, []);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/required")
                    .body(Body::new(http_body_util::StreamBody::new(
                        futures_util::stream::poll_fn(|_| -> Poll<Option<Result<Frame<Bytes>, Error>>> {
                            panic!("schema-based deferred rejection must not read the body")
                        }),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let protocol = app
            .state
            .protocols
            .iter()
            .find(|entry| {
                entry
                    .protocol
                    .protocol_id()
                    .as_str()
                    .ends_with(if codec == "application/json" {
                        "restJson1"
                    } else {
                        "restXml"
                    })
            })
            .unwrap();
        let expected = protocol.protocol.serialize_rejection(
            crate::rejection::MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: Some(codec.parse().unwrap()),
                found_mime: None,
            }
            .into(),
        );
        assert_wire_response(response, expected).await;
    }
    // Even a known REST rejection yields to a later definite body claim.
    static WITH_BODY: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("aws.protocols", "restJson1"),
            shape_id!("test", "bodyRouting"),
        ],
        &[&REQUIRED_OP],
    );
    static AFTER: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restJson1")];
    let response = app(&WITH_BODY, [body_routing(AFTER)])
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/required")
                .header("x-body-claim", "known-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "test#bodyRouting first "
    );
}

#[test]
fn an_invalid_modeled_media_type_fails_the_router_build() {
    // Smithy does not validate `@mediaType` syntax; the claim derivation parses it, and an
    // unparseable value must fail the build loudly instead of panicking.
    static BAD_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "Bad", "data"), ShapeType::Blob, "data", 0)
            .with_http_payload()
            .with_media_type("not a mime type");
    static BAD_INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Bad"), ShapeType::Structure, &[&BAD_PAYLOAD])
            .with_original_name("Bad")
            .with_http(HttpTrait::new("POST", "/bad", Some(200)));
    static BAD_OP: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &BAD_INPUT, &UNIT, &[]);
    static BAD_SERVICE: ServiceSchema<'static> =
        ServiceSchema::new(SERVICE_ID, None, &[shape_id!("aws.protocols", "restJson1")], &[&BAD_OP]);
    let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
        &BAD_SERVICE,
        [],
        [(
            &BAD_OP,
            SyncRoute::<Body>::new(tower::service_fn(|_request: Request<Body>| async move {
                Ok::<_, Infallible>(Response::new(crate::body::boxed(crate::body::Body::empty())))
            })),
        )],
    )
    .build()
    .unwrap_err();
    assert!(matches!(error, RouterBuildError::Configuration(_)), "{error}");
}

#[tokio::test]
async fn cbor_blob_input_routing_selects_cbor_without_body_reads_or_rest_fallback() {
    use aws_smithy_types::Document;

    static DATA: Schema<'static> =
        Schema::new_member(shape_id!("test", "UploadInput", "data"), ShapeType::Blob, "data", 0)
            .with_streaming()
            .with_http_payload();
    static INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "UploadInput"), ShapeType::Structure, &[&DATA])
            .with_original_name("UploadInput")
            .with_http(HttpTrait::new("POST", "/service/Service/operation/upload", Some(200)));
    static UPLOAD: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "upload"), &INPUT, &UNIT, &[]);
    static CBOR: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[shape_id!("smithy.protocols", "rpcv2Cbor")],
        &[&UPLOAD],
    );
    static CBOR_REST: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("smithy.protocols", "rpcv2Cbor"),
            shape_id!("aws.protocols", "restJson1"),
        ],
        &[&UPLOAD],
    );
    static BODY_CBOR: ServiceSchema<'static> = ServiceSchema::new(
        SERVICE_ID,
        None,
        &[
            shape_id!("test", "bodyRouting"),
            shape_id!("smithy.protocols", "rpcv2Cbor"),
        ],
        &[&UPLOAD],
    );
    static BEFORE_CBOR: &[ProtocolOrder] = &[ProtocolOrder::Before("smithy.protocols#rpcv2Cbor")];

    for schema in [&CBOR, &CBOR_REST, &BODY_CBOR] {
        for capitalize in [false, true] {
            let options = ProtocolOptions::default().with_protocol_settings(HashMap::from([(
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                Document::Object(HashMap::from([("capitalizeRoutes".into(), Document::Bool(capitalize))])),
            )]));
            let handler = SyncRoute::new(tower::service_fn(
                |request: Request<Body>| -> std::future::Ready<Result<Response<BoxBody>, Infallible>> {
                    let selected = SelectedOperation::get_from_request(&request).unwrap();
                    assert_eq!(selected.protocol().protocol_id().as_str(), "smithy.protocols#rpcv2Cbor");
                    assert_eq!(selected.operation().shape_id(), UPLOAD.shape_id());
                    std::future::ready(Ok(Response::new(crate::body::empty())))
                },
            ));
            let app = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
                schema,
                [body_routing(BEFORE_CBOR)],
                [(&UPLOAD, handler)],
                options,
            )
            .build()
            .unwrap();
            for prefix in ["", "/prefix"] {
                let name = if capitalize { "Upload" } else { "upload" };
                let body = Body::new(http_body_util::StreamBody::new(futures_util::stream::poll_fn(
                    |_| -> Poll<Option<Result<Frame<Bytes>, Error>>> {
                        panic!("routing must leave CBOR blob collection to the upgrade")
                    },
                )));
                let request = Request::builder()
                    .method("POST")
                    .uri(format!("{prefix}/service/Service/operation/{name}"))
                    .header(SMITHY_PROTOCOL_HEADER, "rpc-v2-cbor")
                    .header("content-type", "application/octet-stream")
                    .body(body)
                    .unwrap();
                assert_wire_response(
                    app.clone().oneshot(request).await.unwrap(),
                    Response::new(crate::body::empty()),
                )
                .await;
            }
        }
    }
}

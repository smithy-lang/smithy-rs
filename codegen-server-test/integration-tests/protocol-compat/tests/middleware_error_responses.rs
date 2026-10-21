/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Uses one middleware abstraction to produce runtime and modeled errors for legacy
//! and schema services. It compares the responses with handler-generated errors and
//! checks that schema middleware uses the selected protocol and preserves modeled
//! error extensions for each request.
//!
//! The same middleware bounds accept generated serde errors and schema modeled errors.
#[path = "support/extractor_rejections.rs"]
mod harness;

use std::{
    convert::Infallible,
    future::{ready, Ready},
    marker::PhantomData,
    task::{Context, Poll},
};

use aws_smithy_http_server::{
    body::Body,
    extension::ModeledErrorExtension,
    operation::OperationShape,
    plugin::{HttpMarker, Plugin},
    response::{IntoResponse, IntoResponseWithRequest, Response},
    runtime_error::InternalFailureException,
    service::ServiceShape,
};
use harness::{observed_runtime_rejection, request, snapshot, Reply, Wire, MESSAGE};
use http::Request;
use tower::{Service, ServiceExt};

trait MakeError {
    fn make_error() -> Self;
}

#[derive(Clone)]
struct RejectPlugin;

impl HttpMarker for RejectPlugin {}

impl<Ser, Op, S, SerdeMode> Plugin<Ser, Op, S, SerdeMode> for RejectPlugin
where
    Ser: ServiceShape,
    Op: OperationShape,
    Op::Error: MakeError + IntoResponseWithRequest<Ser::Protocol, SerdeMode>,
    InternalFailureException: IntoResponseWithRequest<Ser::Protocol, SerdeMode>,
{
    type Output = RejectMiddleware<Ser::Protocol, Op::Error, S, SerdeMode>;

    fn apply(&self, inner: S) -> Self::Output {
        RejectMiddleware {
            inner,
            marker: PhantomData,
        }
    }
}

struct RejectMiddleware<P, E, S, SerdeMode> {
    inner: S,
    marker: PhantomData<fn() -> (P, E, SerdeMode)>,
}

impl<P, E, S: Clone, SerdeMode> Clone for RejectMiddleware<P, E, S, SerdeMode> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            marker: PhantomData,
        }
    }
}

impl<P, E, S, SerdeMode> Service<Request<Body>> for RejectMiddleware<P, E, S, SerdeMode>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible>,
    E: MakeError + IntoResponseWithRequest<P, SerdeMode>,
    InternalFailureException: IntoResponseWithRequest<P, SerdeMode>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Ready<Result<Response, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let response = if request.headers()["x-rejection"] == "internal" {
            InternalFailureException.into_response_with_request(&request)
        } else {
            E::make_error().into_response_with_request(&request)
        };
        ready(Ok(response))
    }
}

async fn send<S>(service: S, request: Request<Body>, modeled: bool) -> Reply
where
    S: Service<Request<Body>, Response = Response, Error = Infallible>,
{
    let response = service.oneshot(request).await.unwrap();
    let extension = response
        .extensions()
        .get::<ModeledErrorExtension>()
        .map(|error| **error);
    assert_eq!(extension, modeled.then_some("ValidationException"));
    snapshot(response).await
}

macro_rules! make_error {
    ($sdk:ident) => {
        impl MakeError for $sdk::error::GetWidgetError {
            fn make_error() -> Self {
                $sdk::error::ValidationException::builder()
                    .message(MESSAGE.to_owned())
                    .build()
                    .unwrap()
                    .into()
            }
        }
    };
}

macro_rules! middleware_service {
    ($sdk:ident, $service:ident, $config:ident, [$($builder:tt)*]) => {{
        use $sdk::{error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, $service, $config};
        $service::builder $($builder)* ($config::builder().http_plugin(RejectPlugin).build())
            .get_widget(|_: GetWidgetInput| async {
                panic!("rejecting middleware reached its handler");
                #[allow(unreachable_code)]
                Ok::<_, GetWidgetError>(GetWidgetOutput { id: None })
            })
            .build_unchecked()
    }};
}

macro_rules! parity {
    ($test:ident, $legacy:ident, $schema:ident, $service:ident, $config:ident, $wire:expr, $name:literal) => {
        make_error!($legacy);
        make_error!($schema);

        #[tokio::test]
        async fn $test() {
            for kind in ["internal", "modeled"] {
                let before = send(middleware_service!($legacy, $service, $config, [::<Body, _, _, _>]), request($wire, $name, kind), kind == "modeled").await;
                if kind == "internal" {
                    assert_eq!(before, observed_runtime_rejection($wire, "internal"));
                } else {
                    // Compare with the real generated serde handler-error serializer.
                    use $legacy::{error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, $service, $config};
                    let service = $service::builder::<Body, _, _, _>($config::builder().build())
                        .get_widget(|_: GetWidgetInput| async { Err::<GetWidgetOutput, _>(GetWidgetError::make_error()) })
                        .build_unchecked();
                    let handler_error = send(service, request($wire, $name, "success"), true).await;
                    assert_eq!(before, handler_error);
                    assert_eq!(before.status, 400);
                    assert!(!before.body.is_empty());
                }
                assert_eq!(send(middleware_service!($schema, $service, $config, []), request($wire, $name, kind), kind == "modeled").await, before);
            }
        }
    };
}

parity!(
    rest_errors_match_legacy,
    compat_rest_legacy_server_sdk,
    compat_rest_server_sdk,
    ReviewRest,
    ReviewRestConfig,
    Wire::Rest,
    "ReviewRest"
);
parity!(
    aws_json_errors_match_legacy,
    compat_awsjson_legacy_server_sdk,
    compat_awsjson_server_sdk,
    ReviewAwsJson,
    ReviewAwsJsonConfig,
    Wire::AwsJson,
    "ReviewAwsJson"
);
parity!(
    aws_json_11_errors_match_legacy,
    compat_awsjson11_legacy_server_sdk,
    compat_awsjson11_server_sdk,
    ReviewAwsJson11,
    ReviewAwsJson11Config,
    Wire::AwsJson11,
    "ReviewAwsJson11"
);
parity!(
    cbor_errors_match_legacy,
    compat_cbor_legacy_server_sdk,
    compat_cbor_server_sdk,
    ReviewCbor,
    ReviewCborConfig,
    Wire::Cbor,
    "ReviewCbor"
);
parity!(
    xml_errors_match_legacy,
    compat_xml_legacy_server_sdk,
    compat_xml_server_sdk,
    ReviewXml,
    ReviewXmlConfig,
    Wire::Xml,
    "ReviewXml"
);

make_error!(compat_rest_cbor_server_sdk);

#[tokio::test]
async fn one_middleware_uses_each_requests_protocol_for_both_error_paths() {
    use compat_rest_cbor_server_sdk::{
        error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, ReviewRestCbor,
        ReviewRestCborConfig,
    };
    let service = ReviewRestCbor::builder(
        ReviewRestCborConfig::builder()
            .http_plugin(RejectPlugin)
            .build(),
    )
    .get_widget(|_: GetWidgetInput| async {
        panic!("rejecting middleware reached its handler");
        #[allow(unreachable_code)]
        Ok::<_, GetWidgetError>(GetWidgetOutput { id: None })
    })
    .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor, Wire::Rest, Wire::Cbor] {
        for kind in ["internal", "modeled"] {
            let actual = send(
                service.clone(),
                request(wire, "ReviewRestCbor", kind),
                kind == "modeled",
            )
            .await;
            if kind == "internal" {
                assert_eq!(actual, observed_runtime_rejection(wire, "internal"));
            } else {
                let expected = match wire {
                    Wire::Rest => {
                        send_legacy_modeled::<
                            aws_smithy_http_server::protocol::rest_json_1::RestJson1,
                            compat_rest_legacy_server_sdk::error::GetWidgetError,
                        >()
                        .await
                    }
                    Wire::Cbor => {
                        send_legacy_modeled::<
                            aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor,
                            compat_cbor_legacy_server_sdk::error::GetWidgetError,
                        >()
                        .await
                    }
                    _ => unreachable!(),
                };
                assert_eq!(actual, expected);
            }
        }
    }
}

async fn send_legacy_modeled<P, E: MakeError + IntoResponse<P>>() -> Reply {
    snapshot(IntoResponse::<P>::into_response(E::make_error())).await
}

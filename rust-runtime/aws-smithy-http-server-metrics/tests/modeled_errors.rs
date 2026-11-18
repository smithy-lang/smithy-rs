/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(not(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
)))]

use aws_smithy_http_server::{
    extension::ModeledErrorExtension,
    response::IntoResponse,
    schema::{
        protocol, MetadataRoutedProtocol, ProtocolBuildContext, ServerProtocol, ServiceSchema,
    },
};
use aws_smithy_http_server_metrics::operation::MetricsError;
use aws_smithy_schema::shape_id;
use http::StatusCode;
use http_body_util::BodyExt;

fn errors() -> [MetricsError; 2] {
    [
        MetricsError::UnknownExtension,
        MetricsError::MetricAlreadyOpened,
    ]
}

#[tokio::test]
async fn generated_serde_retains_the_legacy_empty_500() {
    for error in errors() {
        let response =
            IntoResponse::<aws_smithy_http_server::protocol::rest_json_1::RestJson1>::into_response(
                error,
            );
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.headers().is_empty());
        assert!(response.extensions().is_empty());
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
async fn contextual_override_frames_both_variants_as_rest_json() {
    for error in errors() {
        let response =
            IntoResponse::<()>::into_response_with_protocol(error, &protocol::RestJson1::default());
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(response.headers()["x-amzn-errortype"], "MetricsError");
        assert_eq!(
            **response
                .extensions()
                .get::<ModeledErrorExtension>()
                .unwrap(),
            "MetricsError"
        );
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "{}"
        );
    }
}

fn spec_bindings<P: MetadataRoutedProtocol>() -> P {
    static SERVICE: ServiceSchema<'static> =
        ServiceSchema::new(shape_id!("test", "Service"), None, &[], &[]);
    let settings = aws_smithy_http_server::schema::settings::parse_settings_json(
        br#"{"responseBindings":"smithy-specs"}"#,
    );
    P::from_build_context(&ProtocolBuildContext::new(&SERVICE).with_settings(Some(&settings)))
        .unwrap()
}

#[tokio::test]
async fn aws_json_uses_protocol_discriminators_and_server_error_classification() {
    let cases: [(Box<dyn ServerProtocol>, &str, &str); 4] = [
        (
            Box::<protocol::AwsJson1_0>::default(),
            "application/x-amz-json-1.0",
            r#"{"__type":"aws.smithy.http.server.metrics#MetricsError"}"#,
        ),
        (
            Box::new(spec_bindings::<protocol::AwsJson1_0>()),
            "application/x-amz-json-1.0",
            r#"{"__type":"aws.smithy.http.server.metrics#MetricsError"}"#,
        ),
        (
            Box::<protocol::AwsJson1_1>::default(),
            "application/x-amz-json-1.1",
            r#"{"__type":"MetricsError"}"#,
        ),
        (
            Box::new(spec_bindings::<protocol::AwsJson1_1>()),
            "application/x-amz-json-1.1",
            r#"{"__type":"MetricsError"}"#,
        ),
    ];
    for (protocol, content_type, body) in cases {
        for error in errors() {
            let response = IntoResponse::<()>::into_response_with_protocol(error, &*protocol);
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(response.headers()["content-type"], content_type);
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                body
            );
        }
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! REST bindings on a shared schema must not change RPC responses.

use aws_smithy_http_server::body::{empty, BoxBody};
use aws_smithy_http_server::schema::protocol::{AwsJson1_0Protocol, AwsJson1_1Protocol, RpcV2CborProtocol};
use aws_smithy_http_server::schema::{
    HttpModeledError, MetadataRoutedProtocol, OperationSchema, ProtocolBuildContext, ServerProtocol, ServiceSchema,
};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use http_body_util::BodyExt;

// These head-only tests supply an event stream member; its body is supplied separately.
static STREAMING_MEMBER: Schema = Schema::new_member(
    shape_id!("test", "StreamingOutput", "events"),
    ShapeType::Union,
    "events",
    0,
)
.with_streaming();

static NAME: Schema = Schema::new_member(shape_id!("test", "Out", "name"), ShapeType::String, "name", 0);
static HEADER: Schema =
    Schema::new_member(shape_id!("test", "Out", "etag"), ShapeType::String, "etag", 1).with_http_header("ETag");
static CODE: Schema =
    Schema::new_member(shape_id!("test", "Out", "code"), ShapeType::Integer, "code", 2).with_http_response_code();
static PREFIX: Schema = Schema::new_member(shape_id!("test", "Out", "metadata"), ShapeType::Map, "metadata", 3)
    .with_map_members(&NAME, &NAME)
    .with_http_prefix_headers("x-meta-");
static ERROR_TRAITS: std::sync::LazyLock<aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
    let mut traits = aws_smithy_schema::TraitMap::new();
    traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});
static OUT: Schema = Schema::new_struct(
    shape_id!("test", "Out"),
    ShapeType::Structure,
    &[&NAME, &HEADER, &CODE, &PREFIX],
)
.with_http(HttpTrait::new("PUT", "/things/{name}", Some(201)))
.with_original_name("Out")
.with_traits(&ERROR_TRAITS);

#[derive(Debug)]
struct Out {
    code: Option<i32>,
}

impl SerializableStruct for Out {
    fn schema(&self) -> &Schema<'_> {
        &OUT
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(&NAME, "n")?;
        serializer.write_string(&HEADER, "\"abc\"")?;
        if let Some(code) = self.code {
            serializer.write_integer(&CODE, code)?;
        }
        serializer.write_map(&PREFIX, &|serializer| {
            serializer.write_string(&NAME, "color")?;
            serializer.write_string(&NAME, "red")
        })
    }
}

impl std::fmt::Display for Out {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("test error")
    }
}

impl std::error::Error for Out {}

impl HttpModeledError for Out {
    fn status_code(&self) -> u16 {
        422
    }
}

fn aws_json_protocols() -> Vec<Box<dyn ServerProtocol>> {
    vec![
        Box::new(AwsJson1_0Protocol::default()),
        Box::new(AwsJson1_1Protocol::default()),
    ]
}

async fn body_text(body: BoxBody) -> String {
    let bytes = body.collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn assert_no_rest_headers(headers: &http::HeaderMap) {
    assert!(!headers.contains_key("etag"));
    assert!(!headers.contains_key("x-meta-color"));
}

#[tokio::test]
async fn aws_json_ignores_rest_status_and_serializes_bound_members_in_the_body() {
    for protocol in aws_json_protocols() {
        // An invalid HTTP status is still a valid integer document member.
        for code in [None, Some(299), Some(-1)] {
            let response = protocol.serialize_response(&OUT, &Out { code });
            assert_eq!(response.status(), 200, "{}", protocol.protocol_id());
            assert_no_rest_headers(response.headers());
            let body = body_text(response.into_body()).await;
            assert!(body.contains("\"name\":\"n\""), "{body}");
            assert!(body.contains("\"etag\":\"\\\"abc\\\"\""), "{body}");
            assert!(body.contains("\"metadata\":{\"color\":\"red\"}"), "{body}");
            if let Some(code) = code {
                assert!(body.contains(&format!("\"code\":{code}")), "{body}");
            }
        }
    }
}

#[test]
fn aws_json_streaming_heads_ignore_rest_status_and_headers() {
    for protocol in aws_json_protocols() {
        let response =
            protocol.serialize_streaming_response(&OUT, &Out { code: Some(299) }, &STREAMING_MEMBER, Some(empty()));
        assert_eq!(response.status(), 200, "{}", protocol.protocol_id());
        assert_no_rest_headers(response.headers());
    }
}

#[tokio::test]
async fn aws_json_errors_use_default_error_status_and_ignore_rest_headers() {
    for protocol in aws_json_protocols() {
        let response = protocol.serialize_error(&Out { code: Some(299) });
        assert_eq!(response.status(), 400, "{}", protocol.protocol_id());
        assert_no_rest_headers(response.headers());
        let body = body_text(response.into_body()).await;
        assert!(body.contains("\"etag\":\"\\\"abc\\\"\""), "{body}");
        assert!(body.contains("\"code\":299"), "{body}");
        assert!(body.contains("\"__type\":"), "{body}");
    }
}

#[test]
fn aws_json_build_does_not_validate_ignored_rest_header_names() {
    static INVALID_HEADER: Schema =
        Schema::new_member(shape_id!("test", "Invalid", "value"), ShapeType::String, "value", 0)
            .with_http_header("invalid header");
    static INVALID_OUT: Schema =
        Schema::new_struct(shape_id!("test", "Invalid"), ShapeType::Structure, &[&INVALID_HEADER]);
    static OPERATION: OperationSchema =
        OperationSchema::new(shape_id!("test", "Op"), &OUT, &INVALID_OUT, &[&INVALID_OUT]);
    static SERVICE: ServiceSchema = ServiceSchema::new(shape_id!("test", "Service"), None, &[], &[&OPERATION]);
    let context = ProtocolBuildContext::new(&SERVICE);
    AwsJson1_0Protocol::from_build_context(&context).expect("REST headers do not apply to awsJson1_0");
    AwsJson1_1Protocol::from_build_context(&context).expect("REST headers do not apply to awsJson1_1");
}

#[test]
fn rpc_v2_cbor_ignores_rest_status_and_headers() {
    let response = RpcV2CborProtocol::default().serialize_response(&OUT, &Out { code: Some(299) });
    assert_eq!(response.status(), 200);
    assert_no_rest_headers(response.headers());
}

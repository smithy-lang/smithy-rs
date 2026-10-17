// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use aws_smithy_http_server::schema::protocol::{AwsJson1_0Protocol, AwsJson1_1Protocol, RpcV2CborProtocol};
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use http_body_util::BodyExt;

static NAME: Schema = Schema::new_member(shape_id!("test", "Out", "name"), ShapeType::String, "name", 0);

static HDR: Schema =
    Schema::new_member(shape_id!("test", "Out", "etag"), ShapeType::String, "etag", 1).with_http_header("ETag");

static CODE: Schema =
    Schema::new_member(shape_id!("test", "Out", "code"), ShapeType::Integer, "code", 2).with_http_response_code();

static OUT: Schema = Schema::new_struct(shape_id!("test", "Out"), ShapeType::Structure, &[&NAME, &HDR, &CODE])
    .with_http(HttpTrait::new("PUT", "/things/{name}", Some(201)))
    .with_original_name("Out");

struct Out {
    code: Option<i32>,
}

impl SerializableStruct for Out {
    fn schema(&self) -> &Schema<'_> {
        &OUT
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&NAME, "n")?;
        s.write_string(&HDR, "\"abc\"")?;
        if let Some(code) = self.code {
            s.write_integer(&CODE, code)?;
        }
        Ok(())
    }
}

fn run(protocol: &dyn ServerProtocol, value: &Out) -> (u16, http::HeaderMap, String) {
    let response = protocol.serialize_response(&OUT, value);
    let (parts, body) = response.into_parts();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let body = rt.block_on(body.collect()).unwrap().to_bytes();
    (
        parts.status.as_u16(),
        parts.headers,
        String::from_utf8_lossy(&body).into_owned(),
    )
}

/// FINDING F2: awsJson answers with the REST `@http(code: 201)` status instead of 200.
#[test]
fn aws_json_ignores_rest_http_code() {
    let protocols: [&dyn ServerProtocol; 2] = [&AwsJson1_0Protocol::default(), &AwsJson1_1Protocol::default()];
    for protocol in protocols {
        let (status, _, body) = run(protocol, &Out { code: None });
        assert_eq!(status, 200, "{}: {body}", protocol.protocol_id());
    }
}

/// FINDING F2: `@httpResponseCode` is honored on awsJson (legacy awsJson ignored it: its binding
/// resolver maps every member to the document).
#[test]
fn aws_json_ignores_http_response_code_member() {
    let (status, _, body) = run(&AwsJson1_0Protocol::default(), &Out { code: Some(299) });
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"code\":299"), "{body}");
}

/// FINDING F2: an `@httpHeader` member is written both into the body and as a header.
#[test]
fn aws_json_does_not_emit_rest_headers() {
    let (_, headers, body) = run(&AwsJson1_0Protocol::default(), &Out { code: None });
    assert!(body.contains("\"etag\""), "{body}");
    assert!(
        !headers.contains_key("etag"),
        "REST header leaked into awsJson: {:?}",
        headers.get("etag")
    );
}

/// Control: rpcv2Cbor, built on the same `RpcProtocol`, does ignore all three.
#[test]
fn control_rpc_v2_cbor_ignores_rest_bindings() {
    let (status, headers, _) = run(&RpcV2CborProtocol::default(), &Out { code: Some(299) });
    assert_eq!(status, 200);
    assert!(!headers.contains_key("etag"));
}

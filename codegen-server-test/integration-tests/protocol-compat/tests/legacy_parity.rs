/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Sends each shared request scenario to SDKs generated from the same model with
//! schemaSerde disabled and enabled. Responses must match, except for the explicit
//! JSON-number normalization and CBOR streaming-blob encoding differences below.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{
    awsjson, cbor, document, legacy_awsjson, legacy_cbor, legacy_document, legacy_rest, rest,
};
use support::scenarios::*;
use support::*;

fn assert_response_parity(protocol: &str, scenario: &str, mut generated: Reply, mut schema: Reply) {
    match (protocol, scenario) {
        ("restJson1" | "awsJson1_0", "non-json-number bigdecimal") => {
            // Generated serde preserves the invalid number spelling; schema serde
            // normalizes it to valid JSON. Both outcomes remain explicit.
            assert_eq!(generated.status, 200);
            assert_eq!(schema.status, 200);
            assert_eq!(generated.text(), r#"{"amount":+000987654321}"#);
            assert_eq!(schema.text(), r#"{"amount":987654321}"#);
            assert_eq!(generated.header("content-length"), Some("24"));
            assert_eq!(schema.header("content-length"), Some("20"));
        }
        ("rpcv2Cbor", "streaming blob output") => {
            // Generated serde uses Blob and a definite-length byte string. Schema
            // serde retains ByteStream and emits an indefinite-length byte string.
            assert_eq!(generated.status, 200);
            assert_eq!(schema.status, 200);
            assert_eq!(
                generated.body.as_ref(),
                b"\xbf\x64data\x4eraw blob bytes\xff"
            );
            assert_eq!(
                schema.body.as_ref(),
                b"\xbf\x64data\x5f\x4eraw blob bytes\xff\xff"
            );
            assert_eq!(generated.header("content-length"), Some("22"));
            assert_eq!(schema.header("content-length"), None);
        }
        _ => {
            assert_eq!(
                snapshot(&schema),
                snapshot(&generated),
                "{protocol}/{scenario}"
            );
            return;
        }
    }
    // These cases differ only in the already-asserted body and its length.
    generated.headers.remove("content-length");
    schema.headers.remove("content-length");
    assert_eq!(schema.headers, generated.headers, "{protocol}/{scenario}");
}

async fn compare<L, S, LB, SB>(
    generated: impl Fn() -> L,
    schema: impl Fn() -> S,
    protocol: &str,
    scenarios: Vec<(&'static str, Build)>,
) where
    L: tower::Service<http::Request<Body>, Response = http::Response<LB>>,
    L::Error: std::fmt::Debug,
    LB: http_body::Body,
    LB::Error: std::fmt::Debug,
    S: tower::Service<http::Request<Body>, Response = http::Response<SB>>,
    S::Error: std::fmt::Debug,
    SB: http_body::Body,
    SB::Error: std::fmt::Debug,
{
    assert!(!scenarios.is_empty(), "{protocol} has no request scenarios");
    for (scenario, build) in scenarios {
        let generated = send(generated(), build()).await;
        let schema = send(schema(), build()).await;
        assert_response_parity(protocol, scenario, generated, schema);
    }
}

#[tokio::test]
async fn rest_json_schema_matches_generated_serde() {
    compare(
        || legacy_rest::service(),
        || rest::service(),
        "restJson1",
        rest_scenarios(),
    )
    .await;
}

#[tokio::test]
async fn aws_json_schema_matches_generated_serde() {
    compare(
        || legacy_awsjson::service(),
        || awsjson::service(),
        "awsJson1_0",
        aws_json_scenarios(),
    )
    .await;
}

#[tokio::test]
async fn cbor_schema_matches_generated_serde() {
    compare(
        || legacy_cbor::service(),
        || cbor::service(),
        "rpcv2Cbor",
        cbor_scenarios(),
    )
    .await;
}

#[tokio::test]
async fn document_schema_matches_generated_serde() {
    compare(
        || legacy_document::service(),
        || document::service(),
        "restJson1-document",
        document_scenarios(),
    )
    .await;
}

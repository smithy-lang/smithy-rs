/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checks structured @httpPayload outputs on AWS JSON and CBOR and verifies that RPC
//! serialization retains the modeled payload member's document key. It also checks
//! that a streaming CBOR blob response uses the blob content type rather than the
//! event-stream content type.
//!
//! Review findings: structure `@httpPayload` over RPC protocols, streaming blob content type.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{all, awsjson, cbor, rest};
use serde_json::json;
use support::*;

#[tokio::test]
async fn rpc_protocols_serialize_struct_http_payload_member_as_a_keyed_body_member() {
    let input = json!({"h": "hv", "payload": {"a": "x"}});
    let r = send(
        awsjson::service(),
        aws_json("1.0", "ReviewAwsJson", "StructPayload", &input.to_string()),
    )
    .await;
    assert_eq!(r.json(), input, "awsJson1_0");
    let r = send(
        all::service(),
        aws_json("1.1", "ReviewAll", "StructPayload", &input.to_string()),
    )
    .await;
    assert_eq!(r.json(), input, "awsJson1_1");
    let r = send(cbor::service(), cbor("ReviewCbor", "StructPayload", &input)).await;
    assert_eq!(r.cbor(), input, "rpcv2Cbor");
    let r = send(
        rest::service(),
        rest(
            "POST",
            "/struct-payload",
            &[("content-type", "application/json"), ("x-h", "hv")],
            br#"{"a":"x"}"#,
        ),
    )
    .await;
    assert_eq!((r.header("x-h"), r.json()), (Some("hv"), json!({"a": "x"})));
}

#[tokio::test]
async fn rpcv2_cbor_streaming_blob_output_is_not_labelled_event_stream() {
    let r = send(rest::service(), rest("GET", "/download", &[], b"")).await;
    assert_eq!(
        (r.header("content-type"), r.text().as_str()),
        (Some("application/octet-stream"), "raw blob bytes")
    );
    let r = send(
        cbor::service(),
        cbor("ReviewCbor", "DownloadBlob", &json!({})),
    )
    .await;
    assert_eq!(
        r.header("content-type"),
        Some("application/cbor"),
        "never labelled as an event stream"
    );
    let r = send(
        awsjson::service(),
        aws_json("1.0", "ReviewAwsJson", "DownloadBlob", "{}"),
    )
    .await;
    assert_ne!(
        r.header("content-type"),
        Some("application/vnd.amazon.eventstream")
    );
}

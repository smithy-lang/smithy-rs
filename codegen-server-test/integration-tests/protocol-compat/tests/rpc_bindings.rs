/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Review finding: REST response bindings (`@http(code)`, `@httpResponseCode`, `@httpHeader`) on RPC protocols.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{all, awsjson, awsjson_spec, cbor};
use serde_json::json;
use support::*;

/// Specification behavior: 200, every member in the body, no REST headers.
fn assert_body_only(label: &str, reply: &Reply, body: serde_json::Value) {
    assert_eq!(reply.status, 200, "{label}");
    assert_eq!(reply.header("x-out"), None, "{label}");
    let actual = if reply
        .header("content-type")
        .is_some_and(|c| c.contains("cbor"))
    {
        reply.cbor()
    } else {
        reply.json()
    };
    assert_eq!(actual, body, "{label}");
}

fn expected_bodies() -> [(&'static str, serde_json::Value); 3] {
    [
        ("ResponseCode", json!({"value": "v"})),
        ("ResponseCodeMember", json!({"status": 202, "value": "v"})),
        ("ResponseHeader", json!({"header": "h", "value": "v"})),
    ]
}

/// `responseBindings: "protocol"` on awsJson1_0; rpcv2Cbor ignores the bindings whatever the setting.
#[tokio::test]
async fn aws_json_protocol_mode_and_cbor_ignore_rest_response_bindings() {
    for (op, body) in expected_bodies() {
        let r = send(
            awsjson_spec::service(),
            aws_json("1.0", "ReviewAwsJson", op, "{}"),
        )
        .await;
        assert_body_only(&format!("awsJson1_0 protocol mode/{op}"), &r, body.clone());
        let r = send(cbor::service(), cbor("ReviewCbor", op, &json!({}))).await;
        assert_body_only(&format!("cbor/{op}"), &r, body);
    }
}

/// The default is mainline's behavior: `@http(code)` and `@httpHeader` are honored (the members stay in the body).
#[tokio::test]
async fn aws_json_default_keeps_historical_response_bindings() {
    for (label, reply) in [
        (
            "awsJson1_0",
            send(
                awsjson::service(),
                aws_json("1.0", "ReviewAwsJson", "ResponseCode", "{}"),
            )
            .await,
        ),
        (
            "all awsJson1_1",
            send(
                all::service(),
                aws_json("1.1", "ReviewAll", "ResponseCode", "{}"),
            )
            .await,
        ),
    ] {
        assert_eq!(reply.status, 201, "{label}");
        assert_eq!(reply.json(), json!({"value": "v"}), "{label}");
    }
    let reply = send(
        awsjson::service(),
        aws_json("1.0", "ReviewAwsJson", "ResponseHeader", "{}"),
    )
    .await;
    assert_eq!(reply.header("x-out"), Some("h"));
    assert_eq!(reply.json(), json!({"header": "h", "value": "v"}));
}

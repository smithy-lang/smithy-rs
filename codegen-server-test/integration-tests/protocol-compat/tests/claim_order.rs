/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Review finding: protocol order decides ambiguous requests when several protocols are enabled.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{all, rest_awsjson, rest_cbor, rest_xml};
use serde_json::json;
use support::*;

/// Which protocol answered, judged by the response `content-type`.
fn answered_by(reply: &Reply) -> String {
    format!(
        "{} {}",
        reply.status,
        reply.header("content-type").unwrap_or("-")
    )
}

#[tokio::test]
async fn claim_order_is_deterministic_and_pinned() {
    let xml = &[("content-type", "application/xml")];
    let json_ct = &[("content-type", "application/json")];
    for (label, answer, expected) in [
        (
            "rest_xml: GET label, no content-type",
            answered_by(&send(rest_xml::service(), rest("GET", "/widgets/1", &[], b"")).await),
            "200 application/json",
        ),
        (
            "rest_xml: GET label, xml content-type",
            answered_by(&send(rest_xml::service(), rest("GET", "/widgets/1", xml, b"")).await),
            "200 application/xml",
        ),
        (
            "rest_xml: GET label, json content-type",
            answered_by(&send(rest_xml::service(), rest("GET", "/widgets/1", json_ct, b"")).await),
            "200 application/json",
        ),
        (
            "all: GET label, xml content-type",
            answered_by(&send(all::service(), rest("GET", "/widgets/1", xml, b"")).await),
            "200 application/xml",
        ),
        (
            "all: awsJson1.0",
            answered_by(&send(all::service(), aws_json("1.0", "ReviewAll", "Ping", "{}")).await),
            "200 application/x-amz-json-1.0",
        ),
        (
            "all: awsJson1.1",
            answered_by(&send(all::service(), aws_json("1.1", "ReviewAll", "Ping", "{}")).await),
            "200 application/x-amz-json-1.1",
        ),
        (
            "all: cbor",
            answered_by(&send(all::service(), cbor("ReviewAll", "Ping", &json!({}))).await),
            "200 application/cbor",
        ),
        (
            "rest_awsjson: awsJson1.0",
            answered_by(
                &send(
                    rest_awsjson::service(),
                    aws_json("1.0", "ReviewRestAwsJson", "Ping", "{}"),
                )
                .await,
            ),
            "200 application/x-amz-json-1.0",
        ),
        (
            "rest_awsjson: awsJson1.1 (not enabled)",
            answered_by(
                &send(
                    rest_awsjson::service(),
                    aws_json("1.1", "ReviewRestAwsJson", "Ping", "{}"),
                )
                .await,
            ),
            "404 -",
        ),
        (
            "rest_cbor: cbor",
            answered_by(
                &send(
                    rest_cbor::service(),
                    cbor("ReviewRestCbor", "Ping", &json!({})),
                )
                .await,
            ),
            "200 application/cbor",
        ),
        (
            "all: POST / json content-type, no target",
            answered_by(&send(all::service(), rest("POST", "/", json_ct, b"{}")).await),
            "404 application/json",
        ),
        (
            "all: POST /ping with target header and json content-type",
            answered_by(
                &send(
                    all::service(),
                    rest(
                        "POST",
                        "/ping",
                        &[
                            ("content-type", "application/json"),
                            ("x-amz-target", "ReviewAll.Ping"),
                        ],
                        b"{}",
                    ),
                )
                .await,
            ),
            "415 application/json",
        ),
    ] {
        assert_eq!(answer, expected, "{label}");
    }
}

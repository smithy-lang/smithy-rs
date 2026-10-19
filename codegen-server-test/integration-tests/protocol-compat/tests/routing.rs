/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Review findings: header-less REST requests and unclaimed requests, on generated services.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{all, awsjson, cbor, rest, rest_awsjson, rest_cbor, rest_xml};
use support::*;

/// Spec-compliant restJson1 clients send no `Content-Type` on bodiless requests.
fn headerless() -> Vec<(&'static str, Request, &'static str)> {
    vec![
        (
            "GET label",
            rest("GET", "/widgets/123", &[], b""),
            r#"{"id":"123"}"#,
        ),
        (
            "DELETE label",
            rest("DELETE", "/widgets/123", &[], b""),
            "{}",
        ),
        (
            "POST no input",
            rest("POST", "/ping", &[], b""),
            r#"{"message":"pong"}"#,
        ),
    ]
}
type Request = http::Request<Body>;

macro_rules! headerless_routes {
    ($name:ident, $module:ident) => {
        #[tokio::test]
        async fn $name() {
            for (what, request, expected) in headerless() {
                let reply = send($module::service(), request).await;
                assert_eq!(reply.status, 200, "{what}: {}", reply.text());
                assert_eq!(reply.text(), expected, "{what}");
            }
        }
    };
}

headerless_routes!(sole_rest_json_routes_requests_without_content_type, rest);
headerless_routes!(
    rest_json_plus_rpcv2cbor_routes_requests_without_content_type,
    rest_cbor
);
headerless_routes!(
    rest_json_plus_aws_json_routes_requests_without_content_type,
    rest_awsjson
);
// restJson1 and restXml cannot be told apart without `Content-Type`; protocol priority picks restJson1.
headerless_routes!(
    rest_json_plus_rest_xml_routes_requests_without_content_type_to_rest_json,
    rest_xml
);
headerless_routes!(
    all_protocols_route_requests_without_content_type_to_rest_json,
    all
);

/// Observed unknown-operation responses (status, content-type, x-amzn-errortype, body).
async fn unknown<S, B>(
    service: S,
    request: Request,
) -> (u16, Option<String>, Option<String>, String)
where
    S: tower::Service<Request, Response = http::Response<B>>,
    S::Error: std::fmt::Debug,
    B: http_body::Body,
    B::Error: std::fmt::Debug,
{
    let reply = send(service, request).await;
    (
        reply.status,
        reply.header("content-type").map(str::to_owned),
        reply.header("x-amzn-errortype").map(str::to_owned),
        reply.text(),
    )
}

#[tokio::test]
async fn unknown_operation_responses_per_service() {
    let json = &[("content-type", "application/json")];
    for (name, observed) in [
        ("rest", unknown(rest::service(), rest_req(json)).await),
        (
            "rest_cbor",
            unknown(rest_cbor::service(), rest_req(json)).await,
        ),
        (
            "rest_awsjson",
            unknown(rest_awsjson::service(), rest_req(json)).await,
        ),
        (
            "rest_xml",
            unknown(rest_xml::service(), rest_req(json)).await,
        ),
        ("all", unknown(all::service(), rest_req(json)).await),
        ("awsjson", unknown(awsjson::service(), rest_req(json)).await),
        ("cbor", unknown(cbor::service(), rest_req(json)).await),
    ] {
        let expected = match name {
            "awsjson" => (
                404,
                Some("application/x-amz-json-1.0".to_owned()),
                None,
                String::new(),
            ),
            "cbor" => (405, None, None, String::new()),
            _ => (
                404,
                Some("application/json".to_owned()),
                Some("UnknownOperationException".to_owned()),
                "{}".to_owned(),
            ),
        };
        assert_eq!(observed, expected, "{name}");
    }
}

fn rest_req(headers: &[(&str, &str)]) -> Request {
    rest("GET", "/no/such/operation", headers, b"")
}

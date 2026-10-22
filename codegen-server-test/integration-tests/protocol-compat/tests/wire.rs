/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Sends protocol requests directly through the generated service and test handlers.
//! Explicit content types select REST XML even when a generated client would send an ambiguous body.

use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body_util::BodyExt;
use multi_protocol_server_sdk::server::body::Body;
use serde_json::{json, Value};
use tower::ServiceExt;

#[derive(Clone, Copy, Debug)]
enum Protocol {
    Json10,
    Json11,
    Cbor,
    RestJson,
    RestXml,
}

const PROTOCOLS: [Protocol; 5] = [
    Protocol::Json10,
    Protocol::Json11,
    Protocol::Cbor,
    Protocol::RestJson,
    Protocol::RestXml,
];

fn encode_cbor(value: &Value, encoder: &mut minicbor::Encoder<Vec<u8>>) {
    match value {
        Value::Null => {
            encoder.null().unwrap();
        }
        Value::Bool(value) => {
            encoder.bool(*value).unwrap();
        }
        Value::Number(value) => {
            encoder
                .i64(value.as_i64().expect("integer request fixture"))
                .unwrap();
        }
        Value::String(value) => {
            encoder.str(value).unwrap();
        }
        Value::Array(values) => {
            encoder.array(values.len() as u64).unwrap();
            for value in values {
                encode_cbor(value, encoder);
            }
        }
        Value::Object(values) => {
            encoder.map(values.len() as u64).unwrap();
            for (key, value) in values {
                encoder.str(key).unwrap();
                encode_cbor(value, encoder);
            }
        }
    }
}

fn decode_cbor(decoder: &mut minicbor::Decoder<'_>) -> Value {
    use minicbor::data::Type;
    match decoder.datatype().unwrap() {
        Type::Null => {
            decoder.null().unwrap();
            Value::Null
        }
        Type::Bool => json!(decoder.bool().unwrap()),
        Type::String => json!(decoder.str().unwrap()),
        Type::Bytes => json!(decoder.bytes().unwrap()),
        Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::Int => json!(decoder.i64().unwrap()),
        Type::F16 | Type::F32 | Type::F64 => json!(decoder.f64().unwrap()),
        Type::Tag => {
            decoder.tag().unwrap();
            decode_cbor(decoder)
        }
        Type::Array | Type::ArrayIndef => {
            let length = decoder.array().unwrap();
            let mut values = Vec::new();
            while length.map_or_else(
                || decoder.datatype().unwrap() != Type::Break,
                |length| values.len() < length as usize,
            ) {
                values.push(decode_cbor(decoder));
            }
            if length.is_none() {
                decoder.skip().unwrap();
            }
            Value::Array(values)
        }
        Type::Map | Type::MapIndef => {
            let length = decoder.map().unwrap();
            let mut values = serde_json::Map::new();
            let mut count = 0;
            while length.map_or_else(
                || decoder.datatype().unwrap() != Type::Break,
                |length| count < length,
            ) {
                let key = decoder.str().unwrap().to_owned();
                values.insert(key, decode_cbor(decoder));
                count += 1;
            }
            if length.is_none() {
                decoder.skip().unwrap();
            }
            Value::Object(values)
        }
        other => panic!("unexpected response CBOR type {other:?}"),
    }
}

struct Snapshot {
    status: u16,
    headers: HeaderMap,
    body: Bytes,
}

impl Snapshot {
    fn value(&self, protocol: Protocol) -> Value {
        if matches!(protocol, Protocol::Cbor) {
            let mut decoder = minicbor::Decoder::new(&self.body);
            let value = decode_cbor(&mut decoder);
            assert_eq!(decoder.position(), self.body.len(), "trailing CBOR tokens");
            value
        } else {
            serde_json::from_slice(&self.body).unwrap()
        }
    }

    fn xml(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }
}

async fn call(
    protocol: Protocol,
    operation: &str,
    path: &str,
    scenario: &str,
    rpc: Value,
    rest: Value,
    xml: &str,
) -> Snapshot {
    let mut builder = Request::builder().header("x-scenario", scenario);
    let body = match protocol {
        Protocol::Json10 | Protocol::Json11 => {
            builder = builder
                .method("POST")
                .uri("/")
                .header(
                    "content-type",
                    if matches!(protocol, Protocol::Json10) {
                        "application/x-amz-json-1.0"
                    } else {
                        "application/x-amz-json-1.1"
                    },
                )
                .header("x-amz-target", format!("MultiProtocolService.{operation}"));
            serde_json::to_vec(&rpc).unwrap()
        }
        Protocol::Cbor => {
            builder = builder
                .method("POST")
                .uri(format!(
                    "/service/MultiProtocolService/operation/{operation}"
                ))
                .header("content-type", "application/cbor")
                .header("smithy-protocol", "rpc-v2-cbor");
            let mut encoder = minicbor::Encoder::new(Vec::new());
            encode_cbor(&rpc, &mut encoder);
            encoder.into_writer()
        }
        Protocol::RestJson | Protocol::RestXml => {
            builder = builder
                .method(if operation == "ResponseBindings" {
                    "PUT"
                } else {
                    "POST"
                })
                .uri(path);
            if matches!(protocol, Protocol::RestXml) {
                builder = builder.header("content-type", "application/xml");
                xml.as_bytes().to_vec()
            } else {
                builder = builder.header("content-type", "application/json");
                serde_json::to_vec(&rest).unwrap()
            }
        }
    };
    let request = builder
        .body(Body::new(http_body_util::Full::new(Bytes::from(body))))
        .unwrap();
    let response = protocol_compat::multi::service()
        .oneshot(request)
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    Snapshot {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes(),
    }
}

#[tokio::test]
async fn cbor_distinguishes_unit_from_explicit_empty_structures() {
    for operation in ["Ping", "NoInputOutput"] {
        let unit = operation == "NoInputOutput";
        let mut request = Request::builder()
            .method("POST")
            .uri(format!(
                "/service/MultiProtocolService/operation/{operation}"
            ))
            .header("smithy-protocol", "rpc-v2-cbor")
            .header("accept", "application/cbor");
        if !unit {
            request = request.header("content-type", "application/cbor");
        }
        let request = request
            .body(Body::new(http_body_util::Full::new(Bytes::from_static(
                if unit { b"" } else { b"\xa0" },
            ))))
            .unwrap();
        let response = protocol_compat::multi::service()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-cbor");
        assert_eq!(response.headers().contains_key("content-type"), !unit);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        if unit {
            assert!(body.is_empty());
        } else {
            assert_eq!(decode_cbor(&mut minicbor::Decoder::new(&body)), json!({}));
        }
    }
}

#[tokio::test]
async fn ordinary_structure_payload_keeps_rpc_key_and_xml_member_override() {
    let payload = json!({"label":"fixture","child":{"name":"child","count":7,"active":true}});
    for protocol in PROTOCOLS {
        let response = call(protocol, "StructurePayload", "/payload/structure", "populated",
            json!({"scenario":"populated","body":payload}), payload.clone(),
            "<MemberEnvelope><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child></MemberEnvelope>").await;
        // awsJson keeps its historical bindings: `@http(code)` applies; only rpcv2Cbor pins 200.
        assert_eq!(
            response.status,
            if matches!(protocol, Protocol::Cbor) {
                200
            } else {
                201
            },
            "{protocol:?}: {:?}",
            response.body
        );
        if matches!(protocol, Protocol::RestXml) {
            assert!(
                response.xml().starts_with("<MemberEnvelope"),
                "{}",
                response.xml()
            );
            assert!(
                response.xml().contains("<name>child</name>"),
                "{}",
                response.xml()
            );
        } else {
            let expected = if matches!(protocol, Protocol::RestJson) {
                payload.clone()
            } else {
                json!({"body":payload})
            };
            assert_eq!(response.value(protocol), expected, "{protocol:?}");
        }
    }
}

#[tokio::test]
async fn ordinary_union_payload_preserves_both_variants() {
    for protocol in PROTOCOLS {
        for (payload, xml) in [
            (
                json!({"text":"chosen"}),
                "<MemberChoice><text>chosen</text></MemberChoice>",
            ),
            (
                json!({"record":{"name":"nested","count":3}}),
                "<MemberChoice><record><name>nested</name><count>3</count></record></MemberChoice>",
            ),
        ] {
            let response = call(
                protocol,
                "UnionPayload",
                "/payload/union",
                "populated",
                json!({"scenario":"populated","body":payload}),
                payload.clone(),
                xml,
            )
            .await;
            // awsJson keeps its historical bindings: `@http(code)` applies; only rpcv2Cbor pins 200.
            assert_eq!(
                response.status,
                if matches!(protocol, Protocol::Cbor) {
                    200
                } else {
                    201
                },
                "{protocol:?}"
            );
            if matches!(protocol, Protocol::RestXml) {
                assert_eq!(response.xml(), xml);
            } else {
                assert_eq!(
                    response.value(protocol),
                    if matches!(protocol, Protocol::RestJson) {
                        payload.clone()
                    } else {
                        json!({"body":payload})
                    },
                    "{protocol:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn response_bindings_follow_each_protocols_rules() {
    for (scenario, code) in [
        ("absent", None),
        ("populated", Some(299)),
        ("override", Some(202)),
    ] {
        for protocol in PROTOCOLS {
            let response = call(
                protocol,
                "ResponseBindings",
                "/things",
                scenario,
                json!({"scenario":scenario}),
                json!({}),
                "<ResponseBindingsInput/>",
            )
            .await;
            let rest = matches!(protocol, Protocol::RestJson | Protocol::RestXml);
            let aws_json = matches!(protocol, Protocol::Json10 | Protocol::Json11);
            // REST honors `@httpResponseCode`; historical awsJson keeps that member in the body
            // (status stays the modeled 201); rpcv2Cbor ignores every REST binding.
            let expected_status = if rest {
                code.unwrap_or(201)
            } else if aws_json {
                201
            } else {
                200
            };
            assert_eq!(response.status, expected_status, "{protocol:?}/{scenario}");
            if rest || aws_json {
                assert_eq!(response.headers.get("etag").unwrap(), "\"abc\"");
                assert_eq!(
                    response
                        .headers
                        .get("x-meta-one")
                        .map(|value| value.to_str().unwrap()),
                    code.map(|_| "1")
                );
            } else {
                assert!(
                    !response.headers.contains_key("etag"),
                    "{protocol:?}/{scenario}"
                );
                assert!(!response
                    .headers
                    .keys()
                    .any(|key| key.as_str().starts_with("x-meta-")));
            }
            if !rest {
                // Historical awsJson and rpcv2Cbor keep every member in the document.
                let mut expected = json!({"name":"n","etag":"\"abc\""});
                if let Some(code) = code {
                    expected["code"] = json!(code);
                    expected["metadata"] = json!({"one":"1","two":"2"});
                }
                assert_eq!(
                    response.value(protocol),
                    expected,
                    "{protocol:?}/{scenario}"
                );
            }
        }
    }
}

#[tokio::test]
async fn rich_success_and_error_populate_nested_values_and_error_bindings() {
    for protocol in PROTOCOLS {
        let response = call(
            protocol,
            "RichData",
            "/rich",
            "populated",
            json!({"scenario":"populated"}),
            json!({}),
            "<RichDataInput/>",
        )
        .await;
        let error = call(
            protocol,
            "RichData",
            "/rich",
            "error",
            json!({"scenario":"error"}),
            json!({}),
            "<RichDataInput/>",
        )
        .await;
        // awsJson keeps its historical bindings: `@http(code)` applies; only rpcv2Cbor pins 200.
        assert_eq!(
            response.status,
            if matches!(protocol, Protocol::Cbor) {
                200
            } else {
                201
            },
            "{protocol:?}"
        );
        if matches!(protocol, Protocol::RestXml) {
            for fragment in [
                "<label>fixture</label>",
                "<name>union-record</name>",
                "<member>YWJj</member>",
                "<choices><member><text>list-choice</text></member><member><record><name>union-record</name><count>3</count></record></member></choices>",
            ] {
                assert!(
                    response.xml().contains(fragment),
                    "missing {fragment}: {}",
                    response.xml()
                );
            }
        } else {
            let mut expected = json!({"record":{
                "payload":{
                    "label":"fixture",
                    "child":{"name":"child","count":7,"active":true},
                    "items":[{"name":"first","count":1,"active":false}],
                    "lookup":{"key":{"name":"mapped","count":2}},
                    "choice":{"text":"chosen"},
                    "bytes":"YWJj",
                    "timestamp":0
                },
                "children":[{"name":"extra","count":9}],
                "choices":[{"text":"list-choice"},{"record":{"name":"union-record","count":3}}],
                "blobs":["YWJj","eHl6"]
            }});
            if matches!(protocol, Protocol::Cbor) {
                expected["record"]["payload"]["bytes"] = json!([97, 98, 99]);
                expected["record"]["payload"]["timestamp"] = json!(0.0);
                expected["record"]["blobs"] = json!([[97, 98, 99], [120, 121, 122]]);
            }
            assert_eq!(response.value(protocol), expected, "{protocol:?}");
        }
        // Everything but rpcv2Cbor emits the error's `@httpError(422)` and header bindings;
        // historical awsJson also keeps the bound members in the document.
        assert_eq!(error.status, 422, "{protocol:?}");
        if matches!(protocol, Protocol::Cbor) {
            assert!(!error.headers.contains_key("x-error-tag"));
        } else {
            assert_eq!(error.headers.get("x-error-tag").unwrap(), "error-tag");
            assert_eq!(error.headers.get("x-error-meta-reason").unwrap(), "fixture");
        }
        if !matches!(protocol, Protocol::RestJson | Protocol::RestXml) {
            let value = error.value(protocol);
            assert_eq!(value["etag"], "error-tag");
            assert_eq!(value["metadata"]["reason"], "fixture");
            assert_eq!(value["details"]["name"], "diagnostic");
        }
    }
}

#[tokio::test]
async fn modeled_defaults_use_the_generated_builder_and_allow_overrides() {
    for protocol in PROTOCOLS
        .into_iter()
        .filter(|protocol| !matches!(protocol, Protocol::RestXml))
    {
        for (scenario, expected) in [
            (
                "default",
                json!({"text":"builder","count":17,"color":"red"}),
            ),
            (
                "override",
                json!({"text":"explicit","count":23,"color":"blue"}),
            ),
        ] {
            let response = call(
                protocol,
                "ModeledDefaults",
                "/defaults",
                scenario,
                json!({"scenario":scenario}),
                json!({}),
                "",
            )
            .await;
            assert_eq!(
                response.status,
                if matches!(protocol, Protocol::Cbor) {
                    200
                } else {
                    201
                },
                "{protocol:?}: {:?}",
                response.body
            );
            assert_eq!(
                response.value(protocol),
                expected,
                "{protocol:?}/{scenario}"
            );
        }
    }
}

#[tokio::test]
async fn sparse_outputs_distinguish_absent_empty_and_null_entries() {
    for protocol in PROTOCOLS
        .into_iter()
        .filter(|protocol| !matches!(protocol, Protocol::RestXml))
    {
        for (scenario, expected) in [
            ("absent", json!({})),
            ("empty", json!({"values":[],"entries":{}})),
            (
                "populated",
                json!({"values":["first",null,"last"],"entries":{"one":"value","nil":null}}),
            ),
        ] {
            let response = call(
                protocol,
                "NullableValues",
                "/nullable",
                scenario,
                json!({"scenario":scenario}),
                json!({}),
                "",
            )
            .await;
            assert_eq!(
                response.status,
                if matches!(protocol, Protocol::Cbor) {
                    200
                } else {
                    201
                },
                "{protocol:?}"
            );
            assert_eq!(
                response.value(protocol),
                expected,
                "{protocol:?}/{scenario}"
            );
        }
    }
}

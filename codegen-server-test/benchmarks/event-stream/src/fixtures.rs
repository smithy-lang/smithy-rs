/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Builds deterministic event-stream requests and expected responses for the benchmark.
//! It encodes structured and binary event payloads, fragments bodies, and validates
//! decoded events, frame counts, and modeled stream failures for JSON and CBOR.
//!
use crate::{Case, Direction, Kind, Protocol, Server, METADATA};
use aws_smithy_eventstream::frame::{read_message_from, write_message_to};
use aws_smithy_http_server::body::Body;
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body_util::{BodyExt, StreamBody};
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use tower::ServiceExt;

pub struct Fixture {
    case: Case,
    fail: bool,
    chunks: Arc<[Bytes]>,
}

fn encode_cbor(value: &Value, encoder: &mut minicbor::Encoder<Vec<u8>>) {
    match value {
        Value::String(value) => {
            encoder.str(value).unwrap();
        }
        Value::Number(value) => {
            encoder.i64(value.as_i64().unwrap()).unwrap();
        }
        Value::Bool(value) => {
            encoder.bool(*value).unwrap();
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
        Value::Null => {
            encoder.null().unwrap();
        }
    }
}

fn encode_document(protocol: Protocol, value: &Value) -> Bytes {
    match protocol {
        Protocol::RestJson => Bytes::from(serde_json::to_vec(value).unwrap()),
        Protocol::RpcV2Cbor => {
            let mut encoder = minicbor::Encoder::new(Vec::new());
            encode_cbor(value, &mut encoder);
            Bytes::from(encoder.into_writer())
        }
    }
}

fn decode_cbor(decoder: &mut minicbor::Decoder<'_>) -> Value {
    use minicbor::data::Type;
    match decoder.datatype().unwrap() {
        Type::Map | Type::MapIndef => {
            let length = decoder.map().unwrap();
            let mut result = serde_json::Map::new();
            let mut count = 0;
            while length.map_or_else(|| decoder.datatype().unwrap() != Type::Break, |n| count < n) {
                let key = decoder.str().unwrap().to_owned();
                result.insert(key, decode_cbor(decoder));
                count += 1;
            }
            if length.is_none() {
                decoder.skip().unwrap();
            }
            Value::Object(result)
        }
        Type::Array | Type::ArrayIndef => {
            let length = decoder.array().unwrap();
            let mut result = Vec::new();
            while length.map_or_else(
                || decoder.datatype().unwrap() != Type::Break,
                |n| result.len() < n as usize,
            ) {
                result.push(decode_cbor(decoder));
            }
            if length.is_none() {
                decoder.skip().unwrap();
            }
            Value::Array(result)
        }
        Type::String => json!(decoder.str().unwrap()),
        Type::Bool => json!(decoder.bool().unwrap()),
        Type::Null => {
            decoder.null().unwrap();
            Value::Null
        }
        _ => json!(decoder.i64().unwrap()),
    }
}

fn decode_document(protocol: Protocol, payload: &[u8]) -> Value {
    match protocol {
        Protocol::RestJson => serde_json::from_slice(payload).expect("JSON payload"),
        Protocol::RpcV2Cbor => {
            let mut decoder = minicbor::Decoder::new(payload);
            let result = decode_cbor(&mut decoder);
            assert_eq!(decoder.position(), payload.len(), "trailing CBOR bytes");
            result
        }
    }
}

fn frame(event_type: &str, content_type: &str, payload: Bytes, sequence: Option<usize>) -> Bytes {
    let mut headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(
            ":event-type",
            HeaderValue::String(event_type.to_owned().into()),
        ),
        Header::new(
            ":content-type",
            HeaderValue::String(content_type.to_owned().into()),
        ),
    ];
    if let Some(sequence) = sequence {
        headers.push(Header::new("sequence", HeaderValue::Int64(sequence as i64)));
    }
    let message = Message::new_from_parts(headers, payload);
    let mut bytes = Vec::new();
    write_message_to(&message, &mut bytes).expect("valid event frame");
    Bytes::from(bytes)
}

fn structured(size: usize) -> Value {
    json!({
        "text": "x".repeat(size),
        "nested": {"label": METADATA, "enabled": true},
        "values": [0, 1, 2, 3, 4, 5, 6, 7],
    })
}

fn header<'a>(message: &'a Message, name: &str) -> &'a HeaderValue {
    message
        .headers()
        .iter()
        .find(|header| header.name().as_str() == name)
        .unwrap_or_else(|| panic!("missing event header {name}"))
        .value()
}

fn string_header<'a>(message: &'a Message, name: &str) -> &'a str {
    header(message, name)
        .as_string()
        .expect("string event header")
        .as_str()
}

impl Fixture {
    pub fn new(case: Case) -> Self {
        Self::with_error(case, false)
    }

    pub fn with_error(case: Case, fail: bool) -> Self {
        assert!(case.count > 0 && case.count <= i32::MAX as usize);
        assert!(case.size <= i32::MAX as usize);
        assert!(!fail || case.direction == Direction::Download);
        let mut chunks = Vec::new();
        let initial = json!({
            "kind": case.kind.name(), "size": case.size, "count": case.count,
        });
        if case.direction == Direction::Download {
            if case.protocol == Protocol::RpcV2Cbor {
                let mut input = initial;
                input["fail"] = json!(fail);
                chunks.push(encode_document(case.protocol, &input));
            }
        } else {
            if case.protocol == Protocol::RpcV2Cbor {
                chunks.push(frame(
                    "initial-request",
                    "application/cbor",
                    encode_document(case.protocol, &initial),
                    None,
                ));
            }
            for sequence in 0..case.count {
                let (payload, content_type) = match case.kind {
                    Kind::Binary => (
                        Bytes::from(vec![(sequence % 251) as u8; case.size]),
                        "application/octet-stream",
                    ),
                    Kind::Structured => (
                        encode_document(case.protocol, &structured(case.size)),
                        if case.protocol == Protocol::RestJson {
                            "application/json"
                        } else {
                            "application/cbor"
                        },
                    ),
                };
                chunks.push(frame(
                    case.kind.name(),
                    content_type,
                    payload,
                    Some(sequence),
                ));
            }
        }
        Self {
            case,
            fail,
            chunks: chunks.into(),
        }
    }

    /// Clones immutable pre-encoded bytes; neither payload encoding nor whole-stream copying.
    pub fn request(&self) -> Request<Body> {
        let case = self.case;
        let mut builder = Request::builder().method("POST");
        if case.protocol == Protocol::RestJson {
            builder = builder
                .uri(format!("/{}", case.direction.name()))
                .header("x-benchmark-kind", case.kind.name())
                .header("x-benchmark-size", case.size)
                .header("x-benchmark-count", case.count)
                .header("x-benchmark-error", self.fail.to_string());
        } else {
            builder = builder
                .uri(format!(
                    "/service/BenchmarkService/operation/{}",
                    case.direction.operation()
                ))
                .header("smithy-protocol", "rpc-v2-cbor");
        }
        let content_type = if case.direction != Direction::Download {
            "application/vnd.amazon.eventstream"
        } else if case.protocol == Protocol::RpcV2Cbor {
            "application/cbor"
        } else {
            "application/json"
        };
        let chunks = self.chunks.clone();
        let stream = futures_util::stream::iter(0..chunks.len())
            .map(move |index| Ok::<_, Infallible>(http_body::Frame::data(chunks[index].clone())));
        builder
            .header("content-type", content_type)
            .body(Body::new(StreamBody::new(stream)))
            .unwrap()
    }

    /// Split each input frame into arbitrary HTTP chunks for correctness testing.
    pub fn fragmented(&self, chunk_size: usize) -> Self {
        assert!(chunk_size > 0);
        let chunks = self
            .chunks
            .iter()
            .flat_map(|bytes| {
                (0..bytes.len())
                    .step_by(chunk_size)
                    .map(move |start| bytes.slice(start..(start + chunk_size).min(bytes.len())))
            })
            .collect::<Vec<_>>();
        Self {
            case: self.case,
            fail: self.fail,
            chunks: chunks.into(),
        }
    }

    /// Returns independently encoded event messages, excluding CBOR initial metadata.
    /// Frame decoding and CRC verification happen before serde-only measurement.
    pub fn event_messages(&self) -> Vec<Message> {
        assert!(self.case.direction != Direction::Download);
        self.chunks
            .iter()
            .skip(usize::from(self.case.protocol == Protocol::RpcV2Cbor))
            .map(|bytes| read_message_from(&bytes[..]).expect("valid fixture frame"))
            .collect()
    }

    /// Checks one event's complete modeled payload and event headers outside timing.
    pub fn validate_event(&self, sequence: usize, message: &Message) {
        assert_eq!(string_header(message, ":message-type"), "event");
        assert_eq!(string_header(message, ":event-type"), self.case.kind.name());
        assert_eq!(
            header(message, "sequence").as_int64().unwrap(),
            sequence as i64
        );
        match self.case.kind {
            Kind::Binary => {
                assert_eq!(
                    string_header(message, ":content-type"),
                    "application/octet-stream"
                );
                assert_eq!(
                    message.payload(),
                    &vec![(sequence % 251) as u8; self.case.size]
                );
            }
            Kind::Structured => {
                assert_eq!(
                    string_header(message, ":content-type"),
                    if self.case.protocol == Protocol::RestJson {
                        "application/json"
                    } else {
                        "application/cbor"
                    }
                );
                assert_eq!(
                    decode_document(self.case.protocol, message.payload()),
                    structured(self.case.size)
                );
            }
        }
    }

    /// Decoding and deep payload checks run before timing, never in Criterion's measured drain.
    pub async fn validate(&self, server: Server) {
        let response = server.oneshot(self.request()).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .expect("valid response body")
            .to_bytes();
        assert_eq!(
            parts.status,
            200,
            "{:?}: {}",
            self.case,
            String::from_utf8_lossy(&bytes)
        );
        if self.case.direction == Direction::Upload {
            assert_eq!(
                decode_document(self.case.protocol, &bytes),
                json!({
                    "count": self.case.count, "bytes": self.case.count * self.case.size,
                })
            );
            return;
        }
        self.validate_events(&parts.headers, &bytes);
    }

    fn validate_events(&self, headers: &HeaderMap, mut bytes: &[u8]) {
        let mut messages = Vec::new();
        while !bytes.is_empty() {
            assert!(bytes.len() >= 4, "truncated frame length");
            let length = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
            assert!(
                length >= 16 && length <= bytes.len(),
                "truncated event frame"
            );
            messages
                .push(read_message_from(&bytes[..length]).expect("valid frame CRC and headers"));
            bytes = &bytes[length..];
        }
        let start = if self.case.protocol == Protocol::RpcV2Cbor {
            let initial = &messages[0];
            assert_eq!(string_header(initial, ":event-type"), "initial-response");
            assert_eq!(
                decode_document(self.case.protocol, initial.payload()),
                json!({"metadata": METADATA})
            );
            1
        } else {
            assert_eq!(headers["x-benchmark-metadata"], METADATA);
            0
        };
        assert_eq!(
            messages.len(),
            start + self.case.count + usize::from(self.fail)
        );
        for (sequence, message) in messages[start..start + self.case.count].iter().enumerate() {
            self.validate_event(sequence, message);
        }
        if self.fail {
            let error = messages.last().unwrap();
            assert_eq!(string_header(error, ":message-type"), "exception");
            assert_eq!(string_header(error, ":exception-type"), "failure");
            let mut expected = json!({"message": "expected stream failure"});
            if self.case.protocol == Protocol::RpcV2Cbor {
                expected["__type"] = json!("benchmark.eventstream#StreamFailure");
            }
            assert_eq!(
                decode_document(self.case.protocol, error.payload()),
                expected
            );
        }
    }
}

use futures_util::StreamExt;

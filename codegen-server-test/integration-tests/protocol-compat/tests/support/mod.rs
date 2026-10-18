//! Request builders and response snapshots shared by the review-finding tests.
#![allow(dead_code)]

use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

pub use super::{from_bytes, Body};
pub mod scenarios;

pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.text()))
    }
    pub fn cbor(&self) -> Value {
        let mut decoder = minicbor::Decoder::new(&self.body);
        decode_cbor(&mut decoder)
    }
}

/// Sends one request through the service without a socket.
pub async fn send<S, B>(service: S, request: Request<Body>) -> Reply
where
    S: tower::Service<Request<Body>, Response = http::Response<B>>,
    S::Error: std::fmt::Debug,
    B: http_body::Body,
    B::Error: std::fmt::Debug,
{
    let response = service.oneshot(request).await.expect("infallible service");
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body: body.collect().await.expect("response body").to_bytes(),
    }
}

pub fn body(bytes: &[u8]) -> Body {
    from_bytes(bytes)
}

/// A REST request. Pass no `content-type` to model a spec-compliant client sending a bodiless request.
pub fn rest(method: &str, uri: &str, headers: &[(&str, &str)], payload: &[u8]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(body(payload)).unwrap()
}

pub fn aws_json(version: &str, service: &str, operation: &str, payload: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", format!("application/x-amz-json-{version}"))
        .header("x-amz-target", format!("{service}.{operation}"))
        .body(body(payload.as_bytes()))
        .unwrap()
}

pub fn cbor(service: &str, operation: &str, payload: &Value) -> Request<Body> {
    let mut encoder = minicbor::Encoder::new(Vec::new());
    encode_cbor(payload, &mut encoder);
    Request::builder()
        .method("POST")
        .uri(format!("/service/{service}/operation/{operation}"))
        .header("content-type", "application/cbor")
        .header("smithy-protocol", "rpc-v2-cbor")
        .body(body(&encoder.into_writer()))
        .unwrap()
}

pub fn encode_cbor(value: &Value, encoder: &mut minicbor::Encoder<Vec<u8>>) {
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

pub fn decode_cbor(decoder: &mut minicbor::Decoder<'_>) -> Value {
    use minicbor::data::Type;
    match decoder.datatype().unwrap() {
        Type::Null => {
            decoder.null().unwrap();
            Value::Null
        }
        Type::Bool => json!(decoder.bool().unwrap()),
        Type::String => json!(decoder.str().unwrap()),
        Type::Bytes => json!(decoder.bytes().unwrap()),
        Type::BytesIndef => json!(decoder.bytes_iter().unwrap().flat_map(|chunk| chunk.unwrap().to_vec()).collect::<Vec<u8>>()),
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


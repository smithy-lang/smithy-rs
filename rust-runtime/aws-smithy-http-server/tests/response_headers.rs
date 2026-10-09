/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Complete-response regressions for header multiplicity and framing.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_http_server::body::empty;
use aws_smithy_http_server::routing::IntoMakeService;
use aws_smithy_http_server::schema::protocol::{
    AwsJson1_0Protocol, AwsJson1_1Protocol, RestJson1Protocol, RestXmlProtocol, RpcV2CborProtocol,
};
use aws_smithy_http_server::schema::{HttpModeledError, ServerProtocol};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{prelude, shape_id, Schema, ShapeType};
use bytes::Bytes;
use http_body_util::BodyExt;

static SCALAR: Schema = Schema::new_member(shape_id!("test", "Headers", "scalar"), ShapeType::String, "scalar", 0)
    .with_http_header("x-scalar");
static ITEMS: Schema = Schema::new_member(shape_id!("test", "Headers", "items"), ShapeType::List, "items", 1)
    .with_list_member(&prelude::STRING)
    .with_http_header("x-items");
static NUMBERS: Schema = Schema::new_member(shape_id!("test", "Headers", "numbers"), ShapeType::List, "numbers", 2)
    .with_list_member(&prelude::INTEGER)
    .with_http_header("x-numbers");
static PREFIX: Schema = Schema::new_member(shape_id!("test", "Headers", "prefix"), ShapeType::Map, "prefix", 3)
    .with_map_members(&prelude::STRING, &prelude::STRING)
    .with_http_prefix_headers("content-");
static ATTRIBUTE: Schema =
    Schema::new_member(shape_id!("test", "Headers", "attr"), ShapeType::String, "attr", 4).with_xml_attribute();
static BODY: Schema = Schema::new_member(shape_id!("test", "Headers", "body"), ShapeType::String, "body", 5);
static LENGTH: Schema = Schema::new_member(shape_id!("test", "Headers", "length"), ShapeType::String, "length", 6)
    .with_http_header("content-length");
static CONTENT_TYPE: Schema = Schema::new_member(
    shape_id!("test", "Headers", "contentType"),
    ShapeType::String,
    "contentType",
    7,
)
.with_http_header("content-type");
static OUTPUT: Schema = Schema::new_struct(
    shape_id!("test", "Headers"),
    ShapeType::Structure,
    &[
        &SCALAR,
        &ITEMS,
        &NUMBERS,
        &PREFIX,
        &ATTRIBUTE,
        &BODY,
        &LENGTH,
        &CONTENT_TYPE,
    ],
)
.with_original_name("Headers");

#[derive(Clone, Debug)]
struct HeadersOutput {
    length: Option<String>,
    content_type: Option<String>,
    prefix: Vec<(String, String)>,
}

impl Default for HeadersOutput {
    fn default() -> Self {
        Self {
            length: None,
            content_type: None,
            prefix: vec![("color".into(), "red".into())],
        }
    }
}

impl SerializableStruct for HeadersOutput {
    fn schema(&self) -> &Schema<'_> {
        &OUTPUT
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&SCALAR, "scalar")?;
        s.write_list(&ITEMS, &|s| {
            for item in ["one", "two", "a,b", "a\"b"] {
                s.write_string(&prelude::STRING, item)?;
            }
            Ok(())
        })?;
        s.write_list(&NUMBERS, &|s| {
            s.write_integer(&prelude::INTEGER, 1)?;
            s.write_integer(&prelude::INTEGER, 2)
        })?;
        s.write_map(&PREFIX, &|s| {
            for (key, value) in &self.prefix {
                s.write_string(&prelude::STRING, key)?;
                s.write_string(&prelude::STRING, value)?;
            }
            Ok(())
        })?;
        s.write_string(&ATTRIBUTE, "attr")?;
        s.write_string(&BODY, "body")?;
        if let Some(length) = &self.length {
            s.write_string(&LENGTH, length)?;
        }
        if let Some(content_type) = &self.content_type {
            s.write_string(&CONTENT_TYPE, content_type)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for HeadersOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("modeled header error")
    }
}

impl std::error::Error for HeadersOutput {}

impl HttpModeledError for HeadersOutput {
    fn status_code(&self) -> u16 {
        422
    }
}

fn protocols() -> Vec<Box<dyn ServerProtocol>> {
    vec![
        Box::new(RestJson1Protocol::default()),
        Box::new(RestXmlProtocol::default()),
        Box::new(AwsJson1_0Protocol::default()),
        Box::new(AwsJson1_1Protocol::default()),
    ]
}

fn values<'a>(headers: &'a http::HeaderMap, name: &str) -> Vec<&'a str> {
    headers
        .get_all(name)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect()
}

fn assert_bound_headers(headers: &http::HeaderMap) {
    assert_eq!(values(headers, "x-scalar"), ["scalar"]);
    assert_eq!(values(headers, "x-items"), ["one", "two", "\"a,b\"", "\"a\\\"b\""]);
    assert_eq!(values(headers, "x-numbers"), ["1", "2"]);
    assert_eq!(values(headers, "content-color"), ["red"]);
}

#[test]
fn all_header_values_survive_outputs_errors_and_streaming_heads() {
    for protocol in protocols() {
        let value = HeadersOutput::default();
        let output = protocol.serialize_response(&OUTPUT, &value);
        assert_eq!(output.status(), 200);
        assert_bound_headers(output.headers());
        let error = protocol.serialize_error(&value);
        assert_eq!(error.status(), 422);
        assert_bound_headers(error.headers());
        let streaming = protocol.serialize_streaming_response(&OUTPUT, &value, empty());
        assert_eq!(streaming.status(), 200);
        assert_bound_headers(streaming.headers());
        assert!(streaming.headers().get("content-length").is_none());
    }
}

static EPOCH_TIMESTAMP: Schema = Schema::new(shape_id!("test", "EpochTimestamp"), ShapeType::Timestamp)
    .with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::EpochSeconds);
static DATE_TIME_TIMESTAMP: Schema = Schema::new(shape_id!("test", "DateTimeTimestamp"), ShapeType::Timestamp)
    .with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::DateTime);
static DEFAULT_TIMES: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "defaults"),
    ShapeType::List,
    "defaults",
    0,
)
.with_list_member(&prelude::TIMESTAMP)
.with_http_header("x-default-times");
static EPOCH_TIMES: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "epochs"),
    ShapeType::List,
    "epochs",
    1,
)
.with_list_member(&EPOCH_TIMESTAMP)
.with_http_header("x-epoch-times");
static DATE_TIMES: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "dates"),
    ShapeType::List,
    "dates",
    2,
)
.with_list_member(&DATE_TIME_TIMESTAMP)
.with_http_header("x-date-times");
static MEMBER_FORMAT_TIMES: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "member"),
    ShapeType::List,
    "member",
    3,
)
.with_list_member(&prelude::TIMESTAMP)
.with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::EpochSeconds)
.with_http_header("x-member-times");
static ELEMENT_FORMAT_TIMES: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "element"),
    ShapeType::List,
    "element",
    4,
)
.with_list_member(&EPOCH_TIMESTAMP)
.with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::DateTime)
.with_http_header("x-element-times");
static DEFAULT_TIME: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "default"),
    ShapeType::Timestamp,
    "default",
    5,
)
.with_http_header("x-default-time");
static EPOCH_TIME: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "epoch"),
    ShapeType::Timestamp,
    "epoch",
    6,
)
.with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::EpochSeconds)
.with_http_header("x-epoch-time");
static DATE_TIME: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "date"),
    ShapeType::Timestamp,
    "date",
    7,
)
.with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::DateTime)
.with_http_header("x-date-time");
static TIME_ATTRIBUTE: Schema = Schema::new_member(
    shape_id!("test", "TimestampHeaders", "attr"),
    ShapeType::String,
    "attr",
    8,
)
.with_xml_attribute();
static TIMESTAMP_HEADERS: Schema = Schema::new_struct(
    shape_id!("test", "TimestampHeaders"),
    ShapeType::Structure,
    &[
        &DEFAULT_TIMES,
        &EPOCH_TIMES,
        &DATE_TIMES,
        &MEMBER_FORMAT_TIMES,
        &ELEMENT_FORMAT_TIMES,
        &DEFAULT_TIME,
        &EPOCH_TIME,
        &DATE_TIME,
        &TIME_ATTRIBUTE,
    ],
)
.with_original_name("TimestampHeaders");

#[derive(Debug)]
struct TimestampHeaders {
    element_schema: Option<&'static Schema<'static>>,
}

impl SerializableStruct for TimestampHeaders {
    fn schema(&self) -> &Schema<'_> {
        &TIMESTAMP_HEADERS
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        for member in [
            &DEFAULT_TIMES,
            &EPOCH_TIMES,
            &DATE_TIMES,
            &MEMBER_FORMAT_TIMES,
            &ELEMENT_FORMAT_TIMES,
        ] {
            s.write_list(member, &|s| {
                let element = self.element_schema.unwrap_or_else(|| member.member().unwrap());
                for seconds in [0, 7] {
                    s.write_timestamp(element, &aws_smithy_types::DateTime::from_secs(seconds))?;
                }
                Ok(())
            })?;
        }
        for member in [&DEFAULT_TIME, &EPOCH_TIME, &DATE_TIME] {
            s.write_timestamp(member, &aws_smithy_types::DateTime::from_secs(0))?;
        }
        // Exercise the XML attribute pass as well as ordinary JSON serialization.
        s.write_string(&TIME_ATTRIBUTE, "attr")
    }
}

impl std::fmt::Display for TimestampHeaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("modeled timestamp error")
    }
}

impl std::error::Error for TimestampHeaders {}

impl HttpModeledError for TimestampHeaders {
    fn status_code(&self) -> u16 {
        422
    }
}

#[test]
fn timestamp_headers_use_member_and_element_formats() {
    for protocol in protocols() {
        let value = TimestampHeaders { element_schema: None };
        for (response, status) in [
            (protocol.serialize_response(&TIMESTAMP_HEADERS, &value), 200),
            (protocol.serialize_error(&value), 422),
            (
                protocol.serialize_streaming_response(&TIMESTAMP_HEADERS, &value, empty()),
                200,
            ),
        ] {
            assert_eq!(response.status(), status);
            assert_eq!(
                values(response.headers(), "x-default-times"),
                ["Thu, 01 Jan 1970 00:00:00 GMT", "Thu, 01 Jan 1970 00:00:07 GMT"]
            );
            for name in ["x-epoch-times", "x-member-times", "x-element-times"] {
                assert_eq!(values(response.headers(), name), ["0", "7"], "{name}");
            }
            assert_eq!(
                values(response.headers(), "x-date-times"),
                ["1970-01-01T00:00:00Z", "1970-01-01T00:00:07Z"]
            );
            assert_eq!(
                values(response.headers(), "x-default-time"),
                ["Thu, 01 Jan 1970 00:00:00 GMT"]
            );
            assert_eq!(values(response.headers(), "x-epoch-time"), ["0"]);
            assert_eq!(values(response.headers(), "x-date-time"), ["1970-01-01T00:00:00Z"]);
        }
    }
}

#[test]
fn timestamp_header_lists_keep_compiled_format_when_writing_an_unformatted_target() {
    for protocol in protocols() {
        let value = TimestampHeaders {
            element_schema: Some(&prelude::TIMESTAMP),
        };
        let response = protocol.serialize_response(&TIMESTAMP_HEADERS, &value);
        assert_eq!(response.status(), 200);
        for name in ["x-epoch-times", "x-member-times", "x-element-times"] {
            assert_eq!(values(response.headers(), name), ["0", "7"], "{name}");
        }
        assert_eq!(
            values(response.headers(), "x-date-times"),
            ["1970-01-01T00:00:00Z", "1970-01-01T00:00:07Z"]
        );
    }
}

#[test]
fn timestamp_header_lists_honor_the_schema_passed_to_the_element_writer() {
    for protocol in protocols() {
        let value = TimestampHeaders {
            element_schema: Some(&EPOCH_TIMESTAMP),
        };
        let response = protocol.serialize_response(&TIMESTAMP_HEADERS, &value);
        assert_eq!(response.status(), 200);
        for name in ["x-default-times", "x-date-times"] {
            assert_eq!(values(response.headers(), name), ["0", "7"], "{name}");
        }
    }
}

#[tokio::test]
async fn buffered_lengths_match_actual_output_and_error_bodies() {
    let mut protocols = protocols();
    protocols.push(Box::new(RpcV2CborProtocol::default()));
    for protocol in protocols {
        let value = HeadersOutput::default();
        for response in [
            protocol.serialize_response(&OUTPUT, &value),
            protocol.serialize_error(&value),
        ] {
            let lengths = values(response.headers(), "content-length");
            assert_eq!(lengths.len(), 1);
            let length = lengths[0].parse::<usize>().unwrap();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(length, body.len());
            assert!(!body.is_empty());
        }
    }
}

// AWS JSON includes the bound length member in its body. Find the length of the
// expected JSON document including that decimal string and the error discriminator.
fn matching_aws_json_length(protocol: &dyn ServerProtocol, error: bool) -> String {
    let mut length = 0;
    loop {
        let mut expected = format!(
            r#"{{"scalar":"scalar","items":["one","two","a,b","a\"b"],"numbers":[1,2],"prefix":{{"color":"red"}},"attr":"attr","body":"body","length":"{length}""#
        );
        if error {
            let discriminator = if protocol.protocol_id().shape_name() == "awsJson1_0" {
                "test#Headers"
            } else {
                "Headers"
            };
            expected.push_str(&format!(r#","__type":"{discriminator}""#));
        }
        expected.push('}');
        if expected.len() == length {
            return length.to_string();
        }
        length = expected.len();
    }
}

#[tokio::test]
async fn matching_modeled_lengths_are_emitted_once() {
    for protocol in protocols() {
        for error in [false, true] {
            let mut value = HeadersOutput::default();
            let baseline = if error {
                protocol.serialize_error(&value)
            } else {
                protocol.serialize_response(&OUTPUT, &value)
            };
            let baseline_length = baseline.into_body().collect().await.unwrap().to_bytes().len();
            value.length = Some(if protocol.protocol_id().shape_name().starts_with("awsJson") {
                matching_aws_json_length(protocol.as_ref(), error)
            } else {
                baseline_length.to_string()
            });
            let response = if error {
                protocol.serialize_error(&value)
            } else {
                protocol.serialize_response(&OUTPUT, &value)
            };
            assert_eq!(response.status(), if error { 422 } else { 200 });
            assert_eq!(
                values(response.headers(), "content-length"),
                [value.length.as_deref().unwrap()]
            );
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body.len().to_string(), value.length.unwrap());
        }
    }
}

#[test]
fn modeled_buffered_lengths_are_preserved_without_validation() {
    for protocol in protocols() {
        for length in ["0", "1", "-1", "+1", "1, 1", "18446744073709551616", "invalid"] {
            for prefix in [false, true] {
                let value = if prefix {
                    HeadersOutput {
                        prefix: vec![("length".into(), length.into())],
                        ..Default::default()
                    }
                } else {
                    HeadersOutput {
                        length: Some(length.into()),
                        ..Default::default()
                    }
                };
                for (response, status) in [
                    (protocol.serialize_response(&OUTPUT, &value), 200),
                    (protocol.serialize_error(&value), 422),
                ] {
                    assert_eq!(response.status(), status);
                    assert_eq!(values(response.headers(), "content-length"), [length]);
                    assert!(response
                        .extensions()
                        .get::<aws_smithy_http_server::extension::RuntimeErrorExtension>()
                        .is_none());
                }
            }
        }
        let value = HeadersOutput {
            prefix: vec![("length".into(), "".into())],
            ..Default::default()
        };
        let response = protocol.serialize_response(&OUTPUT, &value);
        assert_eq!(response.status(), 200);
        assert_eq!(values(response.headers(), "content-length"), [""]);
    }
}

#[test]
fn modeled_length_collisions_match_legacy_generated_header_assembly() {
    for protocol in protocols() {
        for (prefix, explicit) in [("6", "6"), ("6", "006"), ("1", "0"), ("invalid", "-1")] {
            // Legacy HttpBindingGenerator appends modeled bindings, then
            // ServerHttpBoundProtocolGenerator adds payload.len() only if absent.
            let builder = http::Response::builder()
                .header(http::header::CONTENT_LENGTH, prefix)
                .header(http::header::CONTENT_LENGTH, explicit);
            let expected =
                aws_smithy_http::header::set_response_header_if_absent(builder, http::header::CONTENT_LENGTH, 123)
                    .body(())
                    .unwrap();
            let value = HeadersOutput {
                length: Some(explicit.into()),
                prefix: vec![("length".into(), prefix.into())],
                ..Default::default()
            };
            for (response, status) in [
                (protocol.serialize_response(&OUTPUT, &value), 200),
                (protocol.serialize_error(&value), 422),
                (protocol.serialize_streaming_response(&OUTPUT, &value, empty()), 200),
            ] {
                assert_eq!(response.status(), status);
                assert_eq!(
                    values(response.headers(), "content-length"),
                    values(expected.headers(), "content-length")
                );
            }
        }
    }
}

#[test]
fn modeled_content_type_overrides_defaults_without_losing_other_values() {
    for protocol in protocols() {
        for prefix in [false, true] {
            let mut value = HeadersOutput::default();
            if prefix {
                value.prefix.push(("type".into(), "text/custom".into()));
            } else {
                value.content_type = Some("text/custom".into());
            }
            for response in [
                protocol.serialize_response(&OUTPUT, &value),
                protocol.serialize_error(&value),
                protocol.serialize_streaming_response(&OUTPUT, &value, empty()),
            ] {
                assert_eq!(values(response.headers(), "content-type"), ["text/custom"]);
                assert_bound_headers(response.headers());
            }
        }
    }
}

#[test]
fn streaming_lengths_are_not_inferred_or_validated_and_bodies_are_not_polled() {
    for protocol in protocols() {
        let polls = Arc::new(AtomicUsize::new(0));
        let body = || {
            let polls = polls.clone();
            aws_smithy_http_server::body::wrap_stream(futures_util::stream::poll_fn(move |_| {
                polls.fetch_add(1, Ordering::SeqCst);
                std::task::Poll::<Option<Result<Bytes, std::io::Error>>>::Pending
            }))
        };
        let mut value = HeadersOutput::default();
        let response = protocol.serialize_streaming_response(&OUTPUT, &value, body());
        assert!(response.headers().get("content-length").is_none());
        value.length = Some("006".into());
        value.prefix.push(("length".into(), "6".into()));
        let response = protocol.serialize_streaming_response(&OUTPUT, &value, body());
        assert_eq!(response.status(), 200);
        assert_eq!(values(response.headers(), "content-length"), ["6", "006"]);
        assert_bound_headers(response.headers());
        for length in ["7", "invalid", "-1", "1, 1", "18446744073709551616", ""] {
            value.prefix.last_mut().unwrap().1 = length.into();
            let response = protocol.serialize_streaming_response(&OUTPUT, &value, body());
            assert_eq!(response.status(), 200);
            assert_eq!(values(response.headers(), "content-length"), [length, "006"]);
        }
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn empty_buffered_responses_emit_one_zero_length() {
    static EMPTY_LENGTH: Schema =
        Schema::new_member(shape_id!("test", "Empty", "length"), ShapeType::String, "length", 0)
            .with_http_header("content-length");
    static EMPTY: Schema = Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[&EMPTY_LENGTH]);
    struct EmptyOutput;
    impl SerializableStruct for EmptyOutput {
        fn schema(&self) -> &Schema<'_> {
            &EMPTY
        }
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&EMPTY_LENGTH, "0")
        }
    }
    for protocol in [
        Box::new(RestJson1Protocol::default()) as Box<dyn ServerProtocol>,
        Box::new(RestXmlProtocol::default()),
    ] {
        let response = protocol.serialize_response(&EMPTY, &EmptyOutput);
        assert_eq!(response.status(), 200);
        assert_eq!(values(response.headers(), "content-length"), ["0"]);
        assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
    }
}

#[tokio::test]
async fn hyper_preserves_lists_and_uses_one_consistent_content_length() {
    let mut value = HeadersOutput::default();
    let baseline = RestXmlProtocol::default().serialize_response(&OUTPUT, &value);
    let expected_body = baseline.into_body().collect().await.unwrap().to_bytes();
    value.length = Some(expected_body.len().to_string());
    let value = Arc::new(value);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let service = tower::service_fn(move |request: http::Request<hyper::body::Incoming>| {
        let mut value = value.as_ref().clone();
        if request.uri().path() == "/computed" {
            value.length = None;
        }
        let response = RestXmlProtocol::default().serialize_response(&OUTPUT, &value);
        async move { Ok::<_, Infallible>(response) }
    });
    let server = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();
    for path in ["/modeled", "/computed", "/modeled"] {
        let request = http::Request::builder()
            .uri(format!("http://{addr}{path}"))
            .body(http_body_util::Empty::<Bytes>::new())
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), client.request(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_bound_headers(response.headers());
        assert_eq!(
            values(response.headers(), "content-length"),
            [expected_body.len().to_string()]
        );
        assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), expected_body);
    }
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

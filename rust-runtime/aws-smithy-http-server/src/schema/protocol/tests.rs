/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::SharedServerProtocol;
use crate::protocol::rpc_v2_cbor::SMITHY_PROTOCOL_HEADER;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use http_body_util::BodyExt;
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::time::Duration;

use crate::protocol::test_helpers::get_body_as_string;
use crate::response::Response;
use crate::schema::protocol::AwsJson1_0Protocol;
use crate::schema::protocol::AwsJson1_1Protocol;
use crate::schema::protocol::RestJson1Protocol;
use crate::schema::protocol::RestXmlProtocol;
use crate::schema::protocol::RpcV2CborProtocol;
use crate::schema::{
    collect_request_body, DeserializableShape, DeserializeError, HttpModeledError, RequestBodyCollectionConfig,
    ServerProtocol,
};

static REST_JSON: LazyLock<RestJson1Protocol> = LazyLock::new(RestJson1Protocol::default);
static REST_XML: LazyLock<RestXmlProtocol> = LazyLock::new(RestXmlProtocol::default);
static AWS_JSON_10: LazyLock<AwsJson1_0Protocol> = LazyLock::new(AwsJson1_0Protocol::default);
static AWS_JSON_11: LazyLock<AwsJson1_1Protocol> = LazyLock::new(AwsJson1_1Protocol::default);
static RPC_V2_CBOR: LazyLock<RpcV2CborProtocol> = LazyLock::new(RpcV2CborProtocol::default);

#[test]
fn json_protocols_accept_verified_legacy_number_spellings() {
    static TIMES: Schema<'static> =
        Schema::new_member(shape_id!("test", "Numbers$times"), ShapeType::Integer, "times", 0);
    static INPUT: Schema<'static> = Schema::new_struct(shape_id!("test", "Numbers"), ShapeType::Structure, &[&TIMES]);
    for (protocol, content_type) in [
        (&*AWS_JSON_10 as &dyn ServerProtocol, "application/x-amz-json-1.0"),
        (&*AWS_JSON_11 as &dyn ServerProtocol, "application/x-amz-json-1.1"),
        (&*REST_JSON as &dyn ServerProtocol, "application/json"),
    ] {
        for (number, expected) in [
            ("214748364.", 214748364),
            ("0147483648", 147483648),
            ("0214748364\r", 214748364),
        ] {
            let body = format!(r#"{{"times":{number}}}"#);
            let req = request("/", &[("content-type", content_type)], body.as_bytes());
            let mut d = protocol.deserialize_request(&INPUT, &req).unwrap();
            let mut value = None;
            d.read_struct(&INPUT, &mut |member, d| {
                value = Some(d.read_integer(member)?);
                Ok(())
            })
            .unwrap();
            assert_eq!(value, Some(expected), "{content_type} {number}");
        }
        for body in [b"{\"times\":1e+}".as_slice(), b"{\"times\":1.5}", b"{} garbage"] {
            let req = request("/", &[("content-type", content_type)], body);
            let mut d = protocol.deserialize_request(&INPUT, &req).unwrap();
            assert!(d
                .read_struct(&INPUT, &mut |member, d| d.read_integer(member).map(|_| ()))
                .is_err());
        }
    }
}

// The operation's `@http` trait is transcribed onto the input and output schemas by codegen.
const PET_HTTP: HttpTrait<'static> = HttpTrait::new("POST", "/pets/{name}", Some(201));
const EMPTY_HTTP: HttpTrait<'static> = HttpTrait::new("POST", "/empty", None);

// --- a REST operation: `POST /pets/{name}?age=..` with a body member, `201` on success ---

static NAME_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "In", "name"), ShapeType::String, "name", 0).with_http_label();
static AGE_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "In", "age"), ShapeType::Integer, "age", 1).with_http_query("age");
static NOTE_MEMBER: Schema<'static> = Schema::new_member(shape_id!("test", "In", "note"), ShapeType::String, "note", 2);
static IN_MEMBERS: [&Schema<'static>; 3] = [&NAME_MEMBER, &AGE_MEMBER, &NOTE_MEMBER];
static IN_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS).with_http(PET_HTTP);

static OUT_MSG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Out", "msg"), ShapeType::String, "msg", 0);
static OUT_MEMBERS: [&Schema<'static>; 1] = [&OUT_MSG_MEMBER];
static OUT_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "Out"), ShapeType::Structure, &OUT_MEMBERS).with_http(PET_HTTP);

// --- an RPC operation whose input and output were modeled by the user ---

static RPC_NOTE_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "RpcIn", "note"), ShapeType::String, "note", 0);
static RPC_IN_MEMBERS: [&Schema<'static>; 1] = [&RPC_NOTE_MEMBER];
static RPC_IN_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "RpcIn"), ShapeType::Structure, &RPC_IN_MEMBERS).with_original_name("RpcIn");
static RPC_OUT_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "RpcOut"), ShapeType::Structure, &OUT_MEMBERS).with_original_name("RpcOut");

// --- synthetic (not user-modeled) empty input and output ---

static EMPTY_MEMBERS: [&Schema<'static>; 0] = [];
static EMPTY_IN_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "EmptyIn"), ShapeType::Structure, &EMPTY_MEMBERS).with_http(EMPTY_HTTP);
static EMPTY_OUT_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "EmptyOut"), ShapeType::Structure, &EMPTY_MEMBERS).with_http(EMPTY_HTTP);

// --- a user-modeled output with no members ---

static MODELED_EMPTY_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("test", "ModeledEmptyOut"),
    ShapeType::Structure,
    &EMPTY_MEMBERS,
)
.with_original_name("ModeledEmptyOut")
.with_http(EMPTY_HTTP);

#[derive(Debug, Default, PartialEq)]
struct TestInput {
    name: Option<String>,
    age: Option<i32>,
    note: Option<String>,
}

impl TestInput {
    fn walk(schema: &Schema<'_>, deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        let mut out = TestInput::default();
        deserializer.read_struct(schema, &mut |member, d| {
            match member.member_name() {
                Some("name") => out.name = Some(d.read_string(member)?),
                Some("age") => out.age = Some(d.read_integer(member)?),
                Some("note") => out.note = Some(d.read_string(member)?),
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }
}

impl DeserializableShape for TestInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        Self::walk(&IN_SCHEMA, deserializer)
    }
}

#[derive(Debug, Default, PartialEq)]
struct RpcTestInput(TestInput);

impl DeserializableShape for RpcTestInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        TestInput::walk(&RPC_IN_SCHEMA, deserializer).map(RpcTestInput)
    }
}

#[derive(Debug)]
struct EmptyInput;

impl DeserializableShape for EmptyInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        deserializer.read_struct(&EMPTY_IN_SCHEMA, &mut |_, _| Ok(()))?;
        Ok(EmptyInput)
    }
}

struct TestOutput;

impl SerializableStruct for TestOutput {
    fn schema(&self) -> &Schema<'_> {
        &OUT_SCHEMA
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&OUT_MSG_MEMBER, "ok")
    }
}

struct Nothing;

impl SerializableStruct for Nothing {
    fn schema(&self) -> &Schema<'_> {
        &EMPTY_OUT_SCHEMA
    }

    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

fn request(
    uri: &str,
    headers: &[(&'static str, &str)],
    body: &[u8],
) -> aws_smithy_runtime_api::http::Request<bytes::Bytes> {
    let mut builder = http::Request::builder().method("POST").uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let converted =
        aws_smithy_runtime_api::http::Request::try_from(builder.body(()).unwrap()).expect("valid test request");
    converted.map(|_| bytes::Bytes::copy_from_slice(body))
}

/// The request half of the upgrade: `Accept` gate, then deserialization of `input`.
/// Wraps an output schema in a throwaway operation descriptor for head validation. The
/// input side is irrelevant to the built-ins' `Accept` gate, so an empty input stands in.
fn head_op<'a>(output: &'a Schema<'a>) -> crate::schema::OperationSchema<'a> {
    crate::schema::OperationSchema::new(shape_id!("test", "HeadOp"), &EMPTY_IN_SCHEMA, output, &[])
}

/// Asks whether the protocol needs to collect the body for this input schema.
fn body_directive(protocol: &dyn ServerProtocol, input: &'static Schema<'static>) -> super::BodyDirective {
    let op = crate::schema::OperationSchema::new(shape_id!("test", "BodyOp"), input, &EMPTY_OUT_SCHEMA, &[]);
    protocol.request_body_requirement(&op)
}

fn deserialize<T: DeserializableShape>(
    protocol: &dyn ServerProtocol,
    input: &Schema<'_>,
    output: &Schema<'_>,
    request: &aws_smithy_runtime_api::http::Request<bytes::Bytes>,
) -> Result<T, DeserializeError> {
    protocol.validate_request_headers(&head_op(output), request.headers())?;
    let mut deserializer = protocol.deserialize_request(input, request)?;
    T::deserialize(&mut *deserializer)
}

async fn body_bytes(response: Response) -> bytes::Bytes {
    response.into_body().collect().await.expect("body collects").to_bytes()
}

#[test]
fn rest_request_bindings_route_labels_query_and_body() {
    let req = request(
        "/pets/rex?age=7",
        &[("content-type", "application/json")],
        br#"{"note":"hi"}"#,
    );
    let input: TestInput = deserialize(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).unwrap();
    assert_eq!(
        input,
        TestInput {
            name: Some("rex".to_string()),
            age: Some(7),
            note: Some("hi".to_string()),
        }
    );
}

#[test]
fn rest_request_content_type_is_checked_only_with_a_body() {
    let req = request("/pets/rex", &[("content-type", "text/xml")], b"{}");
    let err = deserialize::<TestInput>(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::UnsupportedMediaType(_)), "{err}");

    let req = request("/pets/rex", &[], b"");
    let input: TestInput = deserialize(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).unwrap();
    assert_eq!(input.name.as_deref(), Some("rex"));
    assert_eq!(input.note, None);
}

#[test]
fn rest_request_without_modeled_input_rejects_a_content_type() {
    let req = request("/empty", &[("content-type", "application/json")], b"");
    let err = deserialize::<EmptyInput>(&*REST_JSON, &EMPTY_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::UnsupportedMediaType(_)), "{err}");

    let req = request("/empty", &[], b"");
    deserialize::<EmptyInput>(&*REST_JSON, &EMPTY_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &req).unwrap();
}

#[test]
fn rest_request_wire_failures_are_serde_errors() {
    let req = request("/pets/rex?age=old", &[], b"");
    let err = deserialize::<TestInput>(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::Serde(_)), "{err}");
}

#[test]
fn rpc_request_round_trips_through_the_codec() {
    struct Body;
    impl SerializableStruct for Body {
        fn schema(&self) -> &Schema<'_> {
            &RPC_IN_SCHEMA
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&RPC_NOTE_MEMBER, "hi")
        }
    }
    let mut serializer = crate::schema::MetadataRoutedProtocol::event_stream_framing(&*RPC_V2_CBOR)
        .unwrap()
        .payload_codec
        .create_serializer();
    serializer.write_struct(&RPC_IN_SCHEMA, &Body).unwrap();
    let body = serializer.finish_boxed();

    let req = request(
        "/service/Svc/operation/Rpc",
        &[("content-type", "application/cbor")],
        &body,
    );
    let input: RpcTestInput = deserialize(&*RPC_V2_CBOR, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).unwrap();
    assert_eq!(input.0.note.as_deref(), Some("hi"));
}

#[test]
fn rpc_request_with_an_empty_body_leaves_members_unset() {
    let req = request("/", &[("content-type", "application/x-amz-json-1.0")], b"");
    let input: RpcTestInput = deserialize(&*AWS_JSON_10, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).unwrap();
    assert_eq!(input.0, TestInput::default());
}

// --- Accept-header gate ---

#[test]
fn accept_header_gates_every_protocol() {
    // REST: the PET output has a body member, so the expected type is the codec's.
    for accept in [
        "application/json",
        "application/*",
        "*/*",
        "text/xml, application/json;q=0.5",
    ] {
        let req = request("/pets/rex", &[("accept", accept)], b"");
        assert!(
            deserialize::<TestInput>(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).is_ok(),
            "accept: {accept}"
        );
    }
    let req = request("/pets/rex", &[("accept", "text/xml")], b"");
    let err = deserialize::<TestInput>(&*REST_JSON, &IN_SCHEMA, &OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");

    // restJson1 labels every response `application/json` unless the output says otherwise, and
    // gates `Accept` against that label even when the output has no body: the legacy server
    // generates the gate for `NoInputAndNoOutput`. restXml labels nothing and so gates nothing.
    let req = request("/empty", &[("accept", "text/xml")], b"");
    assert!(matches!(
        REST_JSON.validate_request_headers(&head_op(&EMPTY_OUT_SCHEMA), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));
    assert!(REST_XML
        .validate_request_headers(&head_op(&EMPTY_OUT_SCHEMA), req.headers())
        .is_ok());
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(REST_JSON
        .validate_request_headers(&head_op(&EMPTY_OUT_SCHEMA), req.headers())
        .is_ok());

    // awsJson: gated against the fixed protocol content type on every operation.
    let req = request("/", &[("accept", "application/x-amz-json-1.1")], b"");
    assert!(deserialize::<RpcTestInput>(&*AWS_JSON_11, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).is_ok());
    let req = request("/", &[("accept", "application/x-amz-json-1.0")], b"");
    let err = deserialize::<RpcTestInput>(&*AWS_JSON_11, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");
    assert!(matches!(
        AWS_JSON_11.validate_request_headers(&head_op(&EMPTY_OUT_SCHEMA), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));

    // rpcv2Cbor: gated only when the operation's output was modeled by the user.
    let req = request("/service/Svc/operation/Rpc", &[("accept", "text/plain")], b"");
    let err = deserialize::<RpcTestInput>(&*RPC_V2_CBOR, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");
    assert!(deserialize::<RpcTestInput>(&*RPC_V2_CBOR, &RPC_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &req).is_ok());
}

#[test]
fn accept_expectation_follows_the_output_payload() {
    static BLOB_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "BlobOut", "data"), ShapeType::Blob, "data", 0).with_http_payload();
    static BLOB_OUT_MEMBERS: [&Schema<'static>; 1] = [&BLOB_PAYLOAD];
    static BLOB_OUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BlobOut"), ShapeType::Structure, &BLOB_OUT_MEMBERS);
    static STRING_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "TextOut", "text"), ShapeType::String, "text", 0).with_http_payload();
    static STRING_OUT_MEMBERS: [&Schema<'static>; 1] = [&STRING_PAYLOAD];
    static STRING_OUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "TextOut"), ShapeType::Structure, &STRING_OUT_MEMBERS);

    // An untyped blob payload carries no content type on restJson1 (the legacy server sets
    // none), so nothing is gated; restXml labels it `application/octet-stream` and gates that.
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(REST_JSON
        .validate_request_headers(&head_op(&BLOB_OUT), req.headers())
        .is_ok());
    assert!(matches!(
        REST_XML.validate_request_headers(&head_op(&BLOB_OUT), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));
    let req = request("/empty", &[("accept", "application/octet-stream")], b"");
    assert!(REST_XML
        .validate_request_headers(&head_op(&BLOB_OUT), req.headers())
        .is_ok());

    // A string payload is `text/plain` everywhere.
    let req = request("/empty", &[("accept", "text/plain")], b"");
    assert!(REST_JSON
        .validate_request_headers(&head_op(&STRING_OUT), req.headers())
        .is_ok());
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(matches!(
        REST_JSON.validate_request_headers(&head_op(&STRING_OUT), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));
}

// --- streaming payloads ---

static EVENTS_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "StreamOut", "events"), ShapeType::Union, "events", 0)
        .with_http_payload()
        .with_streaming();
static STREAM_TAG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "StreamOut", "tag"), ShapeType::String, "tag", 1).with_http_header("x-tag");
static STREAM_OUT_MEMBERS: [&Schema<'static>; 2] = [&EVENTS_MEMBER, &STREAM_TAG_MEMBER];
static STREAM_OUT: Schema<'static> = Schema::new_struct(
    shape_id!("test", "StreamOut"),
    ShapeType::Structure,
    &STREAM_OUT_MEMBERS,
)
.with_original_name("StreamOut")
.with_http(HttpTrait::new("POST", "/stream", Some(202)));
static STREAM_NAME_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "StreamIn", "name"), ShapeType::String, "name", 1).with_http_label();
static STREAM_IN_MEMBERS: [&Schema<'static>; 2] = [&EVENTS_MEMBER, &STREAM_NAME_MEMBER];
static STREAM_IN: Schema<'static> =
    Schema::new_struct(shape_id!("test", "StreamIn"), ShapeType::Structure, &STREAM_IN_MEMBERS)
        .with_original_name("StreamIn")
        .with_http(HttpTrait::new("POST", "/stream/{name}", Some(202)));

struct StreamOutput;

impl SerializableStruct for StreamOutput {
    fn schema(&self) -> &Schema<'_> {
        &STREAM_OUT
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        // Generated outputs skip their streaming member.
        s.write_string(&STREAM_TAG_MEMBER, "tagged")
    }
}

#[test]
fn streaming_requests_are_never_collected_and_carry_no_content_type_check() {
    assert_eq!(body_directive(&*REST_JSON, &STREAM_IN), super::BodyDirective::Skip);
    assert_eq!(body_directive(&*REST_JSON, &IN_SCHEMA), super::BodyDirective::Collect);

    // The event stream request arrives with its own content type; nothing checks it, and the
    // URI bindings are still read.
    let req = request(
        "/stream/rex",
        &[("content-type", "application/vnd.amazon.eventstream")],
        b"",
    );
    let mut deserializer = REST_JSON.deserialize_request(&STREAM_IN, &req).unwrap();
    let input = TestInput::walk(&STREAM_IN, &mut *deserializer).unwrap();
    assert_eq!(input.name.as_deref(), Some("rex"));
}

#[test]
fn streaming_outputs_gate_accept_the_way_the_legacy_server_does() {
    // REST: against the event stream media type.
    let req = request("/stream", &[("accept", "application/vnd.amazon.eventstream")], b"");
    assert!(REST_JSON
        .validate_request_headers(&head_op(&STREAM_OUT), req.headers())
        .is_ok());
    let req = request("/stream", &[("accept", "application/json")], b"");
    assert!(matches!(
        REST_JSON.validate_request_headers(&head_op(&STREAM_OUT), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));

    // rpcv2Cbor: the event stream media type or, for compatibility with earlier servers, the
    // codec's; awsJson: the codec's only.
    let req = request("/stream", &[("accept", "application/cbor")], b"");
    assert!(RPC_V2_CBOR
        .validate_request_headers(&head_op(&STREAM_OUT), req.headers())
        .is_ok());
    let req = request("/stream", &[("accept", "application/vnd.amazon.eventstream")], b"");
    assert!(RPC_V2_CBOR
        .validate_request_headers(&head_op(&STREAM_OUT), req.headers())
        .is_ok());
    assert!(matches!(
        AWS_JSON_11.validate_request_headers(&head_op(&STREAM_OUT), req.headers()),
        Err(DeserializeError::NotAcceptable)
    ));
    let req = request("/stream", &[("accept", "application/x-amz-json-1.1")], b"");
    assert!(AWS_JSON_11
        .validate_request_headers(&head_op(&STREAM_OUT), req.headers())
        .is_ok());
}

#[tokio::test]
async fn streaming_responses_carry_the_head_only() {
    let body = || crate::body::to_boxed("frames");

    let response = REST_JSON.serialize_streaming_response(&STREAM_OUT, &StreamOutput, body());
    assert_eq!(response.status(), http::StatusCode::ACCEPTED);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/vnd.amazon.eventstream"
    );
    assert_eq!(response.headers().get("x-tag").unwrap(), "tagged");
    assert!(response.headers().get("content-length").is_none());
    assert_eq!(body_bytes(response).await.as_ref(), b"frames");

    // awsJson stamps its own content type on event stream responses; rpcv2Cbor the event stream
    // type plus its protocol header. Neither writes REST headers.
    let response = AWS_JSON_10.serialize_streaming_response(&STREAM_OUT, &StreamOutput, body());
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/x-amz-json-1.0"
    );
    assert_eq!(response.headers().get("x-tag").unwrap(), "tagged");
    let response = RPC_V2_CBOR.serialize_streaming_response(&STREAM_OUT, &StreamOutput, body());
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/vnd.amazon.eventstream"
    );
    assert_eq!(response.headers().get(&SMITHY_PROTOCOL_HEADER).unwrap(), "rpc-v2-cbor");
    assert_eq!(body_bytes(response).await.as_ref(), b"frames");
}

// --- body collection ---

#[tokio::test]
async fn rest_protocols_skip_the_body_when_nothing_is_bound_to_it() {
    static BOUND_MEMBERS: [&Schema<'static>; 2] = [&NAME_MEMBER, &AGE_MEMBER];
    static BOUND_ONLY: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BoundOnly"), ShapeType::Structure, &BOUND_MEMBERS);

    assert_eq!(body_directive(&*REST_JSON, &BOUND_ONLY), super::BodyDirective::Skip);
    assert_eq!(body_directive(&*REST_XML, &BOUND_ONLY), super::BodyDirective::Skip);
    assert_eq!(body_directive(&*REST_JSON, &IN_SCHEMA), super::BodyDirective::Collect);
    assert_eq!(
        body_directive(&*RPC_V2_CBOR, &BOUND_ONLY),
        super::BodyDirective::Collect
    );

    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"read"));
    let collected = collect_request_body(body, &RequestBodyCollectionConfig::default())
        .await
        .unwrap();
    assert_eq!(collected.as_ref(), b"read");
}

#[tokio::test]
async fn rpc_body_handling_is_decided_separately_from_mechanical_collection() {
    // The generated RPC deserializers never touch the body when the input has no members; the
    // RPC protocols answer `Skip` from `request_body_requirement` to mirror that.
    assert_eq!(
        body_directive(&*RPC_V2_CBOR, &EMPTY_IN_SCHEMA),
        super::BodyDirective::Skip
    );
    assert_eq!(
        body_directive(&*RPC_V2_CBOR, &RPC_IN_SCHEMA),
        super::BodyDirective::Collect
    );

    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"ignored"));
    let collected = collect_request_body(body, &RequestBodyCollectionConfig::default())
        .await
        .unwrap();
    assert_eq!(collected.as_ref(), b"ignored");
}

#[tokio::test]
async fn collection_enforces_limits_timeouts_and_body_errors() {
    let limited = RequestBodyCollectionConfig {
        max_bytes: NonZeroUsize::new(3),
        read_timeout: None,
        ..Default::default()
    };
    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"four"));
    assert!(matches!(
        collect_request_body(body, &limited).await,
        Err(super::RequestBodyCollectionError::TooLarge(_))
    ));

    let timed = RequestBodyCollectionConfig {
        max_bytes: None,
        read_timeout: Some(Duration::from_millis(1)),
        ..Default::default()
    };
    let body = http_body_util::StreamBody::new(futures_util::stream::pending::<
        Result<http_body::Frame<bytes::Bytes>, std::io::Error>,
    >());
    assert!(matches!(
        collect_request_body(body, &timed).await,
        Err(super::RequestBodyCollectionError::Timeout { .. })
    ));

    let body = http_body_util::StreamBody::new(futures_util::stream::once(async {
        Err::<http_body::Frame<bytes::Bytes>, _>(std::io::Error::other("broken"))
    }));
    assert!(matches!(
        collect_request_body(body, &RequestBodyCollectionConfig::default()).await,
        Err(super::RequestBodyCollectionError::Body(_))
    ));
}

#[test]
fn provided_methods_collect_the_body_and_gate_nothing() {
    #[derive(Debug, Default)]
    struct Minimal {
        codec: aws_smithy_json::codec::JsonCodec,
    }

    impl ServerProtocol for Minimal {
        fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
            static ID: aws_smithy_schema::ShapeId<'static> = shape_id!("test", "minimal");
            &ID
        }
        fn deserialize_request<'a>(
            &'a self,
            _input: &Schema<'_>,
            request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
        ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
            Ok(aws_smithy_schema::codec::DynCodec::create_deserializer(
                &self.codec,
                request.body(),
            ))
        }
        fn serialize_response(&self, _: &Schema<'_>, _: &dyn SerializableStruct) -> Response {
            unimplemented!()
        }
        fn serialize_streaming_response(
            &self,
            _: &Schema<'_>,
            _: &dyn SerializableStruct,
            _: crate::body::BoxBody,
        ) -> Response {
            unimplemented!()
        }
        fn serialize_error(&self, _: &dyn HttpModeledError) -> Response {
            unimplemented!()
        }
        fn serialize_rejection(&self, _: DeserializeError) -> Response {
            unimplemented!()
        }
    }

    let protocol = Minimal::default();
    assert_eq!(body_directive(&protocol, &RPC_IN_SCHEMA), super::BodyDirective::Collect);
    assert_eq!(
        body_directive(&protocol, &EMPTY_IN_SCHEMA),
        super::BodyDirective::Collect
    );

    let req = request("/", &[("accept", "text/xml")], b"");
    let erased: SharedServerProtocol = SharedServerProtocol::serde_only(protocol);
    assert!(erased
        .validate_request_headers(&head_op(&OUT_SCHEMA), req.headers())
        .is_ok());
    assert_eq!(
        body_directive(&*erased, &EMPTY_IN_SCHEMA),
        super::BodyDirective::Collect
    );
}

// --- responses ---

#[tokio::test]
async fn responses_take_the_status_from_the_output_schema() {
    let response = REST_JSON.serialize_response(&OUT_SCHEMA, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::CREATED);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    assert_eq!(get_body_as_string(response.into_body()).await, r#"{"msg":"ok"}"#);

    let response = RPC_V2_CBOR.serialize_response(&RPC_OUT_SCHEMA, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers().get(&SMITHY_PROTOCOL_HEADER).unwrap(), "rpc-v2-cbor");
    assert_eq!(response.headers().get("content-type").unwrap(), "application/cbor");
}

#[tokio::test]
async fn empty_outputs_follow_the_legacy_framing() {
    // A synthetic output: restJson1 stamps `application/json` on the empty body, the other
    // protocols follow their own rules.
    let response = REST_JSON.serialize_response(&EMPTY_OUT_SCHEMA, &Nothing);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    assert_eq!(response.headers().get("content-length").unwrap(), "0");
    let response = REST_XML.serialize_response(&EMPTY_OUT_SCHEMA, &Nothing);
    assert!(response.headers().get("content-type").is_none());
    let response = AWS_JSON_11.serialize_response(&EMPTY_OUT_SCHEMA, &Nothing);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/x-amz-json-1.1"
    );
    assert_eq!(get_body_as_string(response.into_body()).await, "");
    let response = RPC_V2_CBOR.serialize_response(&EMPTY_OUT_SCHEMA, &Nothing);
    assert!(response.headers().get("content-type").is_none());
    assert_eq!(response.headers().get(&SMITHY_PROTOCOL_HEADER).unwrap(), "rpc-v2-cbor");
    assert_eq!(body_bytes(response).await.len(), 0);

    // A user-modeled empty output is an empty document on the JSON and CBOR protocols and an
    // empty body on restXml.
    let response = REST_JSON.serialize_response(&MODELED_EMPTY_OUT_SCHEMA, &Nothing);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");
    let response = AWS_JSON_10.serialize_response(&MODELED_EMPTY_OUT_SCHEMA, &Nothing);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/x-amz-json-1.0"
    );
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");
    let response = RPC_V2_CBOR.serialize_response(&MODELED_EMPTY_OUT_SCHEMA, &Nothing);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/cbor");
    assert_eq!(body_bytes(response).await.as_ref(), &[0xbf, 0xff]);
    let response = REST_XML.serialize_response(&MODELED_EMPTY_OUT_SCHEMA, &Nothing);
    assert!(response.headers().get("content-type").is_none());
    assert_eq!(get_body_as_string(response.into_body()).await, "");
}

#[tokio::test]
async fn payload_outputs_are_labeled_from_the_schema_not_the_value() {
    static BLOB_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "BlobOut", "data"), ShapeType::Blob, "data", 0).with_http_payload();
    static BLOB_OUT_MEMBERS: [&Schema<'static>; 1] = [&BLOB_PAYLOAD];
    static BLOB_OUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BlobOut"), ShapeType::Structure, &BLOB_OUT_MEMBERS)
            .with_original_name("BlobOut");
    static STRING_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "TextOut", "text"), ShapeType::String, "text", 0).with_http_payload();
    static STRING_OUT_MEMBERS: [&Schema<'static>; 1] = [&STRING_PAYLOAD];
    static STRING_OUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "TextOut"), ShapeType::Structure, &STRING_OUT_MEMBERS)
            .with_original_name("TextOut");

    struct Text(Option<&'static str>);
    impl SerializableStruct for Text {
        fn schema(&self) -> &Schema<'_> {
            &STRING_OUT
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            match self.0 {
                Some(text) => s.write_string(&STRING_PAYLOAD, text),
                None => Ok(()),
            }
        }
    }
    struct Data(Option<&'static [u8]>);
    impl SerializableStruct for Data {
        fn schema(&self) -> &Schema<'_> {
            &BLOB_OUT
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            match self.0 {
                Some(data) => s.write_blob(&BLOB_PAYLOAD, aws_smithy_types::Blob::new(data)),
                None => Ok(()),
            }
        }
    }

    // The legacy restJson1 server labels a string payload `text/plain` whether or not it is set,
    // and never labels an untyped blob payload.
    let response = REST_JSON.serialize_response(&STRING_OUT, &Text(Some("hello")));
    assert_eq!(response.headers().get("content-type").unwrap(), "text/plain");
    assert_eq!(get_body_as_string(response.into_body()).await, "hello");
    let response = REST_JSON.serialize_response(&STRING_OUT, &Text(None));
    assert_eq!(response.headers().get("content-type").unwrap(), "text/plain");
    assert_eq!(get_body_as_string(response.into_body()).await, "");
    let response = REST_JSON.serialize_response(&BLOB_OUT, &Data(Some(b"\x01\x02")));
    assert!(response.headers().get("content-type").is_none());
    assert_eq!(body_bytes(response).await.as_ref(), b"\x01\x02");

    // restXml labels the same blob `application/octet-stream`.
    let response = REST_XML.serialize_response(&BLOB_OUT, &Data(None));
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/octet-stream"
    );
    assert_eq!(body_bytes(response).await.len(), 0);
}

// --- a modeled error with one body member and one header-bound member ---

static BOOM_MSG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Boom", "message"), ShapeType::String, "message", 0);
static BOOM_HDR_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Boom", "tag"), ShapeType::String, "tag", 1).with_http_header("x-boom-tag");
static BOOM_MEMBERS: [&Schema<'static>; 2] = [&BOOM_MSG_MEMBER, &BOOM_HDR_MEMBER];
static ERROR_TRAITS: std::sync::LazyLock<aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
    let mut traits = aws_smithy_schema::TraitMap::new();
    traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});
static BOOM_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "Boom"), ShapeType::Structure, &BOOM_MEMBERS).with_traits(&ERROR_TRAITS);

#[derive(Debug)]
struct Boom;

impl std::fmt::Display for Boom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("boom happened")
    }
}

impl std::error::Error for Boom {}

impl SerializableStruct for Boom {
    fn schema(&self) -> &Schema<'_> {
        &BOOM_SCHEMA
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&BOOM_MSG_MEMBER, "boom happened")?;
        s.write_string(&BOOM_HDR_MEMBER, "tagged")
    }
}

impl HttpModeledError for Boom {
    fn status_code(&self) -> u16 {
        422
    }
}

#[tokio::test]
async fn rest_json_1_frames_errors_with_the_header_discriminator() {
    let response = REST_JSON.serialize_error(&Boom);
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
    assert_eq!(response.headers().get("x-boom-tag").unwrap(), "tagged");
    assert_eq!(
        get_body_as_string(response.into_body()).await,
        r#"{"message":"boom happened"}"#
    );
}

#[tokio::test]
async fn rest_xml_frames_errors_without_a_discriminator() {
    let response = REST_XML.serialize_error(&Boom);
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/xml");
    assert_eq!(response.headers().get("x-boom-tag").unwrap(), "tagged");
    let body = get_body_as_string(response.into_body()).await;
    assert!(body.contains("<message>boom happened</message>"), "{body}");
    assert!(!body.contains("tagged"), "{body}");
}

#[tokio::test]
async fn aws_json_frames_errors_with_a_trailing_type_member() {
    // RPC protocols do not split header-bound members out of the body.
    let body = get_body_as_string(AWS_JSON_10.serialize_error(&Boom).into_body()).await;
    assert!(body.contains(r#""tag":"tagged""#), "{body}");
    assert!(body.ends_with(r#""__type":"test#Boom"}"#), "{body}");

    let body = get_body_as_string(AWS_JSON_11.serialize_error(&Boom).into_body()).await;
    assert!(body.ends_with(r#""__type":"Boom"}"#), "{body}");
}

#[tokio::test]
async fn rpc_v2_cbor_frames_errors_with_a_leading_type_member() {
    let response = RPC_V2_CBOR.serialize_error(&Boom);
    assert_eq!(response.headers().get(&SMITHY_PROTOCOL_HEADER).unwrap(), "rpc-v2-cbor");
    let bytes = body_bytes(response).await;
    let type_pos = bytes.windows(6).position(|w| w == b"__type").expect("__type present");
    let msg_pos = bytes.windows(7).position(|w| w == b"message").expect("message present");
    assert!(type_pos < msg_pos, "__type must be the first map entry");
    assert!(bytes.windows(9).any(|w| w == b"test#Boom"), "full shape ID present");
}

// --- rejection responses: byte-for-byte the legacy `RuntimeError` responses ---

fn media_type_failure() -> DeserializeError {
    DeserializeError::from(crate::rejection::MissingContentTypeReason::UnexpectedMimeType {
        expected_mime: None,
        found_mime: None,
    })
}

fn serde_failure() -> DeserializeError {
    DeserializeError::Serde(SerdeError::custom("bad"))
}

#[tokio::test]
async fn rest_json_1_rejections_keep_the_distinct_statuses() {
    let response = REST_JSON.serialize_rejection(serde_failure());
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    assert_eq!(
        response.headers().get("x-amzn-errortype").unwrap(),
        "SerializationException"
    );
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");

    let response = REST_JSON.serialize_rejection(media_type_failure());
    assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        response.headers().get("x-amzn-errortype").unwrap(),
        "UnsupportedMediaTypeException"
    );
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");

    let response = REST_JSON.serialize_rejection(DeserializeError::NotAcceptable);
    assert_eq!(response.status(), http::StatusCode::NOT_ACCEPTABLE);
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");
}

#[tokio::test]
async fn rest_xml_rejections_collapse_not_acceptable_to_a_400() {
    let response = REST_XML.serialize_rejection(media_type_failure());
    assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/xml");
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");

    // restXml's `From<RequestRejection>` has no `NotAcceptable` arm: legacy falls through to a
    // 400 `Serialization`, not a 406.
    let response = REST_XML.serialize_rejection(DeserializeError::NotAcceptable);
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");
}

#[tokio::test]
async fn aws_json_rejections_collapse_everything_to_a_400() {
    for err in [serde_failure(), media_type_failure(), DeserializeError::NotAcceptable] {
        let response = AWS_JSON_10.serialize_rejection(err);
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/x-amz-json-1.0"
        );
        assert_eq!(get_body_as_string(response.into_body()).await, "{}");
    }
    for err in [serde_failure(), media_type_failure(), DeserializeError::NotAcceptable] {
        let response = AWS_JSON_11.serialize_rejection(err);
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/x-amz-json-1.1"
        );
        // awsJson1.1 answers with an empty body, not `{}`.
        assert_eq!(get_body_as_string(response.into_body()).await, "");
    }
}

#[tokio::test]
async fn rpc_v2_cbor_rejections_collapse_to_a_400_without_the_protocol_header() {
    for err in [serde_failure(), media_type_failure(), DeserializeError::NotAcceptable] {
        let response = RPC_V2_CBOR.serialize_rejection(err);
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        assert_eq!(response.headers().get("content-type").unwrap(), "application/cbor");
        // Legacy never sets `smithy-protocol` on the runtime-error path.
        assert!(response.headers().get(&SMITHY_PROTOCOL_HEADER).is_none());
        // The body is an empty CBOR map with no `__type` (upstream #3716, preserved).
        assert_eq!(body_bytes(response).await.as_ref(), &[0xa0]);
    }
}

#[tokio::test]
async fn constraint_violations_answer_with_the_modeled_error() {
    use crate::extension::{ModeledErrorExtension, RuntimeErrorExtension};

    let response = REST_JSON.serialize_rejection(DeserializeError::ConstraintViolation(Box::new(Boom)));
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
    assert_eq!(**response.extensions().get::<ModeledErrorExtension>().unwrap(), "Boom");
    assert_eq!(
        **response.extensions().get::<RuntimeErrorExtension>().unwrap(),
        "ValidationException"
    );
    assert_eq!(
        get_body_as_string(response.into_body()).await,
        r#"{"message":"boom happened"}"#
    );

    // awsJson validation bodies carry the `__type` discriminator, exactly as the generated
    // smuggled payloads do (full ID on 1.0, shape name on 1.1, both written last).
    let response = AWS_JSON_10.serialize_rejection(DeserializeError::ConstraintViolation(Box::new(Boom)));
    let body = get_body_as_string(response.into_body()).await;
    assert!(body.ends_with(r#""__type":"test#Boom"}"#), "{body}");
    let response = AWS_JSON_11.serialize_rejection(DeserializeError::ConstraintViolation(Box::new(Boom)));
    let body = get_body_as_string(response.into_body()).await;
    assert!(body.ends_with(r#""__type":"Boom"}"#), "{body}");
}

#[tokio::test]
async fn rpc_v2_cbor_constraint_violations_have_no_protocol_header() {
    use crate::extension::RuntimeErrorExtension;

    // Modeled CBOR body with a leading `__type`, but, unlike handler-returned errors, no
    // `smithy-protocol` header: legacy's runtime-error path never sets it.
    let response = RPC_V2_CBOR.serialize_rejection(DeserializeError::ConstraintViolation(Box::new(Boom)));
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.headers().get(&SMITHY_PROTOCOL_HEADER).is_none());
    assert_eq!(
        **response.extensions().get::<RuntimeErrorExtension>().unwrap(),
        "ValidationException"
    );
    let bytes = body_bytes(response).await;
    let type_pos = bytes.windows(6).position(|w| w == b"__type").expect("__type present");
    let msg_pos = bytes.windows(7).position(|w| w == b"message").expect("message present");
    assert!(type_pos < msg_pos, "__type must be the first map entry");
}

#[tokio::test]
async fn rest_xml_constraint_violations_reproduce_the_legacy_empty_body() {
    use crate::extension::RuntimeErrorExtension;

    // Legacy restXml drops the validation body entirely: 400, `application/xml`, literal `{}`.
    let response = REST_XML.serialize_rejection(DeserializeError::ConstraintViolation(Box::new(Boom)));
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/xml");
    assert_eq!(
        **response.extensions().get::<RuntimeErrorExtension>().unwrap(),
        "ValidationException"
    );
    assert_eq!(get_body_as_string(response.into_body()).await, "{}");
}

// --- the erased handle ---

#[test]
fn every_protocol_erases_to_a_dyn_server_protocol() {
    let protocols: Vec<(SharedServerProtocol, &str, bool)> = vec![
        (
            SharedServerProtocol::metadata_routed(RestJson1Protocol::default()),
            "aws.protocols#restJson1",
            false,
        ),
        (
            SharedServerProtocol::metadata_routed(RestXmlProtocol::default()),
            "aws.protocols#restXml",
            false,
        ),
        (
            SharedServerProtocol::metadata_routed(AwsJson1_0Protocol::default()),
            "aws.protocols#awsJson1_0",
            true,
        ),
        (
            SharedServerProtocol::metadata_routed(AwsJson1_1Protocol::default()),
            "aws.protocols#awsJson1_1",
            true,
        ),
        (
            SharedServerProtocol::metadata_routed(RpcV2CborProtocol::default()),
            "smithy.protocols#rpcv2Cbor",
            true,
        ),
    ];
    for (protocol, id, frames) in &protocols {
        assert_eq!(protocol.protocol_id().as_str(), *id);
        assert_eq!(
            protocol.event_stream_framing().unwrap().initial_messages_in_frames,
            *frames,
            "{id}"
        );
        assert!(!protocol.event_stream_framing().unwrap().media_type.is_empty(), "{id}");
    }

    let erased: SharedServerProtocol = SharedServerProtocol::metadata_routed(RestJson1Protocol::default());
    let req = request(
        "/pets/rex?age=7",
        &[("content-type", "application/json")],
        br#"{"note":"hi"}"#,
    );
    let mut deserializer = erased.deserialize_request(&IN_SCHEMA, &req).ok().unwrap();
    let input = TestInput::deserialize(&mut *deserializer).unwrap();
    assert_eq!(input.age, Some(7));

    // A failure travels as `DeserializeError` on the erased path too, and the erased protocol
    // renders it.
    let req = request("/pets/rex", &[("content-type", "text/xml")], b"{}");
    let err = erased.deserialize_request(&IN_SCHEMA, &req).err().unwrap();
    let response = erased.serialize_rejection(err);
    assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let mut serializer = erased.event_stream_framing().unwrap().payload_codec.create_serializer();
    serializer.write_struct(&OUT_SCHEMA, &TestOutput).unwrap();
    assert_eq!(serializer.finish_boxed(), br#"{"msg":"ok"}"#);
}

/// A struct serialized from middleware with a hand-written schema is framed exactly like an
/// operation output with the same schema, on every protocol.
#[tokio::test]
async fn middleware_structs_are_framed_like_operation_outputs() {
    static TEAPOT_MSG: Schema<'static> =
        Schema::new_member(shape_id!("test", "Teapot", "message"), ShapeType::String, "message", 0);
    static TEAPOT_TAG: Schema<'static> =
        Schema::new_member(shape_id!("test", "Teapot", "tag"), ShapeType::String, "tag", 1).with_http_header("x-tag");
    static TEAPOT_MEMBERS: [&Schema<'static>; 2] = [&TEAPOT_MSG, &TEAPOT_TAG];
    static TEAPOT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Teapot"), ShapeType::Structure, &TEAPOT_MEMBERS)
            .with_original_name("Teapot")
            .with_http(HttpTrait::new("GET", "/teapot", Some(418)));

    struct Teapot;
    impl SerializableStruct for Teapot {
        fn schema(&self) -> &Schema<'_> {
            &TEAPOT
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&TEAPOT_MSG, "short and stout")?;
            s.write_string(&TEAPOT_TAG, "brewing")
        }
    }

    let protocols: Vec<SharedServerProtocol> = vec![
        SharedServerProtocol::metadata_routed(RestJson1Protocol::default()),
        SharedServerProtocol::metadata_routed(RestXmlProtocol::default()),
        SharedServerProtocol::metadata_routed(AwsJson1_0Protocol::default()),
        SharedServerProtocol::metadata_routed(AwsJson1_1Protocol::default()),
        SharedServerProtocol::metadata_routed(RpcV2CborProtocol::default()),
    ];
    for protocol in protocols {
        let id = protocol.protocol_id().as_str();
        let (status, headers, body) = {
            let response = protocol.serialize_response(&TEAPOT, &Teapot);
            let (parts, body) = response.into_parts();
            (parts.status, parts.headers, body.collect().await.unwrap().to_bytes())
        };
        assert!(!body.is_empty(), "{id}");
        assert!(headers.contains_key("content-type"), "{id}");
        if id.starts_with("aws.protocols#rest") {
            assert_eq!(status, http::StatusCode::IM_A_TEAPOT, "{id}");
            assert_eq!(headers.get("x-tag").unwrap(), "brewing", "{id}");
        } else {
            // AWS JSON honors modeled success statuses; CBOR ignores HTTP bindings.
            let expected = if id.starts_with("aws.protocols#awsJson") {
                http::StatusCode::IM_A_TEAPOT
            } else {
                http::StatusCode::OK
            };
            assert_eq!(status, expected, "{id}");
            assert_eq!(
                headers.contains_key("x-tag"),
                id.starts_with("aws.protocols#awsJson"),
                "{id}"
            );
        }

        // The same struct serialized as the output of an operation with the same schema.
        let again = protocol.serialize_response(&TEAPOT, &Teapot);
        let (again_parts, again_body) = again.into_parts();
        assert_eq!(again_parts.status, status, "{id}");
        assert_eq!(again_parts.headers, headers, "{id}");
        assert_eq!(again_body.collect().await.unwrap().to_bytes(), body, "{id}");
    }
}

// --- a restXml operation with a wrapped list and a wrapped map in the body ---

const COLLECTIONS_HTTP: HttpTrait<'static> = HttpTrait::new("POST", "/collections", None);
static TAG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Tags", "member"), ShapeType::String, "member", 0);
static TAGS_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Collections", "tags"), ShapeType::List, "tags", 0)
        .with_list_member(&TAG_MEMBER);
static ATTR_KEY: Schema<'static> = Schema::new_member(shape_id!("test", "Attrs", "key"), ShapeType::String, "key", 0);
static ATTR_VALUE: Schema<'static> =
    Schema::new_member(shape_id!("test", "Attrs", "value"), ShapeType::String, "value", 1);
static ATTRS_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Collections", "attrs"), ShapeType::Map, "attrs", 1)
        .with_map_members(&ATTR_KEY, &ATTR_VALUE);
static COLLECTIONS_MEMBERS: [&Schema<'static>; 2] = [&TAGS_MEMBER, &ATTRS_MEMBER];
static COLLECTIONS_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("test", "Collections"),
    ShapeType::Structure,
    &COLLECTIONS_MEMBERS,
)
.with_http(COLLECTIONS_HTTP);
static COLLECTIONS_SERVICE: crate::schema::ServiceSchema<'static> = crate::schema::ServiceSchema::new(
    shape_id!("test", "CollectionsService"),
    None,
    &[shape_id!("aws.protocols", "restXml")],
    &[],
);

#[derive(Debug, Default, PartialEq)]
struct Collections {
    tags: Vec<String>,
    attrs: Vec<(String, String)>,
}

impl DeserializableShape for Collections {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        let mut out = Collections::default();
        deserializer.read_struct(&COLLECTIONS_SCHEMA, &mut |member, d| {
            match member.member_name() {
                Some("tags") => out.tags = d.read_string_list(member)?,
                Some("attrs") => out.attrs = d.read_string_string_map(member)?.into_iter().collect(),
                _ => {}
            }
            Ok(())
        })?;
        out.attrs.sort();
        Ok(out)
    }
}

fn rest_xml_with_settings(settings: Option<&str>) -> Result<RestXmlProtocol, crate::schema::routing::RouterBuildError> {
    use crate::schema::protocol::MetadataRoutedProtocol;
    let settings = settings.map(|json| crate::schema::settings::parse_settings_json(json.as_bytes()));
    RestXmlProtocol::from_build_context(
        &crate::schema::ProtocolBuildContext::new(&COLLECTIONS_SERVICE).with_settings(settings.as_ref()),
    )
}

/// Collection-name validation is independent of document validation and media-type aliases.
#[test]
fn rest_xml_collection_names_are_a_codec_setting() {
    let req = request(
        "/collections",
        &[("content-type", "application/xml")],
        b"<Collections>\
            <tags><item>a</item><member>b</member></tags>\
            <attrs>\
              <item><key>x</key><value>9</value></item>\
              <entry><key>a</key><value>1</value></entry>\
            </attrs>\
          </Collections>",
    );
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    let every_child = Collections {
        tags: vec!["a".to_owned(), "b".to_owned()],
        attrs: vec![pair("a", "1"), pair("x", "9")],
    };
    let named_children = Collections {
        tags: vec!["b".to_owned()],
        attrs: vec![pair("a", "1")],
    };
    for (settings, expected) in [
        (None, &named_children),
        (Some("{}"), &named_children),
        (Some(r#"{"strictCollectionElementNames":false}"#), &every_child),
        (Some(r#"{"strictCollectionElementNames":true}"#), &named_children),
    ] {
        let protocol = rest_xml_with_settings(settings).unwrap();
        let input: Collections = deserialize(&protocol, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &req).unwrap();
        assert_eq!(&input, expected, "{settings:?}");
    }
    let default: Collections = deserialize(&*REST_XML, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &req).unwrap();
    assert_eq!(default, named_children);
}

#[test]
fn rest_xml_collection_name_setting_must_be_a_boolean() {
    for settings in [r#"{"strictCollectionElementNames":"yes"}"#, r#""not an object""#] {
        assert!(
            matches!(
                rest_xml_with_settings(Some(settings)),
                Err(crate::schema::routing::RouterBuildError::Configuration(_))
            ),
            "{settings}"
        );
    }
}

/// Media-type aliases do not change XML parsing strictness.
#[test]
fn rest_xml_text_xml_is_a_separate_protocol_setting() {
    let body = b"<Collections><tags><member>a</member></tags></Collections>";
    let read = |protocol: &dyn ServerProtocol, content_type: &str| {
        let req = request("/collections", &[("content-type", content_type)], body);
        deserialize::<Collections>(protocol, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &req)
    };
    assert_eq!(read(&*REST_XML, "application/xml").unwrap().tags, ["a"]);
    let aliases = rest_xml_with_settings(Some(r#"{"acceptTextXml":true}"#)).unwrap();
    for content_type in ["text/xml", "text/xml; charset=utf-8", "TEXT/XML"] {
        assert!(matches!(
            read(&*REST_XML, content_type),
            Err(DeserializeError::UnsupportedMediaType(_))
        ));
        assert_eq!(read(&aliases, content_type).unwrap().tags, ["a"]);
    }
    for content_type in ["text/plain", "application/json", "application/atom+xml"] {
        assert!(matches!(
            read(&aliases, content_type),
            Err(DeserializeError::UnsupportedMediaType(_))
        ));
    }
}

#[test]
fn json_skipped_value_setting_must_be_a_boolean() {
    use crate::schema::protocol::MetadataRoutedProtocol;
    for json in [
        r#"{"validateSkippedValues":"yes"}"#,
        r#"{"validateSkippedValues":1}"#,
        r#"{"validateSkippedValues":null}"#,
        r#""not an object""#,
    ] {
        let settings = crate::schema::settings::parse_settings_json(json.as_bytes());
        let context = crate::schema::ProtocolBuildContext::new(&COLLECTIONS_SERVICE).with_settings(Some(&settings));
        assert!(matches!(
            AwsJson1_0Protocol::from_build_context(&context),
            Err(crate::schema::routing::RouterBuildError::Configuration(_))
        ));
        assert!(matches!(
            AwsJson1_1Protocol::from_build_context(&context),
            Err(crate::schema::routing::RouterBuildError::Configuration(_))
        ));
        assert!(matches!(
            RestJson1Protocol::from_build_context(&context),
            Err(crate::schema::routing::RouterBuildError::Configuration(_))
        ));
    }
}

#[test]
fn rest_xml_document_validation_is_independent_of_root_and_collection_names() {
    let strict = rest_xml_with_settings(Some(r#"{"validateDocument":true}"#)).unwrap();
    for body in [
        "<Collections><tags><member>a</member></tags></Collections>junk",
        "<Collections><tags><member>a</member></tags></Collections><Collections/>",
        "<Collections><tags><member>a</member></tags></Collections></Collections>",
        "<Collections><tags><member>a</member></tags>",
        "<Collections><tags><member>a</wrong></tags></Collections>",
        "<Collections><tags><member>a</mem",
    ] {
        let req = request("/collections", &[("content-type", "application/xml")], body.as_bytes());
        let parsed = deserialize::<Collections>(&*REST_XML, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &req).unwrap();
        assert_eq!(parsed.tags, ["a"], "{body}");
        assert!(
            deserialize::<Collections>(&strict, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &req).is_err(),
            "{body}"
        );
    }
    let wrong_root = request("/collections", &[("content-type", "application/xml")], b"<Wrong/>");
    assert!(deserialize::<Collections>(&*REST_XML, &COLLECTIONS_SCHEMA, &EMPTY_OUT_SCHEMA, &wrong_root).is_err());
}

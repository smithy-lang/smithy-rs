/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::SharedServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use http_body_util::BodyExt;
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::time::Duration;

use crate::protocol::aws_json_10::AwsJson1_0Protocol;
use crate::protocol::aws_json_11::AwsJson1_1Protocol;
use crate::protocol::rest_json_1::RestJson1Protocol;
use crate::protocol::rest_xml::RestXmlProtocol;
use crate::protocol::rpc_v2_cbor::RpcV2CborProtocol;
use crate::protocol::test_helpers::get_body_as_string;
use crate::response::Response;
use crate::schema::{
    collect_request_body, DeserializableShape, DeserializeError, HttpModeledError, ModeledError,
    RequestBodyCollectionConfig, ServerProtocol, ServerRequest,
};

static REST_JSON: LazyLock<RestJson1Protocol> = LazyLock::new(RestJson1Protocol::default);
static REST_XML: LazyLock<RestXmlProtocol> = LazyLock::new(RestXmlProtocol::default);
static AWS_JSON_10: LazyLock<AwsJson1_0Protocol> = LazyLock::new(AwsJson1_0Protocol::default);
static AWS_JSON_11: LazyLock<AwsJson1_1Protocol> = LazyLock::new(AwsJson1_1Protocol::default);
static RPC_V2_CBOR: LazyLock<RpcV2CborProtocol> = LazyLock::new(RpcV2CborProtocol::default);

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
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&OUT_MSG_MEMBER, "ok")
    }
}

struct Nothing;

impl SerializableStruct for Nothing {
    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

fn request(uri: &str, headers: &[(&'static str, &str)], body: &[u8]) -> ServerRequest {
    let mut builder = http::Request::builder().method("POST").uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let converted =
        aws_smithy_runtime_api::http::Request::try_from(builder.body(()).unwrap()).expect("valid test request");
    let parts = converted.into_parts();
    ServerRequest {
        uri: parts.uri,
        headers: parts.headers,
        body: bytes::Bytes::copy_from_slice(body),
    }
}

/// The request half of the upgrade: `Accept` gate, then deserialization of `input`.
fn deserialize<T: DeserializableShape>(
    protocol: &dyn ServerProtocol,
    input: &Schema<'_>,
    output: &Schema<'_>,
    request: &ServerRequest,
) -> Result<T, DeserializeError> {
    protocol.check_accept(output, &request.headers)?;
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
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&RPC_NOTE_MEMBER, "hi")
        }
    }
    let mut serializer = RPC_V2_CBOR.event_stream().unwrap().payload_codec().create_serializer();
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
        REST_JSON.check_accept(&EMPTY_OUT_SCHEMA, &req.headers),
        Err(DeserializeError::NotAcceptable)
    ));
    assert!(REST_XML.check_accept(&EMPTY_OUT_SCHEMA, &req.headers).is_ok());
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(REST_JSON.check_accept(&EMPTY_OUT_SCHEMA, &req.headers).is_ok());

    // awsJson: gated against the fixed protocol content type on every operation.
    let req = request("/", &[("accept", "application/x-amz-json-1.1")], b"");
    assert!(deserialize::<RpcTestInput>(&*AWS_JSON_11, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).is_ok());
    let req = request("/", &[("accept", "application/x-amz-json-1.0")], b"");
    let err = deserialize::<RpcTestInput>(&*AWS_JSON_11, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");
    assert!(matches!(
        AWS_JSON_11.check_accept(&EMPTY_OUT_SCHEMA, &req.headers),
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
    assert!(REST_JSON.check_accept(&BLOB_OUT, &req.headers).is_ok());
    assert!(matches!(
        REST_XML.check_accept(&BLOB_OUT, &req.headers),
        Err(DeserializeError::NotAcceptable)
    ));
    let req = request("/empty", &[("accept", "application/octet-stream")], b"");
    assert!(REST_XML.check_accept(&BLOB_OUT, &req.headers).is_ok());

    // A string payload is `text/plain` everywhere.
    let req = request("/empty", &[("accept", "text/plain")], b"");
    assert!(REST_JSON.check_accept(&STRING_OUT, &req.headers).is_ok());
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(matches!(
        REST_JSON.check_accept(&STRING_OUT, &req.headers),
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
static STREAM_IN_MEMBERS: [&Schema<'static>; 2] = [&EVENTS_MEMBER, &NAME_MEMBER];
static STREAM_IN: Schema<'static> =
    Schema::new_struct(shape_id!("test", "StreamIn"), ShapeType::Structure, &STREAM_IN_MEMBERS)
        .with_original_name("StreamIn")
        .with_http(HttpTrait::new("POST", "/stream/{name}", Some(202)));

struct StreamOutput;

impl SerializableStruct for StreamOutput {
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        // Generated outputs skip their streaming member.
        s.write_string(&STREAM_TAG_MEMBER, "tagged")
    }
}

#[test]
fn streaming_requests_are_never_collected_and_carry_no_content_type_check() {
    assert!(!REST_JSON.reads_request_body(&STREAM_IN));
    assert!(REST_JSON.reads_request_body(&IN_SCHEMA));

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
    assert!(REST_JSON.check_accept(&STREAM_OUT, &req.headers).is_ok());
    let req = request("/stream", &[("accept", "application/json")], b"");
    assert!(matches!(
        REST_JSON.check_accept(&STREAM_OUT, &req.headers),
        Err(DeserializeError::NotAcceptable)
    ));

    // rpcv2Cbor: the event stream media type or, for compatibility with earlier servers, the
    // codec's; awsJson: the codec's only.
    let req = request("/stream", &[("accept", "application/cbor")], b"");
    assert!(RPC_V2_CBOR.check_accept(&STREAM_OUT, &req.headers).is_ok());
    let req = request("/stream", &[("accept", "application/vnd.amazon.eventstream")], b"");
    assert!(RPC_V2_CBOR.check_accept(&STREAM_OUT, &req.headers).is_ok());
    assert!(matches!(
        AWS_JSON_11.check_accept(&STREAM_OUT, &req.headers),
        Err(DeserializeError::NotAcceptable)
    ));
    let req = request("/stream", &[("accept", "application/x-amz-json-1.1")], b"");
    assert!(AWS_JSON_11.check_accept(&STREAM_OUT, &req.headers).is_ok());
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
    assert!(response.headers().get("x-tag").is_none());
    let response = RPC_V2_CBOR.serialize_streaming_response(&STREAM_OUT, &StreamOutput, body());
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/vnd.amazon.eventstream"
    );
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
    assert_eq!(body_bytes(response).await.as_ref(), b"frames");
}

// --- body collection ---

#[tokio::test]
async fn rest_protocols_skip_the_body_when_nothing_is_bound_to_it() {
    static BOUND_MEMBERS: [&Schema<'static>; 2] = [&NAME_MEMBER, &AGE_MEMBER];
    static BOUND_ONLY: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BoundOnly"), ShapeType::Structure, &BOUND_MEMBERS);

    assert!(!REST_JSON.reads_request_body(&BOUND_ONLY));
    assert!(!REST_XML.reads_request_body(&BOUND_ONLY));
    assert!(REST_JSON.reads_request_body(&IN_SCHEMA));
    assert!(RPC_V2_CBOR.reads_request_body(&BOUND_ONLY));

    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"read"));
    let collected = collect_request_body(body, &RequestBodyCollectionConfig::default())
        .await
        .unwrap();
    assert_eq!(collected.as_ref(), b"read");
}

#[tokio::test]
async fn rpc_body_handling_is_decided_separately_from_mechanical_collection() {
    // The generated RPC deserializers never touch the body when the input has no members; the
    // RPC protocols answer `reads_request_body` to mirror that.
    assert!(!RPC_V2_CBOR.reads_request_body(&EMPTY_IN_SCHEMA));
    assert!(RPC_V2_CBOR.reads_request_body(&RPC_IN_SCHEMA));

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
    };
    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"four"));
    assert!(matches!(
        collect_request_body(body, &limited).await,
        Err(super::RequestBodyCollectionError::TooLarge(_))
    ));

    let timed = RequestBodyCollectionConfig {
        max_bytes: None,
        read_timeout: Some(Duration::from_millis(1)),
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
        fn build_router(
            &self,
            _: &'static aws_smithy_schema::ServiceSchema<'static>,
            targets: &[crate::routing::OperationIndex],
            _: &crate::routing::SchemaRoutingOptions,
        ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError> {
            crate::routing::schema::rest_router::<crate::protocol::rest_json_1::RestJson1>(targets)
        }
        fn serialize_internal_failure(&self) -> crate::response::Response {
            crate::response::IntoResponse::<crate::protocol::rest_json_1::RestJson1>::into_response(
                crate::runtime_error::InternalFailureException,
            )
        }

        fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
            static ID: aws_smithy_schema::ShapeId<'static> = shape_id!("test", "minimal");
            &ID
        }
        fn deserialize_request<'a>(
            &'a self,
            _input: &Schema<'_>,
            request: &'a ServerRequest,
        ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
            Ok(aws_smithy_schema::codec::DynCodec::create_deserializer(
                &self.codec,
                &request.body,
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
    assert!(protocol.reads_request_body(&RPC_IN_SCHEMA));
    assert!(protocol.reads_request_body(&EMPTY_IN_SCHEMA));
    assert!(protocol.event_stream().is_none());

    let req = request("/", &[("accept", "text/xml")], b"");
    let erased: SharedServerProtocol = SharedServerProtocol::new(protocol);
    assert!(erased.check_accept(&OUT_SCHEMA, &req.headers).is_ok());
    assert!(erased.reads_request_body(&EMPTY_IN_SCHEMA));
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
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
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
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
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
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            match self.0 {
                Some(text) => s.write_string(&STRING_PAYLOAD, text),
                None => Ok(()),
            }
        }
    }
    struct Data(Option<&'static [u8]>);
    impl SerializableStruct for Data {
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
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&BOOM_MSG_MEMBER, "boom happened")?;
        s.write_string(&BOOM_HDR_MEMBER, "tagged")
    }
}

impl ModeledError for Boom {
    fn schema(&self) -> &Schema<'_> {
        &BOOM_SCHEMA
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
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
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
        assert!(response.headers().get("smithy-protocol").is_none());
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
    assert!(response.headers().get("smithy-protocol").is_none());
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
            SharedServerProtocol::new(RestJson1Protocol::default()),
            "aws.protocols#restJson1",
            false,
        ),
        (
            SharedServerProtocol::new(RestXmlProtocol::default()),
            "aws.protocols#restXml",
            false,
        ),
        (
            SharedServerProtocol::new(AwsJson1_0Protocol::default()),
            "aws.protocols#awsJson1_0",
            true,
        ),
        (
            SharedServerProtocol::new(AwsJson1_1Protocol::default()),
            "aws.protocols#awsJson1_1",
            true,
        ),
        (
            SharedServerProtocol::new(RpcV2CborProtocol::default()),
            "smithy.protocols#rpcv2Cbor",
            true,
        ),
    ];
    for (protocol, id, frames) in &protocols {
        assert_eq!(protocol.protocol_id().as_str(), *id);
        assert_eq!(
            protocol.event_stream().unwrap().initial_messages_in_frames(),
            *frames,
            "{id}"
        );
        assert!(
            !protocol.event_stream().unwrap().event_stream_media_type().is_empty(),
            "{id}"
        );
    }

    let erased: SharedServerProtocol = SharedServerProtocol::new(RestJson1Protocol::default());
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

    let mut serializer = erased.event_stream().unwrap().payload_codec().create_serializer();
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
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&TEAPOT_MSG, "short and stout")?;
            s.write_string(&TEAPOT_TAG, "brewing")
        }
    }

    let protocols: Vec<SharedServerProtocol> = vec![
        SharedServerProtocol::new(RestJson1Protocol::default()),
        SharedServerProtocol::new(RestXmlProtocol::default()),
        SharedServerProtocol::new(AwsJson1_0Protocol::default()),
        SharedServerProtocol::new(AwsJson1_1Protocol::default()),
        SharedServerProtocol::new(RpcV2CborProtocol::default()),
    ];
    for protocol in protocols {
        let id = protocol.protocol_id().as_str();
        let (status, headers, body) = {
            let response = protocol.serialize_response(&TEAPOT, &Teapot);
            let (parts, body) = response.into_parts();
            (parts.status, parts.headers, body.collect().await.unwrap().to_bytes())
        };
        assert_eq!(status, http::StatusCode::IM_A_TEAPOT, "{id}");
        assert!(!body.is_empty(), "{id}");
        assert!(headers.contains_key("content-type"), "{id}");
        if id.starts_with("aws.protocols#rest") {
            assert_eq!(headers.get("x-tag").unwrap(), "brewing", "{id}");
        } else {
            assert!(!headers.contains_key("x-tag"), "{id}");
        }

        // The same struct serialized as the output of an operation with the same schema.
        let again = protocol.serialize_response(&TEAPOT, &Teapot);
        let (again_parts, again_body) = again.into_parts();
        assert_eq!(again_parts.status, status, "{id}");
        assert_eq!(again_parts.headers, headers, "{id}");
        assert_eq!(again_body.collect().await.unwrap().to_bytes(), body, "{id}");
    }
}

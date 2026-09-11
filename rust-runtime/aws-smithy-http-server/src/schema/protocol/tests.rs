/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, OperationSchema, Schema, ServiceSchema, ShapeType};
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
    collect_request_body, CompiledOperation, DeserializableShape, DeserializeError, ErasedCompiledOperation,
    HttpModeledError, ModeledError, ProtocolRoutingTable, RequestBodyCollectionConfig, ServerProtocol, ServerRequest,
};

static REST_JSON: LazyLock<RestJson1Protocol> = LazyLock::new(RestJson1Protocol::default);
static REST_XML: LazyLock<RestXmlProtocol> = LazyLock::new(RestXmlProtocol::default);
static AWS_JSON_10: LazyLock<AwsJson1_0Protocol> = LazyLock::new(AwsJson1_0Protocol::default);
static AWS_JSON_11: LazyLock<AwsJson1_1Protocol> = LazyLock::new(AwsJson1_1Protocol::default);
static RPC_V2_CBOR: LazyLock<RpcV2CborProtocol> = LazyLock::new(RpcV2CborProtocol::default);

// --- a REST operation: `POST /pets/{name}?age=..` with a body member, `201` on success ---

static NAME_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "In", "name"), ShapeType::String, "name", 0).with_http_label();
static AGE_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "In", "age"), ShapeType::Integer, "age", 1).with_http_query("age");
static NOTE_MEMBER: Schema<'static> = Schema::new_member(shape_id!("test", "In", "note"), ShapeType::String, "note", 2);
static IN_MEMBERS: [&Schema<'static>; 3] = [&NAME_MEMBER, &AGE_MEMBER, &NOTE_MEMBER];
static IN_SCHEMA: Schema<'static> = Schema::new_struct(shape_id!("test", "In"), ShapeType::Structure, &IN_MEMBERS);

static OUT_MSG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Out", "msg"), ShapeType::String, "msg", 0);
static OUT_MEMBERS: [&Schema<'static>; 1] = [&OUT_MSG_MEMBER];
static OUT_SCHEMA: Schema<'static> = Schema::new_struct(shape_id!("test", "Out"), ShapeType::Structure, &OUT_MEMBERS);

static PET_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "Pet"), ShapeType::Operation)
    .with_http(HttpTrait::new("POST", "/pets/{name}", Some(201)));
static PET: OperationSchema<'static> = OperationSchema::new(&PET_SHAPE, &IN_SCHEMA, &OUT_SCHEMA, &[]);
static REST_PET: LazyLock<CompiledOperation<super::RestOperationState>> =
    LazyLock::new(|| REST_JSON.compile_operation(&PET));

// --- an RPC operation whose input and output were modeled by the user ---

static RPC_NOTE_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "RpcIn", "note"), ShapeType::String, "note", 0);
static RPC_IN_MEMBERS: [&Schema<'static>; 1] = [&RPC_NOTE_MEMBER];
static RPC_IN_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "RpcIn"), ShapeType::Structure, &RPC_IN_MEMBERS).with_original_name("RpcIn");
static RPC_OUT_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "RpcOut"), ShapeType::Structure, &OUT_MEMBERS).with_original_name("RpcOut");
static RPC_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "Rpc"), ShapeType::Operation);
static RPC: OperationSchema<'static> = OperationSchema::new(&RPC_SHAPE, &RPC_IN_SCHEMA, &RPC_OUT_SCHEMA, &[]);
static COMPILED_RPC: LazyLock<CompiledOperation<super::RpcOperationState>> =
    LazyLock::new(|| RPC_V2_CBOR.compile_operation(&RPC));

// --- a REST operation with no modeled input and an empty (synthetic) output ---

static EMPTY_IN_MEMBERS: [&Schema<'static>; 0] = [];
static EMPTY_IN_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "EmptyIn"), ShapeType::Structure, &EMPTY_IN_MEMBERS);
static EMPTY_OUT_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "EmptyOut"), ShapeType::Structure, &EMPTY_IN_MEMBERS);
static EMPTY_SHAPE: Schema<'static> =
    Schema::new(shape_id!("test", "Empty"), ShapeType::Operation).with_http(HttpTrait::new("POST", "/empty", None));
static EMPTY: OperationSchema<'static> = OperationSchema::new(&EMPTY_SHAPE, &EMPTY_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &[]);
static COMPILED_EMPTY: LazyLock<CompiledOperation<super::RpcOperationState>> =
    LazyLock::new(|| RPC_V2_CBOR.compile_operation(&EMPTY));
static REST_EMPTY: LazyLock<CompiledOperation<super::RestOperationState>> =
    LazyLock::new(|| REST_JSON.compile_operation(&EMPTY));

// --- an RPC operation with a synthetic (non-user-modeled) output ---

static RPC_SYNTHETIC_OUT: OperationSchema<'static> =
    OperationSchema::new(&RPC_SHAPE, &RPC_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &[]);
static COMPILED_RPC_SYNTHETIC_OUT: LazyLock<CompiledOperation<super::RpcOperationState>> =
    LazyLock::new(|| RPC_V2_CBOR.compile_operation(&RPC_SYNTHETIC_OUT));

static SERVICE_SHAPE: Schema<'static> = Schema::new(shape_id!("test", "Service"), ShapeType::Service);
static SERVICE_PROTOCOLS: [aws_smithy_schema::ShapeId<'static>; 1] = [shape_id!("aws.protocols", "restJson1")];
static SERVICE_OPERATIONS: [&OperationSchema<'static>; 2] = [&PET, &EMPTY];
static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
    &SERVICE_SHAPE,
    Some("2026-09-10"),
    &SERVICE_PROTOCOLS,
    &SERVICE_OPERATIONS,
);

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

async fn body_bytes(response: Response) -> bytes::Bytes {
    response.into_body().collect().await.expect("body collects").to_bytes()
}

#[test]
fn routing_table_compiles_and_indexes_protocol_operations() {
    let table = ProtocolRoutingTable::new(RestJson1Protocol::default(), &SERVICE);
    let pet = shape_id!("test", "Pet");
    let empty = shape_id!("test", "Empty");
    let missing = shape_id!("test", "Missing");

    assert_eq!(table.protocol().protocol_id().as_str(), "aws.protocols#restJson1");
    assert_eq!(table.operation(&pet).unwrap().schema().shape_id().as_str(), "test#Pet");
    assert!(table.operation(&missing).is_none());
    assert!(table.protocol().reads_request_body(table.operation(&pet).unwrap()));
    assert!(!table.protocol().reads_request_body(table.operation(&empty).unwrap()));
}

#[test]
fn rest_request_bindings_route_labels_query_and_body() {
    let req = request(
        "/pets/rex?age=7",
        &[("content-type", "application/json")],
        br#"{"note":"hi"}"#,
    );
    let input: TestInput = REST_JSON.deserialize(&REST_PET, &req).unwrap();
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
    let err = REST_JSON.deserialize::<TestInput>(&REST_PET, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::UnsupportedMediaType(_)), "{err}");

    let req = request("/pets/rex", &[], b"");
    let input: TestInput = REST_JSON.deserialize(&REST_PET, &req).unwrap();
    assert_eq!(input.name.as_deref(), Some("rex"));
    assert_eq!(input.note, None);
}

#[test]
fn rest_request_without_modeled_input_rejects_a_content_type() {
    let req = request("/empty", &[("content-type", "application/json")], b"");
    let err = REST_JSON.deserialize::<EmptyInput>(&REST_EMPTY, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::UnsupportedMediaType(_)), "{err}");

    let req = request("/empty", &[], b"");
    REST_JSON.deserialize::<EmptyInput>(&REST_EMPTY, &req).unwrap();
}

#[test]
fn rest_request_wire_failures_are_serde_errors() {
    let req = request("/pets/rex?age=old", &[], b"");
    let err = REST_JSON.deserialize::<TestInput>(&REST_PET, &req).unwrap_err();
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
    let mut serializer = RPC_V2_CBOR.codec().create_serializer();
    serializer.write_struct(&RPC_IN_SCHEMA, &Body).unwrap();
    let body = serializer.finish();

    let req = request(
        "/service/Svc/operation/Rpc",
        &[("content-type", "application/cbor")],
        &body,
    );
    let input: RpcTestInput = RPC_V2_CBOR.deserialize(&COMPILED_RPC, &req).unwrap();
    assert_eq!(input.0.note.as_deref(), Some("hi"));
}

#[test]
fn rpc_request_with_an_empty_body_leaves_members_unset() {
    let req = request("/", &[("content-type", "application/x-amz-json-1.0")], b"");
    let input: RpcTestInput = AWS_JSON_10.deserialize(&COMPILED_RPC, &req).unwrap();
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
            REST_JSON.deserialize::<TestInput>(&REST_PET, &req).is_ok(),
            "accept: {accept}"
        );
    }
    let req = request("/pets/rex", &[("accept", "text/xml")], b"");
    let err = REST_JSON.deserialize::<TestInput>(&REST_PET, &req).unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");

    // REST: no body-bound output members means no gate at all.
    let req = request("/empty", &[("accept", "text/xml")], b"");
    assert!(REST_JSON.deserialize::<EmptyInput>(&REST_EMPTY, &req).is_ok());

    // awsJson: gated against the fixed protocol content type on every operation.
    let req = request("/", &[("accept", "application/x-amz-json-1.1")], b"");
    assert!(AWS_JSON_11.deserialize::<RpcTestInput>(&COMPILED_RPC, &req).is_ok());
    let req = request("/", &[("accept", "application/x-amz-json-1.0")], b"");
    let err = AWS_JSON_11
        .deserialize::<RpcTestInput>(&COMPILED_RPC, &req)
        .unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");

    // rpcv2Cbor: gated only when the operation's output was modeled by the user.
    let req = request("/service/Svc/operation/Rpc", &[("accept", "text/plain")], b"");
    let err = RPC_V2_CBOR
        .deserialize::<RpcTestInput>(&COMPILED_RPC, &req)
        .unwrap_err();
    assert!(matches!(err, DeserializeError::NotAcceptable), "{err}");
    assert!(RPC_V2_CBOR
        .deserialize::<RpcTestInput>(&COMPILED_RPC_SYNTHETIC_OUT, &req)
        .is_ok());
}

#[test]
fn accept_expectation_follows_the_output_payload() {
    static BLOB_PAYLOAD: Schema<'static> =
        Schema::new_member(shape_id!("test", "BlobOut", "data"), ShapeType::Blob, "data", 0).with_http_payload();
    static BLOB_OUT_MEMBERS: [&Schema<'static>; 1] = [&BLOB_PAYLOAD];
    static BLOB_OUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BlobOut"), ShapeType::Structure, &BLOB_OUT_MEMBERS);
    static BLOB_OP: OperationSchema<'static> = OperationSchema::new(&EMPTY_SHAPE, &EMPTY_IN_SCHEMA, &BLOB_OUT, &[]);

    // An untyped blob response defaults to `application/octet-stream`, but accepts any media type.
    let req = request("/empty", &[("accept", "application/octet-stream")], b"");
    let operation = REST_JSON.compile_operation(&BLOB_OP);
    assert!(REST_JSON.deserialize::<EmptyInput>(&operation, &req).is_ok());
    let req = request("/empty", &[("accept", "application/json")], b"");
    assert!(REST_JSON.deserialize::<EmptyInput>(&operation, &req).is_ok());
}

// --- body collection ---

#[tokio::test]
async fn rest_protocols_skip_the_body_when_nothing_is_bound_to_it() {
    static BOUND_MEMBERS: [&Schema<'static>; 2] = [&NAME_MEMBER, &AGE_MEMBER];
    static BOUND_ONLY: Schema<'static> =
        Schema::new_struct(shape_id!("test", "BoundOnly"), ShapeType::Structure, &BOUND_MEMBERS);
    static BOUND_OPERATION: OperationSchema<'static> = OperationSchema::new(&PET_SHAPE, &BOUND_ONLY, &OUT_SCHEMA, &[]);
    let compiled_bound = RPC_V2_CBOR.compile_operation(&BOUND_OPERATION);
    let rest_json_bound = REST_JSON.compile_operation(&BOUND_OPERATION);
    let rest_xml_bound = REST_XML.compile_operation(&BOUND_OPERATION);

    assert!(!REST_JSON.reads_request_body(&rest_json_bound));
    assert!(!REST_XML.reads_request_body(&rest_xml_bound));
    assert!(REST_JSON.reads_request_body(&REST_PET));
    assert!(RPC_V2_CBOR.reads_request_body(&compiled_bound));

    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"read"));
    let collected = collect_request_body(body, &RequestBodyCollectionConfig::default())
        .await
        .unwrap();
    assert_eq!(collected.as_ref(), b"read");
}

#[tokio::test]
async fn rpc_body_handling_is_compiled_separately_from_mechanical_collection() {
    // The generated RPC deserializers never touch the body when the input has no members; the
    // RPC protocols override `reads_request_body` to mirror that.
    assert!(!RPC_V2_CBOR.reads_request_body(&COMPILED_EMPTY));
    assert!(RPC_V2_CBOR.reads_request_body(&COMPILED_RPC));

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
fn protocols_without_operation_state_collect_the_body_by_default() {
    #[derive(Debug, Default)]
    struct Stateless {
        codec: aws_smithy_json::codec::JsonCodec,
    }

    impl ServerProtocol for Stateless {
        type Codec = aws_smithy_json::codec::JsonCodec;
        type OperationState = ();

        fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
            static ID: aws_smithy_schema::ShapeId<'static> = shape_id!("test", "stateless");
            &ID
        }
        fn codec(&self) -> &Self::Codec {
            &self.codec
        }
        fn deserialize_request<'a>(
            &'a self,
            _operation: &'a CompiledOperation<()>,
            request: &'a ServerRequest,
        ) -> Result<Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>, DeserializeError> {
            Ok(Box::new(self.codec().create_deserializer(&request.body)))
        }
        fn serialize_response(&self, _: &CompiledOperation<()>, _: &dyn SerializableStruct) -> Response {
            unimplemented!()
        }
        fn serialize_error(&self, _: &dyn HttpModeledError) -> Response {
            unimplemented!()
        }
        fn serialize_rejection(&self, _: DeserializeError) -> Response {
            unimplemented!()
        }
    }

    let protocol = Stateless::default();
    let with_input = protocol.compile_operation(&RPC);
    let without_input = protocol.compile_operation(&EMPTY);
    assert!(protocol.reads_request_body(&with_input));
    assert!(protocol.reads_request_body(&without_input));

    let erased: &dyn super::DynServerProtocol = &protocol;
    assert!(erased.reads_request_body(&without_input));
    assert!(erased.accepts_operation(&without_input));
}

#[test]
fn streaming_inputs_are_compiled_as_streaming() {
    static STREAM: Schema<'static> =
        Schema::new_member(shape_id!("test", "StreamingInput", "body"), ShapeType::Blob, "body", 0)
            .with_http_payload()
            .with_streaming();
    static MEMBERS: [&Schema<'static>; 1] = [&STREAM];
    static INPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "StreamingInput"), ShapeType::Structure, &MEMBERS);
    static OP: OperationSchema<'static> = OperationSchema::new(&PET_SHAPE, &INPUT, &EMPTY_OUT_SCHEMA, &[]);

    let operation = REST_JSON.compile_operation(&OP);
    assert!(operation.input_is_streaming());
    assert!(ErasedCompiledOperation::input_is_streaming(&operation));
    assert!(!REST_PET.input_is_streaming());
}

// --- responses ---

#[tokio::test]
async fn responses_take_the_status_from_the_operation() {
    let response = REST_JSON.serialize_response(&REST_PET, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::CREATED);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    assert_eq!(get_body_as_string(response.into_body()).await, r#"{"msg":"ok"}"#);

    let response = RPC_V2_CBOR.serialize_response(&COMPILED_RPC, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
    assert_eq!(response.headers().get("content-type").unwrap(), "application/cbor");
}

#[tokio::test]
async fn aws_json_stamps_the_content_type_on_an_empty_body() {
    static OP: OperationSchema<'static> = OperationSchema::new(&RPC_SHAPE, &RPC_IN_SCHEMA, &EMPTY_OUT_SCHEMA, &[]);
    let compiled_op = AWS_JSON_11.compile_operation(&OP);
    struct Nothing;
    impl SerializableStruct for Nothing {
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    let response = AWS_JSON_11.serialize_response(&compiled_op, &Nothing);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/x-amz-json-1.1"
    );
    assert_eq!(response.headers().get("content-length").unwrap(), "0");

    let rest_operation = REST_JSON.compile_operation(&OP);
    let response = REST_JSON.serialize_response(&rest_operation, &Nothing);
    assert!(response.headers().get("content-type").is_none());
}

// --- a modeled error with one body member and one header-bound member ---

static BOOM_MSG_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Boom", "message"), ShapeType::String, "message", 0);
static BOOM_HDR_MEMBER: Schema<'static> =
    Schema::new_member(shape_id!("test", "Boom", "tag"), ShapeType::String, "tag", 1).with_http_header("x-boom-tag");
static BOOM_MEMBERS: [&Schema<'static>; 2] = [&BOOM_MSG_MEMBER, &BOOM_HDR_MEMBER];
static BOOM_SCHEMA: Schema<'static> =
    Schema::new_struct(shape_id!("test", "Boom"), ShapeType::Structure, &BOOM_MEMBERS);

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

    // Modeled CBOR body with a leading `__type`, but — unlike handler-returned errors — no
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

#[tokio::test]
async fn every_marker_erases_to_a_dyn_server_protocol() {
    let protocols: Vec<(&dyn crate::schema::DynServerProtocol, &str)> = vec![
        (&*REST_JSON, "aws.protocols#restJson1"),
        (&*REST_XML, "aws.protocols#restXml"),
        (&*AWS_JSON_10, "aws.protocols#awsJson1_0"),
        (&*AWS_JSON_11, "aws.protocols#awsJson1_1"),
        (&*RPC_V2_CBOR, "smithy.protocols#rpcv2Cbor"),
    ];
    for (protocol, id) in &protocols {
        assert_eq!(protocol.protocol_id().as_str(), *id);
    }

    let erased: &dyn crate::schema::DynServerProtocol = &*REST_JSON;
    let req = request(
        "/pets/rex?age=7",
        &[("content-type", "application/json")],
        br#"{"note":"hi"}"#,
    );
    let mut deserializer = erased.deserialize_request(&*REST_PET, &req).ok().unwrap();
    let input = TestInput::deserialize(&mut *deserializer).unwrap();
    assert_eq!(input.age, Some(7));

    // A failure travels as `DeserializeError` on the erased path too, and the erased protocol
    // renders it.
    let req = request("/pets/rex", &[("content-type", "text/xml")], b"{}");
    let err = erased.deserialize_request(&*REST_PET, &req).err().unwrap();
    let response = erased.serialize_rejection(err);
    assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let mut serializer = erased.codec().create_serializer();
    serializer.write_struct(&OUT_SCHEMA, &TestOutput).unwrap();
    assert_eq!(serializer.finish_boxed(), br#"{"msg":"ok"}"#);
}

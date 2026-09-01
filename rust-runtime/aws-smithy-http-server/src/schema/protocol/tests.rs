/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use crate::deserialize::{DeserializableShape, DeserializeError};
use crate::modeled_error::HttpModeledError;
use crate::modeled_error::ModeledError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::protocol::rest_json_1::RestJson1;
use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
use crate::protocol::test_helpers::get_body_as_string;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{Schema, ShapeId, ShapeType};

// ------------------------------------------------------------------
// A hand-built operation input: label + query + header + body member.
// ------------------------------------------------------------------

static NAME_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#In$name", "test", "In"),
    ShapeType::String,
    "name",
    0,
)
.with_http_label();
static AGE_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#In$age", "test", "In"),
    ShapeType::Integer,
    "age",
    1,
)
.with_http_query("age");
static NOTE_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#In$note", "test", "In"),
    ShapeType::String,
    "note",
    2,
);
static IN_MEMBERS: [&Schema<'static>; 3] = [&NAME_MEMBER, &AGE_MEMBER, &NOTE_MEMBER];
static IN_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("test#In", "test", "In"),
    ShapeType::Structure,
    &IN_MEMBERS,
)
.with_http(HttpTrait::new("POST", "/pets/{name}", Some(200)));

// Body-only variant for the RPC protocols (no @http bindings at all).
static RPC_NOTE_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#RpcIn$note", "test", "RpcIn"),
    ShapeType::String,
    "note",
    0,
);
static RPC_IN_MEMBERS: [&Schema<'static>; 1] = [&RPC_NOTE_MEMBER];
static RPC_IN_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("test#RpcIn", "test", "RpcIn"),
    ShapeType::Structure,
    &RPC_IN_MEMBERS,
)
.with_original_name("RpcIn");

#[derive(Debug, Default, PartialEq)]
struct TestInput {
    name: Option<String>,
    age: Option<i32>,
    note: Option<String>,
}

impl TestInput {
    fn walk(
        schema: &'static Schema<'static>,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Self, DeserializeError> {
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

/// The same walker against the body-only RPC schema.
#[derive(Debug, Default, PartialEq)]
struct RpcTestInput(TestInput);

impl DeserializableShape for RpcTestInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        TestInput::walk(&RPC_IN_SCHEMA, deserializer).map(RpcTestInput)
    }
}

fn parts(uri: &str, headers: &[(&'static str, &str)]) -> http::request::Parts {
    let mut builder = http::Request::builder().method("POST").uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(()).unwrap().into_parts().0
}

static EMPTY_IN_MEMBERS: [&Schema<'static>; 0] = [];
static EMPTY_IN_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("test#EmptyIn", "test", "EmptyIn"),
    ShapeType::Structure,
    &EMPTY_IN_MEMBERS,
)
.with_http(HttpTrait::new("POST", "/empty", Some(200)));

#[derive(Debug)]
struct EmptyInput;
impl DeserializableShape for EmptyInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        deserializer.read_struct(&EMPTY_IN_SCHEMA, &mut |_, _| Ok(()))?;
        Ok(EmptyInput)
    }
}

#[test]
fn request_paths() {
    use crate::protocol::rest_json_1::rejection::RequestRejection;

    // REST: label + query + body member route through the composite.
    let p = parts("/pets/rex?age=7", &[("content-type", "application/json")]);
    let input: TestInput = RestJson1::deserialize_request(&IN_SCHEMA, &IN_SCHEMA, &p, br#"{"note":"hi"}"#).unwrap();
    assert_eq!(
        input,
        TestInput {
            name: Some("rex".to_string()),
            age: Some(7),
            note: Some("hi".to_string()),
        }
    );

    // Wrong content type (non-empty body) and bad Accept are rejected.
    let p = parts("/pets/rex", &[("content-type", "text/xml")]);
    assert!(matches!(
        RestJson1::deserialize_request::<TestInput>(&IN_SCHEMA, &IN_SCHEMA, &p, b"{}").unwrap_err(),
        RequestRejection::MissingContentType(_)
    ));
    let p = parts(
        "/pets/rex",
        &[("content-type", "application/json"), ("accept", "text/xml")],
    );
    assert!(matches!(
        RestJson1::deserialize_request::<TestInput>(&IN_SCHEMA, &IN_SCHEMA, &p, b"{}").unwrap_err(),
        RequestRejection::NotAcceptable
    ));

    // The legacy `if !bytes.is_empty()` gate: no content type required
    // when no body was sent, even though body members are modeled.
    let p = parts("/pets/rex", &[]);
    let input: TestInput = RestJson1::deserialize_request(&IN_SCHEMA, &IN_SCHEMA, &p, b"").unwrap();
    assert_eq!(input.name.as_deref(), Some("rex"));
    assert_eq!(input.note, None);

    // `serverContentTypeCheckNoModeledInput`: content-type must NOT be
    // present when the operation has no modeled input.
    let p = parts("/empty", &[("content-type", "application/json")]);
    assert!(matches!(
        RestJson1::deserialize_request::<EmptyInput>(&EMPTY_IN_SCHEMA, &EMPTY_IN_SCHEMA, &p, b"").unwrap_err(),
        RequestRejection::MissingContentType(_)
    ));
    let p = parts("/empty", &[]);
    RestJson1::deserialize_request::<EmptyInput>(&EMPTY_IN_SCHEMA, &EMPTY_IN_SCHEMA, &p, b"").unwrap();

    // RPC: body round-trips through the protocol's own codec.
    use aws_smithy_schema::codec::FinishSerializer;
    struct Body;
    impl SerializableStruct for Body {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&RPC_NOTE_MEMBER, "hi")
        }
    }
    let mut serializer = <RpcV2Cbor as ServerProtocol>::codec().create_serializer();
    serializer.write_struct(&RPC_IN_SCHEMA, &Body).unwrap();
    let body = serializer.finish();
    let p = parts("/service/Op", &[("content-type", "application/cbor")]);
    let input: RpcTestInput = RpcV2Cbor::deserialize_request(&RPC_IN_SCHEMA, &RPC_IN_SCHEMA, &p, &body).unwrap();
    assert_eq!(input.0.note.as_deref(), Some("hi"));

    // RPC empty body: members stay unset (`build()` owns @required).
    let p = parts("/service/Op", &[("content-type", "application/x-amz-json-1.0")]);
    let input: RpcTestInput = AwsJson1_0::deserialize_request(&RPC_IN_SCHEMA, &RPC_IN_SCHEMA, &p, b"").unwrap();
    assert_eq!(input.0, TestInput::default());
}

// ------------------------------------------------------------------
// Responses
// ------------------------------------------------------------------

static OUT_MSG_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#Out$msg", "test", "Out"),
    ShapeType::String,
    "msg",
    0,
);
static OUT_MEMBERS: [&Schema<'static>; 1] = [&OUT_MSG_MEMBER];
static OUT_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("test#Out", "test", "Out"),
    ShapeType::Structure,
    &OUT_MEMBERS,
)
.with_http(HttpTrait::new("POST", "/pets/{name}", Some(201)))
.with_original_name("Out");

struct TestOutput;
impl SerializableStruct for TestOutput {
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&OUT_MSG_MEMBER, "ok")
    }
}

#[tokio::test]
async fn response_paths() {
    // REST: status from the @http trait, protocol content type, codec body.
    let response = RestJson1::serialize_response(&OUT_SCHEMA, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::CREATED);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");
    let body = get_body_as_string(response.into_body()).await;
    assert_eq!(body, r#"{"msg":"ok"}"#);

    // RPC: default status, protocol headers stamped.
    let response = RpcV2Cbor::serialize_response(&RPC_IN_SCHEMA, &TestOutput);
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
    assert_eq!(response.headers().get("content-type").unwrap(), "application/cbor");
}

// ------------------------------------------------------------------
// Errors and the 2d validation seam
// ------------------------------------------------------------------

static BOOM_MSG_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#Boom$message", "test", "Boom"),
    ShapeType::String,
    "message",
    0,
);
static BOOM_HDR_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("test#Boom$tag", "test", "Boom"),
    ShapeType::String,
    "tag",
    1,
)
.with_http_header("x-boom-tag");
static BOOM_MEMBERS: [&Schema<'static>; 2] = [&BOOM_MSG_MEMBER, &BOOM_HDR_MEMBER];
static BOOM_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("test#Boom", "test", "Boom"),
    ShapeType::Structure,
    &BOOM_MEMBERS,
);

#[derive(Debug)]
struct Boom;

impl std::fmt::Display for Boom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("boom happened")
    }
}

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
async fn error_framing_and_validation_seam() {
    // restJson1: name-only header discriminator, status from
    // status_code(), @httpHeader-bound error member split out of the body.
    let response = RestJson1::serialize_error(&Boom);
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
    assert_eq!(response.headers().get("x-boom-tag").unwrap(), "tagged");
    let body = get_body_as_string(response.into_body()).await;
    assert_eq!(body, r#"{"message":"boom happened"}"#);

    // awsJson 1.0: full shape ID written last; header-bound members are
    // NOT split on RPC protocols. awsJson 1.1: name only.
    let body = get_body_as_string(AwsJson1_0::serialize_error(&Boom).into_body()).await;
    assert!(body.contains(r#""tag":"tagged""#), "{body}");
    assert!(body.ends_with(r#""__type":"test#Boom"}"#), "{body}");
    let body = get_body_as_string(AwsJson1_1::serialize_error(&Boom).into_body()).await;
    assert!(body.ends_with(r#""__type":"Boom"}"#), "{body}");

    // rpcv2Cbor: full shape ID as the FIRST map entry.
    let response = RpcV2Cbor::serialize_error(&Boom);
    assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");
    use http_body_util::BodyExt;
    let bytes = response.into_body().collect().await.expect("body collects").to_bytes();
    let type_pos = bytes.windows(6).position(|w| w == b"__type").expect("__type present");
    let msg_pos = bytes.windows(7).position(|w| w == b"message").expect("message present");
    assert!(type_pos < msg_pos, "__type must be the first map entry");
    assert!(bytes.windows(9).any(|w| w == b"test#Boom"), "full shape ID present");

    // The 2d seam: walker constraint-violation channel → rejection →
    // RuntimeError::ModeledValidation → serialized exactly ONCE, by the
    // protocol, at the boundary, with the ACTUAL shape name (2f
    // fix-forward — not the legacy hard-coded `ValidationException`).
    use crate::protocol::rest_json_1::rejection::RequestRejection;
    use crate::protocol::rest_json_1::runtime_error::RuntimeError;
    use crate::response::IntoResponse;
    let rejection: RequestRejection = DeserializeError::ConstraintViolation(Box::new(Boom)).into();
    let runtime_error = RuntimeError::from(rejection);
    assert!(matches!(runtime_error, RuntimeError::ModeledValidation(_)));
    let response = IntoResponse::<RestJson1>::into_response(runtime_error);
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
    let body = get_body_as_string(response.into_body()).await;
    assert_eq!(body, r#"{"message":"boom happened"}"#);
}

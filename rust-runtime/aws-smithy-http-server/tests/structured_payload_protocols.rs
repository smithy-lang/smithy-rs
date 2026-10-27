/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Regression for PR #4894 discussion_r4254993789: serializers pass the payload member schema;
//! REST framing obtains target metadata from the nested value's own schema.
//! The matching generated serde fixture is `ServerSchemaRpcPayloadTest`.

use aws_smithy_cbor::{data::Type, Decoder};
use aws_smithy_http_server::schema::protocol::{AwsJson1_0, AwsJson1_1, RestJson1, RestXml, RpcV2Cbor};
use aws_smithy_http_server::schema::{MetadataRoutedProtocol, ProtocolBuildContext, ServerProtocol, ServiceSchema};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_types::Document;
use http_body_util::BodyExt;
use std::collections::HashMap;

static TARGET_CHILD: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "Target", "child"),
    ShapeType::String,
    "child",
    0,
)
.with_xml_name("Child");
static TARGET: Schema = Schema::new_struct(
    shape_id!("test.sharedrpcpayload", "Target"),
    ShapeType::Structure,
    &[&TARGET_CHILD],
)
.with_xml_name("WireTarget")
.with_xml_namespace("urn:target", Some("t"));
static CHOICE_CHILD: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "Choice", "child"),
    ShapeType::String,
    "child",
    0,
)
.with_xml_name("Child");
static CHOICE: Schema = Schema::new_struct(
    shape_id!("test.sharedrpcpayload", "Choice"),
    ShapeType::Union,
    &[&CHOICE_CHILD],
)
.with_xml_name("WireChoice")
.with_xml_namespace("urn:target", Some("t"));

static STRUCT_BODY: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "StructPayloadOutput", "body"),
    ShapeType::Structure,
    "body",
    0,
)
.with_http_payload()
.with_xml_name("MemberTarget");
static STRUCT_HEADER: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "StructPayloadOutput", "header"),
    ShapeType::String,
    "header",
    1,
)
.with_http_header("x-h");
static STRUCT_OUTPUT: Schema = Schema::new_struct(
    shape_id!("test.sharedrpcpayload", "StructPayloadOutput"),
    ShapeType::Structure,
    &[&STRUCT_BODY, &STRUCT_HEADER],
)
.with_http(HttpTrait::new("POST", "/struct", Some(200)))
.with_original_name("StructPayloadOutput");
static UNION_BODY: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "UnionPayloadOutput", "body"),
    ShapeType::Union,
    "body",
    0,
)
.with_http_payload()
.with_xml_name("MemberChoice");
static UNION_HEADER: Schema = Schema::new_member(
    shape_id!("test.sharedrpcpayload", "UnionPayloadOutput", "header"),
    ShapeType::String,
    "header",
    1,
)
.with_http_header("x-h");
static UNION_OUTPUT: Schema = Schema::new_struct(
    shape_id!("test.sharedrpcpayload", "UnionPayloadOutput"),
    ShapeType::Structure,
    &[&UNION_BODY, &UNION_HEADER],
)
.with_http(HttpTrait::new("POST", "/union", Some(200)))
.with_original_name("UnionPayloadOutput");
static SERVICE: ServiceSchema = ServiceSchema::new(shape_id!("test.sharedrpcpayload", "Payloads"), Some("1"), &[], &[]);

struct NestedValue(&'static Schema<'static>);

impl SerializableStruct for NestedValue {
    fn schema(&self) -> &Schema<'_> {
        self.0
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(self.0.members()[0], "value")
    }
}

struct Payload<'a> {
    schema: &'static Schema<'static>,
    body: Option<&'a NestedValue>,
}

impl SerializableStruct for Payload<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.schema
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        if let Some(nested) = self.body {
            // The member carries the RPC field key and HTTP/XML binding; the nested
            // value reports the target schema used for standalone REST framing.
            serializer.write_struct(self.schema.members()[0], nested)?;
        }
        serializer.write_string(self.schema.members()[1], "hv")
    }
}

fn json_document(body: &[u8]) -> Document {
    let mut tokens = aws_smithy_json::deserialize::json_token_iter(body).peekable();
    let document = aws_smithy_json::deserialize::token::expect_document(&mut tokens).unwrap();
    assert!(tokens.next().is_none(), "trailing JSON data");
    document
}

fn cbor_value(decoder: &mut Decoder<'_>) -> Document {
    match decoder.datatype().unwrap() {
        Type::Map | Type::MapIndef => {
            let length = decoder.map().unwrap();
            let mut entries = HashMap::new();
            while length
                .map(|count| entries.len() < count as usize)
                .unwrap_or_else(|| decoder.datatype().unwrap() != Type::Break)
            {
                let key = decoder.str().unwrap().into_owned();
                assert!(entries.insert(key, cbor_value(decoder)).is_none(), "duplicate CBOR key");
            }
            if length.is_none() {
                decoder.skip().unwrap();
            }
            Document::Object(entries)
        }
        Type::String => Document::String(decoder.str().unwrap().into_owned()),
        kind => panic!("unexpected fixture CBOR type: {kind:?}"),
    }
}

fn cbor_document(body: &[u8]) -> Document {
    let mut decoder = Decoder::new(body);
    let document = cbor_value(&mut decoder);
    assert_eq!(decoder.position(), body.len(), "trailing CBOR data");
    document
}

#[tokio::test]
async fn shared_structured_payloads_preserve_rpc_documents_and_rest_framing() {
    let aws_json_10 = AwsJson1_0::default();
    let aws_json_11 = AwsJson1_1::default();
    let cbor = RpcV2Cbor::default();
    let rest_json = RestJson1::default();
    let rest_xml = RestXml::default();
    let settings =
        aws_smithy_http_server::schema::settings::parse_settings_json(br#"{"responseBindings":"smithy-specs"}"#);
    let context = ProtocolBuildContext::new(&SERVICE).with_settings(Some(&settings));
    let aws_json_10_protocol = AwsJson1_0::from_build_context(&context).unwrap();
    let aws_json_11_protocol = AwsJson1_1::from_build_context(&context).unwrap();
    let protocols: [(&str, &dyn ServerProtocol, &str, bool); 7] = [
        ("awsJson1_0", &aws_json_10, "application/x-amz-json-1.0", true),
        ("awsJson1_1", &aws_json_11, "application/x-amz-json-1.1", true),
        ("rpcv2Cbor", &cbor, "application/cbor", false),
        ("restJson1", &rest_json, "application/json", true),
        ("restXml", &rest_xml, "application/xml", true),
        (
            "awsJson1_0/protocol",
            &aws_json_10_protocol,
            "application/x-amz-json-1.0",
            false,
        ),
        (
            "awsJson1_1/protocol",
            &aws_json_11_protocol,
            "application/x-amz-json-1.1",
            false,
        ),
    ];
    let target = NestedValue(&TARGET);
    let choice = NestedValue(&CHOICE);
    for present in [true, false] {
        let structure = Payload {
            schema: &STRUCT_OUTPUT,
            body: present.then_some(&target),
        };
        let union = Payload {
            schema: &UNION_OUTPUT,
            body: present.then_some(&choice),
        };
        let values = [
            ("structure", &structure, "MemberTarget"),
            ("union", &union, "MemberChoice"),
        ];
        // Revisit protocols in both directions using the exact same schemas,
        // values, and protocol instances, including after a REST XML response.
        for (name, protocol, content_type, has_header) in
            protocols.iter().chain(protocols.iter().rev()).chain(protocols.iter())
        {
            for (kind, value, root) in values {
                let response = protocol.serialize_response(value.schema(), value);
                let (parts, body) = response.into_parts();
                let body = body.collect().await.unwrap().to_bytes();
                assert_eq!(parts.status, 200, "{name} {kind} present={present}");
                assert_eq!(parts.headers["content-type"], *content_type);
                assert_eq!(
                    parts.headers["content-length"]
                        .to_str()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap(),
                    body.len(),
                    "{name} {kind} present={present}"
                );
                assert_eq!(
                    parts.headers.get("x-h").map(|v| v.to_str().unwrap()),
                    has_header.then_some("hv")
                );
                assert_eq!(
                    parts.headers.get("smithy-protocol").map(|v| v.to_str().unwrap()),
                    (*name == "rpcv2Cbor").then_some("rpc-v2-cbor")
                );
                assert_eq!(
                    parts.headers.len(),
                    2 + usize::from(*has_header) + usize::from(*name == "rpcv2Cbor")
                );
                match *name {
                    "restXml" => {
                        let expected = if present {
                            format!("<{root} xmlns:t=\"urn:target\"><Child>value</Child></{root}>")
                        } else {
                            String::new()
                        };
                        assert_eq!(body.as_ref(), expected.as_bytes(), "{name} {kind} present={present}");
                    }
                    // Generated serde REST JSON writes {} for an unset structure
                    // payload, and an empty body for an unset union payload.
                    "restJson1" if !present && kind == "union" => assert!(body.is_empty()),
                    _ => {
                        let document = if *name == "rpcv2Cbor" {
                            cbor_document(&body)
                        } else {
                            json_document(&body)
                        };
                        let expected = match (*name, present) {
                            ("restJson1", true) => br#"{"child":"value"}"#.as_slice(),
                            ("restJson1", false) => b"{}".as_slice(),
                            (_, true) => br#"{"body":{"child":"value"},"header":"hv"}"#.as_slice(),
                            (_, false) => br#"{"header":"hv"}"#.as_slice(),
                        };
                        assert_eq!(document, json_document(expected), "{name} {kind} present={present}");
                    }
                }
                assert_eq!(TARGET.xml_name().unwrap().value(), "WireTarget");
                assert_eq!(CHOICE.xml_name().unwrap().value(), "WireChoice");
                assert_eq!(STRUCT_BODY.xml_name().unwrap().value(), "MemberTarget");
                assert_eq!(UNION_BODY.xml_name().unwrap().value(), "MemberChoice");
                assert!(std::ptr::eq(target.schema(), &TARGET));
                assert!(std::ptr::eq(choice.schema(), &CHOICE));
            }
        }
    }
}

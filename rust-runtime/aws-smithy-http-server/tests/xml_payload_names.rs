/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML payload roots follow legacy member/target naming precedence without losing target metadata.

use aws_smithy_http_server::schema::protocol::{RestJson1Protocol, RestXmlProtocol};
use aws_smithy_http_server::schema::{HttpModeledError, ServerProtocol};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use http_body_util::BodyExt;

static CHILD: Schema =
    Schema::new_member(shape_id!("test", "Target", "child"), ShapeType::String, "child", 0).with_xml_name("Child");
static ATTRIBUTE: Schema =
    Schema::new_member(shape_id!("test", "Target", "attr"), ShapeType::String, "attr", 1).with_xml_attribute();
static UNION_MEMBERS: [&Schema; 1] = [&CHILD];
static STRUCT_MEMBERS: [&Schema; 2] = [&CHILD, &ATTRIBUTE];

struct TargetValue<'a>(&'a Schema<'a>);
impl SerializableStruct for TargetValue<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.0
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&CHILD, "value")?;
        if self.0.shape_type() == ShapeType::Structure {
            s.write_string(&ATTRIBUTE, "a")?;
        }
        Ok(())
    }
}

struct Output<'a> {
    schema: &'a Schema<'a>,
    target: &'a Schema<'a>,
}
impl SerializableStruct for Output<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.schema
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        // Generated payload serializers pass the target's schema.
        s.write_struct(self.target, &TargetValue(self.target))
    }
}

#[tokio::test]
async fn structured_payload_root_precedence_preserves_namespace_and_children() {
    for kind in [ShapeType::Structure, ShapeType::Union] {
        let members = if kind == ShapeType::Structure {
            &STRUCT_MEMBERS[..]
        } else {
            &UNION_MEMBERS[..]
        };
        for target_name in [None, Some("RenamedTarget")] {
            for member_name in [None, Some("WirePayload")] {
                for namespace in [None, Some(("urn:target", None)), Some(("urn:target", Some("t")))] {
                    let mut target = Schema::new_struct(shape_id!("test", "Target"), kind, members);
                    if let Some(name) = target_name {
                        target = target.with_xml_name(name);
                    }
                    if let Some((uri, prefix)) = namespace {
                        target = target.with_xml_namespace(uri, prefix);
                    }
                    let mut payload =
                        Schema::new_member(shape_id!("test", "Output", "body"), kind, "body", 0).with_http_payload();
                    if let Some(name) = member_name {
                        payload = payload.with_xml_name(name);
                    }
                    let output_members = [&payload];
                    let output = Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &output_members);
                    let response = RestXmlProtocol::default().serialize_response(
                        &output,
                        &Output {
                            schema: &output,
                            target: &target,
                        },
                    );
                    assert_eq!(response.status(), 200);
                    let body = response.into_body().collect().await.unwrap().to_bytes();
                    let root = member_name.or(target_name).unwrap_or("Target");
                    let xmlns = match namespace {
                        None => String::new(),
                        Some((uri, None)) => format!(" xmlns=\"{uri}\""),
                        Some((uri, Some(prefix))) => format!(" xmlns:{prefix}=\"{uri}\""),
                    };
                    let attr = if kind == ShapeType::Structure {
                        " attr=\"a\""
                    } else {
                        ""
                    };
                    assert_eq!(
                        body.as_ref(),
                        format!("<{root}{xmlns}{attr}><Child>value</Child></{root}>").as_bytes(),
                    );
                    assert_eq!(target.xml_name().map(|name| name.value()), target_name);
                }
            }
        }
    }
}

#[tokio::test]
async fn shared_payload_target_can_have_distinct_roots_without_changing_json() {
    let target = Schema::new_struct(shape_id!("test", "Target"), ShapeType::Structure, &STRUCT_MEMBERS)
        .with_xml_name("RenamedTarget")
        .with_xml_namespace("urn:target", None);
    for name in ["FirstPayload", "SecondPayload", "FirstPayload"] {
        let payload = Schema::new_member(shape_id!("test", "Output", "body"), ShapeType::Structure, "body", 0)
            .with_http_payload()
            .with_xml_name(name);
        let members = [&payload];
        let output = Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &members);
        let value = Output {
            schema: &output,
            target: &target,
        };
        let xml = RestXmlProtocol::default().serialize_response(&output, &value);
        let body = xml.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            body.as_ref(),
            format!("<{name} xmlns=\"urn:target\" attr=\"a\"><Child>value</Child></{name}>").as_bytes()
        );
        let json = RestJson1Protocol::default().serialize_response(&output, &value);
        let body = json.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), br#"{"child":"value","attr":"a"}"#);
    }
}

#[tokio::test]
async fn modeled_error_payloads_use_the_xml_protocol_root_override() {
    static TARGET: Schema = Schema::new_struct(shape_id!("test", "Target"), ShapeType::Structure, &STRUCT_MEMBERS)
        .with_xml_name("RenamedTarget")
        .with_xml_namespace("urn:target", None);
    static PAYLOAD: Schema = Schema::new_member(
        shape_id!("test", "PayloadError", "body"),
        ShapeType::Structure,
        "body",
        0,
    )
    .with_http_payload()
    .with_xml_name("WireError");
    static ERROR: Schema = Schema::new_struct(shape_id!("test", "PayloadError"), ShapeType::Structure, &[&PAYLOAD]);
    #[derive(Debug)]
    struct PayloadError;
    impl SerializableStruct for PayloadError {
        fn schema(&self) -> &Schema<'_> {
            &ERROR
        }
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_struct(&TARGET, &TargetValue(&TARGET))
        }
    }
    impl std::fmt::Display for PayloadError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("payload error")
        }
    }
    impl std::error::Error for PayloadError {}
    impl HttpModeledError for PayloadError {
        fn status_code(&self) -> u16 {
            422
        }
    }
    let response = RestXmlProtocol::default().serialize_error(&PayloadError);
    assert_eq!(response.status(), 422);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        body.as_ref(),
        b"<WireError xmlns=\"urn:target\" attr=\"a\"><Child>value</Child></WireError>"
    );
}

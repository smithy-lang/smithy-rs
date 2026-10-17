/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Media-typed list headers retain legacy base64 encoding and round-trip through request bindings.

use aws_smithy_http_server::body::empty;
use aws_smithy_http_server::schema::protocol::{
    AwsJson1_0Protocol, AwsJson1_1Protocol, RestJson1Protocol, RestXmlProtocol,
};
use aws_smithy_http_server::schema::{HttpModeledError, ServerProtocol};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{prelude, shape_id, Schema, ShapeType};
use bytes::Bytes;

static ELEMENT: Schema = Schema::new_member(
    shape_id!("test", "MediaStrings", "member"),
    ShapeType::String,
    "member",
    0,
)
.with_media_type("application/json");
static HEADER: Schema = Schema::new_member(shape_id!("test", "Media", "items"), ShapeType::List, "items", 0)
    .with_list_member(&ELEMENT)
    .with_http_header("x-media");
static PLAIN: Schema = Schema::new_member(shape_id!("test", "Media", "plain"), ShapeType::List, "plain", 1)
    .with_list_member(&prelude::STRING)
    .with_http_header("x-plain");
static ATTRIBUTE: Schema =
    Schema::new_member(shape_id!("test", "Media", "attr"), ShapeType::String, "attr", 2).with_xml_attribute();
static OUTPUT: Schema = Schema::new_struct(
    shape_id!("test", "Media"),
    ShapeType::Structure,
    &[&HEADER, &PLAIN, &ATTRIBUTE],
);
static INPUT: Schema = Schema::new_struct(shape_id!("test", "Media"), ShapeType::Structure, &[&HEADER]);

#[derive(Debug)]
struct MediaOutput {
    values: Vec<String>,
    prelude_elements: bool,
}

impl MediaOutput {
    fn new(values: &[&str]) -> Self {
        Self {
            values: values.iter().map(|value| (*value).to_owned()).collect(),
            prelude_elements: false,
        }
    }
}

impl SerializableStruct for MediaOutput {
    fn schema(&self) -> &Schema<'_> {
        &OUTPUT
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_list(&HEADER, &|s| {
            for value in &self.values {
                let element = if self.prelude_elements {
                    &prelude::STRING
                } else {
                    &ELEMENT
                };
                s.write_string(element, value)?;
            }
            Ok(())
        })?;
        s.write_list(&PLAIN, &|s| {
            s.write_string(&prelude::STRING, "raw,quoted")?;
            s.write_string(&prelude::STRING, "a\"b")
        })?;
        s.write_string(&ATTRIBUTE, "value")
    }
}

impl std::fmt::Display for MediaOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("media header error")
    }
}

impl std::error::Error for MediaOutput {}

impl HttpModeledError for MediaOutput {
    fn status_code(&self) -> u16 {
        422
    }
}

fn header_values<'a>(headers: &'a http::HeaderMap, name: &str) -> Vec<&'a str> {
    headers
        .get_all(name)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect()
}

fn protocols() -> Vec<Box<dyn ServerProtocol>> {
    vec![
        Box::new(RestJson1Protocol::default()),
        Box::new(RestXmlProtocol::default()),
        Box::new(AwsJson1_0Protocol::default()),
        Box::new(AwsJson1_1Protocol::default()),
    ]
}

#[test]
fn media_list_elements_are_base64_encoded_in_outputs_errors_and_streaming_heads() {
    let cases: &[(&[&str], &[&str])] = &[
        (&["{}"], &["e30="]),
        (&["{}", "[1,2]"], &["e30=", "WzEsMl0="]),
        (
            &["a,b", "a\"b", "line\nbreak", "snowman ☃"],
            &["YSxi", "YSJi", "bGluZQpicmVhaw==", "c25vd21hbiDimIM="],
        ),
    ];
    for protocol in protocols() {
        for &(raw, expected) in cases {
            for prelude_elements in [false, true] {
                let mut value = MediaOutput::new(raw);
                value.prelude_elements = prelude_elements;
                for (response, status) in [
                    (protocol.serialize_response(&OUTPUT, &value), 200),
                    (protocol.serialize_error(&value), 422),
                    (protocol.serialize_streaming_response(&OUTPUT, &value, empty()), 200),
                ] {
                    assert_eq!(response.status(), status);
                    assert_eq!(header_values(response.headers(), "x-media"), expected);
                    assert_eq!(
                        header_values(response.headers(), "x-plain"),
                        ["\"raw,quoted\"", "\"a\\\"b\""]
                    );
                }
            }
        }
    }
}

#[test]
fn empty_media_list_elements_are_omitted_like_legacy() {
    for protocol in protocols() {
        for (raw, expected) in [
            (&[][..], &[][..]),
            (&[""][..], &[][..]),
            (&["", "{}", ""][..], &["e30="][..]),
        ] {
            let value = MediaOutput::new(raw);
            for response in [
                protocol.serialize_response(&OUTPUT, &value),
                protocol.serialize_error(&value),
                protocol.serialize_streaming_response(&OUTPUT, &value, empty()),
            ] {
                assert_eq!(header_values(response.headers(), "x-media"), expected);
            }
        }
    }
}

#[test]
fn media_list_response_headers_round_trip_as_repeated_or_combined_request_headers() {
    for protocol in [
        Box::new(RestJson1Protocol::default()) as Box<dyn ServerProtocol>,
        Box::new(RestXmlProtocol::default()),
    ] {
        let raw = ["{}", "[1,2]", "a,b", "a\"b", "line\nbreak", "snowman ☃"];
        let response = protocol.serialize_response(&OUTPUT, &MediaOutput::new(&raw));
        let values = header_values(response.headers(), "x-media");
        for combined in [false, true] {
            let headers = if combined {
                vec![values.join(", ")]
            } else {
                values.iter().map(|value| (*value).to_owned()).collect()
            };
            let mut request = http::Request::builder();
            for value in headers {
                request = request.header("x-media", value);
            }
            let request = aws_smithy_runtime_api::http::Request::try_from(request.body(Bytes::new()).unwrap()).unwrap();
            let mut d = protocol.deserialize_request(&INPUT, &request).unwrap();
            let mut decoded = Vec::new();
            d.read_struct(&INPUT, &mut |member, d| {
                d.read_list(member, &mut |d| {
                    decoded.push(d.read_string(&ELEMENT)?);
                    Ok(())
                })
            })
            .unwrap();
            assert_eq!(decoded, raw);
        }
    }
}

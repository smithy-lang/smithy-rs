/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Regressions for malformed modeled headers and sensitive serialization diagnostics.

use std::io::Write;
use std::sync::{Arc, Mutex};

use aws_smithy_http_server::schema::protocol::{
    AwsJson1_0Protocol, AwsJson1_1Protocol, RestJson1Protocol, RestXmlProtocol,
};
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_runtime_api::http::Request;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::{prelude, shape_id, Schema, ShapeType};
use aws_smithy_types::DateTime;
use bytes::Bytes;

fn member(kind: ShapeType) -> Schema<'static> {
    Schema::new_member(shape_id!("test", "Headers", "value"), kind, "value", 0)
}

fn output_schema<'a>(members: &'a [&'a Schema<'a>]) -> Schema<'a> {
    Schema::new_struct(shape_id!("test", "Headers"), ShapeType::Structure, members)
}

struct HeaderOutput<'a> {
    schema: &'a Schema<'a>,
    value: &'a str,
    key: &'a str,
}

impl SerializableStruct for HeaderOutput<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.schema
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let member = self.schema.members()[0];
        match member.shape_type() {
            ShapeType::List => serializer.write_list(member, &|s| s.write_string(member.member().unwrap(), self.value)),
            ShapeType::Map => serializer.write_map(member, &|s| {
                s.write_string(member.key().unwrap(), self.key)?;
                s.write_string(member.member().unwrap(), self.value)
            }),
            _ => serializer.write_string(member, self.value),
        }
    }
}

#[derive(Clone, Default)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn sensitive_header_diagnostics_are_redacted_across_protocols() {
    let secret_string = member(ShapeType::String).with_sensitive();
    let bindings = [
        member(ShapeType::String).with_http_header("x-secret").with_sensitive(),
        member(ShapeType::List)
            .with_list_member(&prelude::STRING)
            .with_http_header("x-secret")
            .with_sensitive(),
        // Collection element schemas also carry traits inherited from their targets.
        member(ShapeType::List)
            .with_list_member(&secret_string)
            .with_http_header("x-secret"),
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &prelude::STRING)
            .with_http_prefix_headers("x-secret-")
            .with_sensitive(),
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &secret_string)
            .with_http_prefix_headers("x-secret-"),
    ];
    let container_bindings = [
        member(ShapeType::String).with_http_header("x-secret"),
        member(ShapeType::List)
            .with_list_member(&prelude::STRING)
            .with_http_header("x-secret"),
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &prelude::STRING)
            .with_http_prefix_headers("x-secret-"),
    ];
    let protocols: Vec<Box<dyn ServerProtocol>> = vec![
        Box::new(RestJson1Protocol::default()),
        Box::new(RestXmlProtocol::default()),
        Box::new(AwsJson1_0Protocol::default()),
        Box::new(AwsJson1_1Protocol::default()),
    ];
    for protocol in protocols {
        for (binding, container_sensitive) in bindings
            .iter()
            .map(|binding| (binding, false))
            .chain(container_bindings.iter().map(|binding| (binding, true)))
        {
            let members = [binding];
            let schema = output_schema(&members);
            let schema = if container_sensitive {
                schema.with_sensitive()
            } else {
                schema
            };
            let value = HeaderOutput {
                schema: &schema,
                value: "secret-token\ninvalid",
                key: "token",
            };
            for streaming in [false, true] {
                let writer = LogWriter::default();
                let sink = writer.clone();
                let subscriber = tracing_subscriber::fmt()
                    .with_ansi(false)
                    .without_time()
                    .with_writer(move || sink.clone())
                    .finish();
                let response = tracing::subscriber::with_default(subscriber, || {
                    if streaming {
                        protocol.serialize_streaming_response(&schema, &value, aws_smithy_http_server::body::empty())
                    } else {
                        protocol.serialize_response(&schema, &value)
                    }
                });
                assert_eq!(response.status(), 400);
                let log = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
                assert!(log.contains("failed to serialize response"), "{log}");
                assert!(log.contains("{redacted}"), "{log}");
                assert!(!log.contains("secret-token"), "{log}");
                assert!(!log.contains("\ninvalid"), "{log}");
            }
        }
    }
}

#[test]
fn sensitive_prefix_header_keys_are_redacted() {
    let secret_key = member(ShapeType::String).with_sensitive();
    for binding in [
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &prelude::STRING)
            .with_http_prefix_headers("x-secret-")
            .with_sensitive(),
        member(ShapeType::Map)
            .with_map_members(&secret_key, &prelude::STRING)
            .with_http_prefix_headers("x-secret-"),
    ] {
        let members = [&binding];
        let schema = output_schema(&members);
        let value = HeaderOutput {
            schema: &schema,
            value: "valid",
            key: "secret-key\ninvalid",
        };
        let writer = LogWriter::default();
        let sink = writer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || sink.clone())
            .finish();
        let response = tracing::subscriber::with_default(subscriber, || {
            RestJson1Protocol::default().serialize_response(&schema, &value)
        });
        assert_eq!(response.status(), 400);
        let log = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("{redacted}"), "{log}");
        assert!(!log.contains("secret-key"), "{log}");
    }
}

fn request(name: &str, values: &[&[u8]]) -> Request<Bytes> {
    let mut request = http::Request::builder();
    for value in values {
        request = request.header(name, http::HeaderValue::from_bytes(value).unwrap());
    }
    Request::try_from(request.body(Bytes::new()).unwrap()).unwrap()
}

fn read_member(schema: &Schema<'_>, d: &mut dyn ShapeDeserializer) -> Result<(), SerdeError> {
    match schema.shape_type() {
        ShapeType::List => d.read_list(schema, &mut |d| read_member(schema.member().unwrap(), d)),
        ShapeType::Map => d.read_map(schema, &mut |_, d| read_member(schema.member().unwrap(), d)),
        ShapeType::Timestamp => d.read_timestamp(schema).map(|_| ()),
        ShapeType::Integer => d.read_integer(schema).map(|_| ()),
        _ => d.read_string(schema).map(|_| ()),
    }
}

#[test]
fn malformed_modeled_header_instances_are_rejected() {
    let strings = member(ShapeType::List).with_list_member(&prelude::STRING);
    let bindings = [
        member(ShapeType::String).with_http_header("x-value"),
        member(ShapeType::String)
            .with_media_type("application/json")
            .with_http_header("x-value"),
        member(ShapeType::Integer).with_http_header("x-value"),
        member(ShapeType::List)
            .with_list_member(&prelude::STRING)
            .with_http_header("x-value"),
        member(ShapeType::Timestamp).with_http_header("x-value"),
        member(ShapeType::List)
            .with_list_member(&prelude::TIMESTAMP)
            .with_http_header("x-value"),
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &prelude::STRING)
            .with_http_prefix_headers("X-Value"),
        member(ShapeType::Map)
            .with_map_members(&prelude::STRING, &strings)
            .with_http_prefix_headers("X-Value"),
    ];
    for protocol in [
        Box::new(RestJson1Protocol::default()) as Box<dyn ServerProtocol>,
        Box::new(RestXmlProtocol::default()),
    ] {
        for binding in &bindings {
            let valid: &[u8] = match binding.shape_type() {
                ShapeType::Timestamp => b"Thu, 01 Jan 1970 00:00:00 GMT",
                ShapeType::List if binding.member().unwrap().shape_type() == ShapeType::Timestamp => {
                    b"Thu, 01 Jan 1970 00:00:00 GMT"
                }
                ShapeType::Integer => b"1",
                _ if binding.media_type().is_some() => b"e30=",
                _ => b"valid",
            };
            for values in [vec![b"\xff".as_slice()], vec![valid, b"\xff"], vec![b"\xff", valid]] {
                let members = [binding];
                let schema = output_schema(&members);
                let request = request("x-value", &values);
                let mut d = protocol.deserialize_request(&schema, &request).unwrap();
                let mut seen = false;
                let error = d
                    .read_struct(&schema, &mut |m, d| {
                        seen = true;
                        read_member(m, d)
                    })
                    .expect_err("malformed header must not disappear")
                    .to_string();
                assert!(error.contains("valid utf-8") || error.contains("multiple"), "{error}");
                assert!(!seen, "malformed binding must fail before the member consumer");
            }
        }
    }
}

#[test]
fn valid_and_unmodeled_headers_keep_existing_behavior() {
    let binding = member(ShapeType::String).with_http_header("x-value");
    let members = [&binding];
    let schema = output_schema(&members);
    let protocol = RestJson1Protocol::default();
    let mut request = request("x-unmodeled", &[b"\xff"]);
    request.headers_mut().insert("x-value", "  valid  ");
    let mut d = protocol.deserialize_request(&schema, &request).unwrap();
    let mut value = None;
    d.read_struct(&schema, &mut |m, d| {
        value = Some(d.read_string(m)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(value.as_deref(), Some("valid"));

    let binding = member(ShapeType::List)
        .with_list_member(&prelude::TIMESTAMP)
        .with_http_header("x-date");
    let members = [&binding];
    let schema = output_schema(&members);
    let request = self::request(
        "x-date",
        &[b"Thu, 01 Jan 1970 00:00:00 GMT", b"Fri, 02 Jan 1970 00:00:00 GMT"],
    );
    let mut d = protocol.deserialize_request(&schema, &request).unwrap();
    let mut dates = Vec::new();
    d.read_struct(&schema, &mut |m, d| {
        d.read_list(m, &mut |d| {
            dates.push(d.read_timestamp(&prelude::TIMESTAMP)?);
            Ok(())
        })
    })
    .unwrap();
    assert_eq!(dates, [DateTime::from_secs(0), DateTime::from_secs(86400)]);
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Sensitive values must not reach logs on codec serialization failures.

use aws_smithy_http_server::schema::protocol::{AwsJson1_0, AwsJson1_1, RestJson1};
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType};

#[derive(Clone, Default)]
struct LogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

static SECRET_DEC: Schema = Schema::new_member(
    shape_id!("test", "Secret", "amount"),
    ShapeType::BigDecimal,
    "amount",
    0,
)
.with_sensitive();

static SECRET: Schema =
    Schema::new_struct(shape_id!("test", "Secret"), ShapeType::Structure, &[&SECRET_DEC]).with_original_name("Secret");

struct Secret;

impl SerializableStruct for Secret {
    fn schema(&self) -> &Schema<'_> {
        &SECRET
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        // Accepted by `BigDecimal::from_str` (leading `+`, leading zeros) but not a JSON number.
        let value: aws_smithy_types::BigDecimal = "+000987654321".parse().unwrap();
        s.write_big_decimal(&SECRET_DEC, &value)
    }
}

#[test]
fn sensitive_body_values_are_not_logged_on_serialization_failure() {
    let protocols: [&dyn ServerProtocol; 3] = [&RestJson1::default(), &AwsJson1_0::default(), &AwsJson1_1::default()];
    for protocol in protocols {
        let writer = LogWriter::default();
        let sink = writer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || sink.clone())
            .finish();
        let response = tracing::subscriber::with_default(subscriber, || protocol.serialize_response(&SECRET, &Secret));
        let log = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
        assert!(
            !log.contains("987654321"),
            "{}: status {} leaked sensitive value into log: {log}",
            protocol.protocol_id(),
            response.status()
        );
    }
}

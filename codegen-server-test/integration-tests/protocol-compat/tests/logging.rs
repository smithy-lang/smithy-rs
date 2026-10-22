/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Review finding: `@sensitive` values must not reach logs when serialization fails.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::{awsjson, rest};
use std::sync::{Arc, Mutex};
use support::*;

#[derive(Clone, Default)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn logged<F: std::future::Future<Output = Reply>>(future: F) -> (Reply, String) {
    let writer = LogWriter::default();
    let sink = writer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || sink.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let reply = future.await;
    let log = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
    (reply, log)
}

#[tokio::test]
async fn sensitive_values_are_not_logged_when_response_serialization_fails() {
    let (reply, log) = logged(send(
        rest::service(),
        rest("POST", "/sensitive-timestamp", &[], b""),
    ))
    .await;
    assert_eq!(reply.status, 400);
    assert!(
        !log.contains("9223372036854775807"),
        "restJson1 log leaked: {log}"
    );
    let (reply, log) = logged(send(
        awsjson::service(),
        aws_json("1.0", "ReviewAwsJson", "SensitiveTimestamp", "{}"),
    ))
    .await;
    assert_eq!(reply.status, 400);
    assert!(
        !log.contains("9223372036854775807"),
        "awsJson1_0 log leaked: {log}"
    );
}

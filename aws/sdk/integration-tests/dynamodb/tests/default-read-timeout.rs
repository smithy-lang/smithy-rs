/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The per-service default read timeout, against the generated `aws-sdk-dynamodb` crate.
//!
//! The codegen tests cover this with synthetic models. This covers the crate a customer actually
//! gets. DynamoDB is absent from the shared timeout-defaults artifact, so it takes the 5-minute
//! general default once the opt-in is set.
//!
//! This lives in its own file, and therefore its own test binary, because the opt-in is process
//! state: a test that needs it unset cannot share a process with one that needs it set.

use std::time::Duration;

use aws_sdk_dynamodb::config::Region;
use aws_smithy_http_client::test_util::NeverClient;
use aws_smithy_types::retry::RetryConfig;

const OPT_IN: &str = "AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026";

/// Captures the `read_timeout` the orchestrator hands to the HTTP client for one request.
///
/// The read timeout is enforced inside the connector, so `HttpConnectorSettings` is where a
/// per-service default either arrives or silently does not.
async fn captured_read_timeout() -> Option<Duration> {
    use aws_smithy_runtime_api::client::http::{http_client_fn, HttpClient};
    use std::sync::{Arc, Mutex};

    let captured: Arc<Mutex<Option<Option<Duration>>>> = Default::default();
    let sink = captured.clone();
    let inner = NeverClient::new();
    let http_client = http_client_fn(move |settings, components| {
        *sink.lock().unwrap() = Some(settings.read_timeout());
        inner.http_connector(settings, components)
    });

    let conf = aws_sdk_dynamodb::Config::builder()
        .with_test_defaults()
        .region(Region::new("us-east-2"))
        .http_client(http_client)
        .retry_config(RetryConfig::disabled())
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(conf);

    // `NeverClient` never responds, so bound the call; the settings are captured before the
    // connector is ever polled.
    let _ = tokio::time::timeout(Duration::from_millis(300), client.list_tables().send()).await;

    let seen = captured.lock().unwrap().take();
    seen.expect("the HTTP client must have been asked for a connector")
}

/// Both states of the opt-in, in one test.
///
/// Deliberately not two `#[test]`s: they would run on parallel threads in this binary and
/// overwrite each other's environment.
#[tokio::test]
async fn the_opt_in_gates_the_default_read_timeout() {
    std::env::remove_var(OPT_IN);
    assert_eq!(
        captured_read_timeout().await,
        None,
        "without the opt-in, a real DynamoDB client must get no default read timeout"
    );

    std::env::set_var(OPT_IN, "true");
    assert_eq!(
        captured_read_timeout().await,
        Some(Duration::from_secs(300)),
        "with the opt-in, an unlisted service takes the 5-minute general default"
    );

    std::env::remove_var(OPT_IN);
}

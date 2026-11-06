/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::Duration;

use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_dynamodb::config::{Credentials, Region, StalledStreamProtectionConfig};
use aws_sdk_dynamodb::error::SdkError;
use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep, Sleep};
use aws_smithy_http_client::test_util::NeverClient;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::SdkConfig;

#[derive(Debug, Clone)]
struct InstantSleep;
impl AsyncSleep for InstantSleep {
    fn sleep(&self, _duration: Duration) -> Sleep {
        Sleep::new(Box::pin(async move {}))
    }
}

#[tokio::test]
async fn api_call_timeout_retries() {
    let http_client = NeverClient::new();
    let conf = SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .http_client(http_client.clone())
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .timeout_config(
            TimeoutConfig::builder()
                .operation_attempt_timeout(Duration::new(123, 0))
                .build(),
        )
        .retry_config(RetryConfig::standard())
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .sleep_impl(SharedAsyncSleep::new(InstantSleep))
        .build();
    let client = aws_sdk_dynamodb::Client::new(&conf);
    let resp = client
        .list_tables()
        .send()
        .await
        .expect_err("call should fail");
    assert_eq!(
        http_client.num_calls(),
        3,
        "client level timeouts should be retried"
    );
    assert!(
        matches!(resp, SdkError::TimeoutError { .. }),
        "expected a timeout error, got: {}",
        resp
    );
}

#[tokio::test]
async fn no_retries_on_operation_timeout() {
    let http_client = NeverClient::new();
    let conf = SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .http_client(http_client.clone())
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .timeout_config(
            TimeoutConfig::builder()
                .operation_timeout(Duration::new(123, 0))
                .build(),
        )
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .retry_config(RetryConfig::standard())
        .sleep_impl(SharedAsyncSleep::new(InstantSleep))
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(aws_sdk_dynamodb::Config::new(&conf));
    let resp = client
        .list_tables()
        .send()
        .await
        .expect_err("call should fail");
    assert_eq!(
        http_client.num_calls(),
        1,
        "operation level timeouts should not be retried"
    );
    assert!(
        matches!(resp, SdkError::TimeoutError { .. }),
        "expected a timeout error, got: {}",
        resp
    );
}

/// Captures the `read_timeout` the orchestrator hands to the HTTP client for one request.
///
/// The read timeout is enforced inside the connector, so the `HttpConnectorSettings` are where a
/// per-service default either arrives or silently does not.
async fn captured_read_timeout(conf: aws_sdk_dynamodb::Config) -> Option<Duration> {
    use aws_smithy_runtime_api::client::http::{http_client_fn, HttpClient};
    use std::sync::{Arc, Mutex};

    let captured: Arc<Mutex<Option<Option<Duration>>>> = Default::default();
    let sink = captured.clone();
    let inner = NeverClient::new();
    let http_client = http_client_fn(move |settings, components| {
        *sink.lock().unwrap() = Some(settings.read_timeout());
        inner.http_connector(settings, components)
    });

    let conf = conf
        .to_builder()
        .http_client(http_client)
        .retry_config(RetryConfig::disabled())
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(conf);

    // `NeverClient` never responds, so bound the call; the settings are captured before the
    // connector is polled.
    let _ = tokio::time::timeout(Duration::from_millis(300), client.list_tables().send()).await;

    let seen = captured.lock().unwrap().take();
    seen.expect("the HTTP client must have been asked for a connector")
}

/// DynamoDB is absent from the shared timeout-defaults artifact, so it takes the 5-minute general
/// default — and that value has to reach the HTTP client, not just the config bag.
///
/// The codegen tests assert this against synthetic models; this asserts it against the generated
/// `aws-sdk-dynamodb` crate a customer actually gets.
#[tokio::test]
async fn default_read_timeout_is_five_minutes() {
    let conf = aws_sdk_dynamodb::Config::builder()
        .with_test_defaults()
        .region(Region::new("us-east-2"))
        .build();
    assert_eq!(
        captured_read_timeout(conf).await,
        Some(Duration::from_secs(300)),
        "an unlisted service takes the 5-minute general default"
    );
}

/// The default is gated: a caller pinned before `v2026_11_15` keeps the previous behavior.
#[tokio::test]
#[allow(deprecated)]
async fn no_default_read_timeout_on_the_previous_behavior_version() {
    use aws_sdk_dynamodb::config::BehaviorVersion;

    let conf = aws_sdk_dynamodb::Config::builder()
        .with_test_defaults()
        .region(Region::new("us-east-2"))
        .behavior_version(BehaviorVersion::v2026_01_12())
        .build();
    assert_eq!(
        captured_read_timeout(conf).await,
        None,
        "callers pinned before v2026_11_15 must keep the old behavior"
    );
}

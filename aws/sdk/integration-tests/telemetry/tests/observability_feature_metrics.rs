/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_sdk_s3::config::{Credentials, SharedCredentialsProvider};
use aws_smithy_observability::TelemetryProvider;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use serial_test::serial;
use utils::init_metrics;

mod utils;

// Note: These tests are written with a multi-threaded runtime since OTel requires that to work
// and they are all run serially since they touch global state

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn observability_otel_metrics_feature_tracked_in_user_agent() {
    let (meter_provider, _exporter) = init_metrics();

    // Create a replay client to capture the actual HTTP request
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder().body(SdkBody::empty()).unwrap(),
        http::Response::builder().body(SdkBody::empty()).unwrap(),
    )]);

    let config = aws_config::SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();

    let s3_client = aws_sdk_s3::Client::new(&config);
    let _ = s3_client
        .get_object()
        .bucket("test-bucket")
        .key("test.txt")
        .send()
        .await;

    // Get the actual HTTP request that was made
    let requests = http_client.actual_requests();
    let last_request = requests.last().expect("should have made a request");

    let user_agent = last_request
        .headers()
        .get("x-amz-user-agent")
        .expect("should have user-agent header");

    // Should contain OBSERVABILITY_OTEL_METRICS metric (value "7")
    // The metric appears in the m/ section, e.g. "m/E,b,7"
    assert!(
        user_agent.contains(",7") || user_agent.ends_with("/7") || user_agent.contains("7,"),
        "User-Agent should contain observability OTel metrics feature (7), got: {}",
        user_agent
    );

    meter_provider.flush().unwrap();

    // Reset to noop for other tests
    aws_smithy_observability::global::set_telemetry_provider(TelemetryProvider::noop()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn noop_provider_does_not_track_observability_metrics() {
    // Reset to noop provider
    aws_smithy_observability::global::set_telemetry_provider(TelemetryProvider::noop()).unwrap();

    // Create a replay client to capture the actual HTTP request
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder().body(SdkBody::empty()).unwrap(),
        http::Response::builder().body(SdkBody::empty()).unwrap(),
    )]);

    let config = aws_config::SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();

    let s3_client = aws_sdk_s3::Client::new(&config);
    let _ = s3_client
        .get_object()
        .bucket("test-bucket")
        .key("test.txt")
        .send()
        .await;

    // Get the actual HTTP request that was made
    let requests = http_client.actual_requests();
    let last_request = requests.last().expect("should have made a request");

    let user_agent = last_request
        .headers()
        .get("x-amz-user-agent")
        .expect("should have user-agent header");

    // Should NOT contain OBSERVABILITY_OTEL_METRICS metric when using noop provider
    // The metric value is "7"
    assert!(
        !user_agent.contains(",7") && !user_agent.ends_with("/7") && !user_agent.contains("7,"),
        "User-Agent should not contain observability OTel metrics feature (7) with noop provider, got: {}",
        user_agent
    );
}

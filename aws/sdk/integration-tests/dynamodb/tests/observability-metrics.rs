/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_credential_types::Credentials;
use aws_runtime::user_agent::test_util::assert_ua_contains_metric_values;
use aws_sdk_dynamodb::config::Builder;
use aws_smithy_observability::TelemetryProvider;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use http::header::USER_AGENT;
use std::sync::Arc;

fn test_client(update_builder: fn(Builder) -> Builder) -> (aws_sdk_dynamodb::Client, StaticReplayClient) {
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder()
            .uri("https://dynamodb.us-east-1.amazonaws.com/")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder()
            .status(200)
            .body(SdkBody::from(r#"{"TableNames":[]}"#))
            .unwrap(),
    )]);

    let config = update_builder(
        aws_sdk_dynamodb::Config::builder()
            .credentials_provider(Credentials::for_tests_with_account_id())
            .region(Region::from_static("us-east-1"))
            .http_client(http_client.clone()),
    )
    .build();

    (aws_sdk_dynamodb::Client::from_conf(config), http_client)
}

async fn call_operation(client: aws_sdk_dynamodb::Client) {
    let _ = client.list_tables().send().await;
}

#[tokio::test]
async fn observability_metrics_in_user_agent() {
    // Test case 1: No telemetry provider configured (default noop)
    {
        let (client, rx) = test_client(std::convert::identity);
        call_operation(client).await;
        let req = rx.expect_request();
        let user_agent = req.headers().get("x-amz-user-agent").unwrap();
        
        // Should NOT contain observability metrics when using noop provider
        let ua_str = user_agent.to_str().unwrap();
        assert!(!ua_str.contains("m/4")); // OBSERVABILITY_TRACING = "4"
        assert!(!ua_str.contains("m/6")); // OBSERVABILITY_OTEL_TRACING = "6"
        assert!(!ua_str.contains("m/7")); // OBSERVABILITY_OTEL_METRICS = "7"
    }

    // Test case 2: OpenTelemetry metrics provider configured
    {
        use aws_smithy_observability_otel::meter::OtelMeterProvider;
        use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
        use opentelemetry_sdk::runtime::Tokio;
        use opentelemetry_sdk::testing::metrics::InMemoryMetricsExporter;

        // Create OTel meter provider
        let exporter = InMemoryMetricsExporter::default();
        let reader = PeriodicReader::builder(exporter.clone(), Tokio).build();
        let otel_mp = SdkMeterProvider::builder().with_reader(reader).build();
        let sdk_mp = Arc::new(OtelMeterProvider::new(otel_mp));
        let sdk_tp = TelemetryProvider::builder().meter_provider(sdk_mp).build();

        // Set global telemetry provider
        aws_smithy_observability::global::set_telemetry_provider(sdk_tp).unwrap();

        let (client, rx) = test_client(std::convert::identity);
        call_operation(client).await;
        let req = rx.expect_request();
        let user_agent = req.headers().get("x-amz-user-agent").unwrap();

        // Should contain OBSERVABILITY_OTEL_METRICS metric
        assert_ua_contains_metric_values(user_agent.to_str().unwrap(), &["7"]);

        // Reset to noop for other tests
        aws_smithy_observability::global::set_telemetry_provider(TelemetryProvider::noop())
            .unwrap();
    }
}

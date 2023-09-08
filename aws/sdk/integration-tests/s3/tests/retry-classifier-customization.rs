/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::interceptors::InterceptorContext;
use aws_sdk_s3::config::retry::{ClassifyRetry, ReconnectMode, RetryClassifierResult, RetryConfig};
use aws_sdk_s3::config::timeout::TimeoutConfig;
use aws_sdk_s3::config::{Credentials, Region, SharedAsyncSleep};
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_client::test_connection::wire_mock::{
    check_matches, ReplayedEvent, WireLevelTestConnection,
};
use aws_smithy_client::{ev, match_events};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct CustomRetryClassifier {
    counter: Arc<Mutex<u8>>,
}

impl CustomRetryClassifier {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0u8)),
        }
    }

    pub fn counter(&self) -> u8 {
        *self.counter.lock().unwrap()
    }
}

impl ClassifyRetry for CustomRetryClassifier {
    fn classify_retry(
        &self,
        _: &InterceptorContext,
        _: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult> {
        *self.counter.lock().unwrap() += 1;

        Some(RetryClassifierResult::DontRetry)
    }

    fn name(&self) -> &'static str {
        "Custom Retry Classifier"
    }
}

#[tokio::test]
async fn test_retry_classifier_customization() {
    tracing_subscriber::fmt::init();
    let mock = WireLevelTestConnection::spinup(vec![
        ReplayedEvent::status(503),
        ReplayedEvent::status(503),
        ReplayedEvent::with_body("here-is-your-object"),
    ])
    .await;

    let custom_retry_classifier = CustomRetryClassifier::new();

    let config = aws_sdk_s3::Config::builder()
        .region(Region::from_static("us-east-2"))
        .credentials_provider(Credentials::for_tests())
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .endpoint_url(mock.endpoint_url())
        .http_connector(mock.http_connector())
        .retry_config(
            RetryConfig::standard().with_reconnect_mode(ReconnectMode::ReuseAllConnections),
        )
        .timeout_config(TimeoutConfig::disabled())
        .retry_classifier(custom_retry_classifier.clone())
        .build();

    let client = aws_sdk_s3::Client::from_conf(config);
    let _ = client
        .get_object()
        .bucket("bucket")
        .key("key")
        .send()
        .await
        .expect_err("fails without attempting a retry");

    // ensure our classifier was called
    assert_eq!(1, custom_retry_classifier.counter());

    match_events!(ev!(dns), ev!(connect), ev!(http(503)))(&mock.events());
}

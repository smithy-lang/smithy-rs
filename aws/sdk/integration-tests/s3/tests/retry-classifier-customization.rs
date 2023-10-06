/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::interceptors::InterceptorContext;
use aws_sdk_s3::config::retry::{ClassifyRetry, RetryAction, RetryConfig};
use aws_sdk_s3::config::SharedAsyncSleep;
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;
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
        ctx: &InterceptorContext,
        _: Option<RetryAction>,
    ) -> Option<RetryAction> {
        *self.counter.lock().unwrap() += 1;

        // Interceptors may call this classifier before a response is received. If a response was received,
        // ensure that it has the expected status code.
        if let Some(res) = ctx.response() {
            assert_eq!(
                res.status(),
                500,
                "expected a 500 response from test connection"
            );
        }

        Some(RetryAction::NoRetry)
    }

    fn name(&self) -> &'static str {
        "Custom Retry Classifier"
    }
}

fn req() -> http::Request<SdkBody> {
    http::Request::builder()
        .body(SdkBody::from("request body"))
        .unwrap()
}

fn ok() -> http::Response<&'static str> {
    http::Response::builder()
        .status(200)
        .body("Hello!")
        .unwrap()
}

fn err() -> http::Response<&'static str> {
    http::Response::builder()
        .status(500)
        .body("This was an error")
        .unwrap()
}

#[tokio::test]
async fn test_retry_classifier_customization() {
    tracing_subscriber::fmt::init();
    let test_connection = TestConnection::new(vec![(req(), err()), (req(), ok())]);

    let custom_retry_classifier = CustomRetryClassifier::new();

    let config = aws_sdk_s3::Config::builder()
        .with_test_defaults()
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .http_connector(test_connection)
        .retry_config(RetryConfig::standard())
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

    // ensure our custom retry classifier was called at least once.
    assert_ne!(custom_retry_classifier.counter(), 0);
}

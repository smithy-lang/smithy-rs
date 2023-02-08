/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_dynamodb::types::SdkError;
use aws_smithy_async::rt::sleep::{default_async_sleep, AsyncSleep, Sleep};
use aws_smithy_client::http_connector::ConnectorSettings;
use aws_smithy_client::hyper_ext;
use aws_smithy_client::never::NeverConnector;
use aws_smithy_types::error::display::DisplayErrorContext;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::region::Region;
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
    let conn = NeverConnector::new();
    let conf = SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .http_connector(conn.clone())
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .timeout_config(
            TimeoutConfig::builder()
                .operation_attempt_timeout(Duration::new(123, 0))
                .build(),
        )
        .retry_config(RetryConfig::standard())
        .sleep_impl(Arc::new(InstantSleep))
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(aws_sdk_dynamodb::Config::new(&conf));
    let resp = client
        .list_tables()
        .send()
        .await
        .expect_err("call should fail");
    assert_eq!(
        conn.num_calls(),
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
async fn building_client_from_sdk_config_allows_to_override_connect_timeout() {
    let connect_timeout_value = 100;
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .build();
    let conf = SdkConfig::builder()
        .region(Some(Region::from_static("us-east-1")))
        .endpoint_url(
            // Emulate a connect timeout error by hitting an unroutable IP
            "http://172.255.255.0:18104",
        )
        .http_connector(
            hyper_ext::Adapter::builder()
                // // feel free to uncomment this if you want to make sure that the test pass when timeout is correctly set
                // .connector_settings(
                //     ConnectorSettings::builder()
                //         .connect_timeout(Duration::from_millis(connect_timeout_value))
                //         .build(),
                // )
                .build(connector),
        )
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .retry_config(RetryConfig::disabled())
        .sleep_impl(default_async_sleep().unwrap())
        .build();

    // overwrite connect_timeout config with Client::from_conf
    let client = aws_sdk_dynamodb::Client::from_conf(
        aws_sdk_dynamodb::config::Builder::from(&conf)
            .timeout_config(
                TimeoutConfig::builder()
                    .connect_timeout(Duration::from_millis(connect_timeout_value))
                    .build(),
            )
            .build(),
    );

    if let Ok(result) =
        tokio::time::timeout(Duration::from_millis(1000), client.list_tables().send()).await
    {
        match result {
            Ok(_) => panic!("should not have succeeded"),
            Err(err) => {
                let message = format!("{}", DisplayErrorContext(&err));
                let expected =
                    format!("timeout: error trying to connect: HTTP connect timeout occurred after {connect_timeout_value}ms");
                assert!(
                    message.contains(&expected),
                    "expected '{message}' to contain '{expected}'"
                );
            }
        }
    } else {
        panic!("the client didn't timeout");
    }
}

#[tokio::test]
async fn no_retries_on_operation_timeout() {
    let conn = NeverConnector::new();
    let conf = SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .http_connector(conn.clone())
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .timeout_config(
            TimeoutConfig::builder()
                .operation_timeout(Duration::new(123, 0))
                .build(),
        )
        .retry_config(RetryConfig::standard())
        .sleep_impl(Arc::new(InstantSleep))
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(aws_sdk_dynamodb::Config::new(&conf));
    let resp = client
        .list_tables()
        .send()
        .await
        .expect_err("call should fail");
    assert_eq!(
        conn.num_calls(),
        1,
        "operation level timeouts should not be retried"
    );
    assert!(
        matches!(resp, SdkError::TimeoutError { .. }),
        "expected a timeout error, got: {}",
        resp
    );
}

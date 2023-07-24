/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::retry::RetryConfig;
use aws_credential_types::Credentials;
use aws_smithy_async::time::StaticTimeSource;
use aws_smithy_client::http_connector::HttpConnector;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::region::Region;
use std::future::Future;
use std::time::{Duration, UNIX_EPOCH};

pub(crate) fn get_default_config(
    connector: impl Into<HttpConnector>,
) -> impl Future<Output = aws_config::SdkConfig> {
    aws_config::from_env()
        .region(Region::from_static("us-west-2"))
        .credentials_provider(Credentials::from_keys(
            "access_key",
            "secret_key",
            Some("session_token".to_string()),
        ))
        .timeout_config(TimeoutConfig::disabled())
        .time_source(StaticTimeSource::new(
            UNIX_EPOCH + Duration::from_secs(1234567890),
        ))
        .retry_config(RetryConfig::disabled())
        .http_connector(connector)
        .load()
}

#[cfg(test)]
mod test {
    use crate::default_config::get_default_config;
    use aws_smithy_client::test_connection::capture_request;

    #[tokio::test]
    pub async fn test_default_config() {
        let (connector, req) = capture_request(None);
        let shared_config = get_default_config(connector).await;
        let client = aws_sdk_s3::Client::new(&shared_config);
        assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2");
        req.expect_no_request();
    }
}

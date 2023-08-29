/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::{retry::RetryConfig, timeout::TimeoutConfig};

pub(crate) async fn get_default_config() -> aws_config::SdkConfig {
    aws_config::from_env()
        .region("us-east-2")
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .no_credentials()
        .load()
        .await
}

#[tokio::test]
pub async fn test_default_config() {
    let shared_config = get_default_config().await;
    let client = aws_sdk_s3::Client::new(&shared_config);
    assert_eq!(client.conf().region().unwrap().to_string(), "us-east-2")
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::{retry::RetryConfig, timeout::TimeoutConfig};
use aws_types::region::Region;
use std::future::Future;

pub(crate) fn get_default_config() -> impl Future<Output = aws_config::SdkConfig> {
    aws_config::from_env()
        .region(Region::from_static("us-west-2"))
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .load()
}

#[tokio::test]
pub async fn test_default_config() {
    let shared_config = get_default_config().await;
    let client = aws_sdk_s3::Client::new(&shared_config);
    assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2")
}

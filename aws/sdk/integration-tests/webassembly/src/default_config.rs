/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::environment::EnvironmentVariableCredentialsProvider;
use aws_credential_types::cache::CredentialsCache;
use aws_smithy_async::rt::sleep::WasiSleep;
use aws_smithy_types::{retry::RetryConfig, timeout::TimeoutConfig};

fn credentials_cache() -> CredentialsCache {
    CredentialsCache::lazy_builder()
        .sleep(std::sync::Arc::new(WasiSleep::default()))
        .into_credentials_cache()
}

pub(crate) async fn get_default_config() -> aws_config::SdkConfig {
    aws_config::from_env()
        .region("us-west-2")
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .sleep_impl(WasiSleep::default())
        .credentials_cache(credentials_cache())
        .credentials_provider(EnvironmentVariableCredentialsProvider::default())
        .load()
        .await
}

#[tokio::test]
pub async fn test_default_config() {
    let shared_config = get_default_config().await;
    let client = aws_sdk_s3::Client::new(&shared_config);
    assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2")
}

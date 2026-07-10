/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tests for AWS_IGNORE_CONFIGURED_ENDPOINT_URLS behavior with service-specific endpoints.
//!
//! Per https://docs.aws.amazon.com/sdkref/latest/guide/feature-ss-endpoints.html:
//!
//! When `ignore_configured_endpoint_urls` is `true`:
//! > "The SDK or tool does not read any custom configuration options from the shared config
//! > file or from environment variables for setting an endpoint URL."
//!
//! This means both the global `AWS_ENDPOINT_URL` AND service-specific
//! `AWS_ENDPOINT_URL_<SERVICE>` (e.g. `AWS_ENDPOINT_URL_DYNAMODB`) must be suppressed.
//!
//! The only exception is an explicit endpoint set programmatically in code:
//! > "Note that any explicit endpoint set in the code or on a service client itself is used
//! > regardless of this setting."

use aws_smithy_http_client::test_util::capture_request;
use aws_types::os_shim_internal::Env;
use http_1x::Uri;

/// When `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS=true`, the service-specific endpoint URL
/// set via `AWS_ENDPOINT_URL_DYNAMODB` should NOT be used.
///
/// Expected: Request goes to https://dynamodb.us-east-1.amazonaws.com/
#[tokio::test]
async fn ignore_configured_endpoint_urls_should_suppress_service_specific_env_var() {
    let (http_client, request) = capture_request(None);

    let sdk_config = aws_config::defaults(aws_sdk_dynamodb::config::BehaviorVersion::latest())
        .http_client(http_client)
        .test_credentials()
        .region(aws_sdk_dynamodb::config::Region::new("us-east-1"))
        .env(Env::from_slice(&[
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "true"),
            ("AWS_ENDPOINT_URL_DYNAMODB", "http://localhost:9999"),
        ]))
        .load()
        .await;

    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let _ = client.list_tables().send().await;

    let req = request.expect_request();
    let actual_uri = req.uri();
    assert_eq!(
        actual_uri,
        &Uri::from_static("https://dynamodb.us-east-1.amazonaws.com/"),
        "When ignore_configured_endpoint_urls is true, service-specific endpoint URLs \
         (AWS_ENDPOINT_URL_DYNAMODB) should be suppressed, but the request was sent to {actual_uri}"
    );
}

/// The global `AWS_ENDPOINT_URL` should also be suppressed when the ignore flag is set.
#[tokio::test]
async fn ignore_configured_endpoint_urls_should_suppress_global_env_var() {
    let (http_client, request) = capture_request(None);

    let sdk_config = aws_config::defaults(aws_sdk_dynamodb::config::BehaviorVersion::latest())
        .http_client(http_client)
        .test_credentials()
        .region(aws_sdk_dynamodb::config::Region::new("us-east-1"))
        .env(Env::from_slice(&[
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "true"),
            ("AWS_ENDPOINT_URL", "http://localhost:8888"),
        ]))
        .load()
        .await;

    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let _ = client.list_tables().send().await;

    assert_eq!(
        request.expect_request().uri(),
        &Uri::from_static("https://dynamodb.us-east-1.amazonaws.com/"),
    );
}

/// Service-specific endpoint URL set in the shared config file's `[services]` section
/// should also be suppressed when `ignore_configured_endpoint_urls` is true.
#[tokio::test]
async fn ignore_configured_endpoint_urls_should_suppress_service_specific_profile() {
    #[allow(deprecated)]
    use aws_config::profile::profile_file::{ProfileFileKind, ProfileFiles};
    use aws_types::os_shim_internal::Fs;

    let (http_client, request) = capture_request(None);

    let config_content = r#"
[profile custom]
ignore_configured_endpoint_urls = true
services = my-services

[services my-services]
dynamodb =
  endpoint_url = http://localhost:7777
"#;

    let sdk_config = aws_config::defaults(aws_sdk_dynamodb::config::BehaviorVersion::latest())
        .http_client(http_client)
        .test_credentials()
        .region(aws_sdk_dynamodb::config::Region::new("us-east-1"))
        .env(Env::from_slice(&[]))
        .fs(Fs::from_slice(&[("test_config", config_content)]))
        .profile_name("custom")
        .profile_files(
            #[allow(deprecated)]
            ProfileFiles::builder()
                .with_file(
                    #[allow(deprecated)]
                    ProfileFileKind::Config,
                    "test_config",
                )
                .build(),
        )
        .load()
        .await;

    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let _ = client.list_tables().send().await;

    let req = request.expect_request();
    let actual_uri = req.uri();
    assert_eq!(
        actual_uri,
        &Uri::from_static("https://dynamodb.us-east-1.amazonaws.com/"),
        "When ignore_configured_endpoint_urls is true, service-specific endpoint URLs \
         from the profile config should be suppressed, but the request was sent to {actual_uri}"
    );
}

/// Per the spec: "any explicit endpoint set in the code or on a service client itself
/// is used regardless of this setting."
#[tokio::test]
async fn programmatic_endpoint_url_not_suppressed_by_ignore_flag() {
    let (http_client, request) = capture_request(None);

    let sdk_config = aws_config::defaults(aws_sdk_dynamodb::config::BehaviorVersion::latest())
        .http_client(http_client)
        .test_credentials()
        .region(aws_sdk_dynamodb::config::Region::new("us-east-1"))
        .endpoint_url("http://programmatic-endpoint:8080")
        .env(Env::from_slice(&[
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "true"),
            ("AWS_ENDPOINT_URL_DYNAMODB", "http://localhost:9999"),
        ]))
        .load()
        .await;

    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let _ = client.list_tables().send().await;

    assert_eq!(
        request.expect_request().uri(),
        &Uri::from_static("http://programmatic-endpoint:8080/"),
    );
}

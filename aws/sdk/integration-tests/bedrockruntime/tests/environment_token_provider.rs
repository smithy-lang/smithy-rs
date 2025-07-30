/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_runtime::env_config::EnvConfigLoader;
use aws_runtime::user_agent::test_util::assert_ua_contains_metric_values;
use aws_sdk_bedrockruntime::config::{Region, Token};
use aws_sdk_bedrockruntime::error::DisplayErrorContext;
use aws_sdk_bedrockruntime::{Client, Config};
use aws_smithy_http_client::test_util::capture_request;
use aws_smithy_runtime::assert_str_contains;
use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
use aws_types::origin::Origin;
use aws_types::os_shim_internal::Env;
use aws_types::SdkConfig;

enum ClientBuilder {
    SdkConfig,
    RawConfig,
}

async fn create_client_with_env(
    builder_type: ClientBuilder,
    http_client: aws_smithy_runtime::client::http::test_util::CaptureRequestHandler,
    env_vars: &[(&str, &str)],
) -> Client {
    match builder_type {
        ClientBuilder::SdkConfig => {
            let shared_config = SdkConfig::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .service_config(
                    EnvConfigLoader::builder()
                        .env(Env::from_slice(env_vars))
                        .build(),
                )
                .build();
            Client::new(&shared_config)
        }
        ClientBuilder::RawConfig => {
            let conf = Config::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .with_env(env_vars)
                .build();
            Client::from_conf(conf)
        }
    }
}

async fn test_valid_service_specific_token_configured_impl(builder_type: ClientBuilder) {
    let (http_client, captured_request) = capture_request(None);
    let expected_token = "bedrock-token";
    let client = create_client_with_env(
        builder_type,
        http_client,
        &[("AWS_BEARER_TOKEN_BEDROCK", expected_token)],
    )
    .await;
    let _ = client
        .get_async_invoke()
        .invocation_arn("arn:aws:bedrock:us-west-2:123456789012:invoke/ExampleModel")
        .send()
        .await;
    let request = captured_request.expect_request();
    let authorization_header = request.headers().get("authorization").unwrap();
    assert!(authorization_header.starts_with(&format!("Bearer {expected_token}")));

    // Verify that the user agent contains the expected metric value.
    let user_agent = request.headers().get("x-amz-user-agent").unwrap();
    assert_ua_contains_metric_values(user_agent, &["3"]);
}

#[tokio::test]
async fn test_valid_service_specific_token_configured() {
    test_valid_service_specific_token_configured_impl(ClientBuilder::SdkConfig).await;
}

#[tokio::test]
async fn test_valid_service_specific_token_configured_raw_config() {
    test_valid_service_specific_token_configured_impl(ClientBuilder::RawConfig).await;
}

async fn test_token_configured_for_different_service_impl(builder_type: ClientBuilder) {
    let (http_client, _) = capture_request(None);
    let client = create_client_with_env(
        builder_type,
        http_client,
        &[("AWS_BEARER_TOKEN_FOO", "foo-token")],
    )
    .await;
    let err = client
        .get_async_invoke()
        .invocation_arn("arn:aws:bedrock:us-west-2:123456789012:invoke/ExampleModel")
        .send()
        .await
        .unwrap_err();
    assert_str_contains!(
        format!("{}", DisplayErrorContext(err)),
        "failed to select an auth scheme to sign the request with."
    );
}

#[tokio::test]
async fn test_token_configured_for_different_service() {
    test_token_configured_for_different_service_impl(ClientBuilder::SdkConfig).await;
}

#[tokio::test]
async fn test_token_configured_for_different_service_raw_config() {
    test_token_configured_for_different_service_impl(ClientBuilder::RawConfig).await;
}

async fn test_token_configured_with_auth_scheme_preference_also_set_in_env_impl(
    builder_type: ClientBuilder,
) {
    let (http_client, captured_request) = capture_request(None);
    let expected_token = "bedrock-token";

    let client = match builder_type {
        ClientBuilder::SdkConfig => {
            let mut shared_config = SdkConfig::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .service_config(
                    EnvConfigLoader::builder()
                        .env(Env::from_slice(&[(
                            "AWS_BEARER_TOKEN_BEDROCK",
                            expected_token,
                        )]))
                        .build(),
                )
                .auth_scheme_preference([
                    aws_runtime::auth::sigv4::SCHEME_ID,
                    HTTP_BEARER_AUTH_SCHEME_ID,
                ]);
            shared_config.insert_origin(
                "auth_scheme_preference",
                Origin::shared_environment_variable(),
            );
            Client::new(&shared_config.build())
        }
        ClientBuilder::RawConfig => {
            let conf = Config::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .with_env(&[
                    ("AWS_BEARER_TOKEN_BEDROCK", expected_token),
                    ("AWS_AUTH_SCHEME_PREFERENCE", "sigv4,httpBearerAuth"),
                ])
                .build();
            Client::from_conf(conf)
        }
    };

    let _ = client
        .get_async_invoke()
        .invocation_arn("arn:aws:bedrock:us-west-2:123456789012:invoke/ExampleModel")
        .send()
        .await;
    let request = captured_request.expect_request();
    let authorization_header = request.headers().get("authorization").unwrap();
    assert!(authorization_header.starts_with(&format!("Bearer {expected_token}")));
}

#[tokio::test]
async fn test_token_configured_with_auth_scheme_preference_also_set_in_env() {
    test_token_configured_with_auth_scheme_preference_also_set_in_env_impl(
        ClientBuilder::SdkConfig,
    )
    .await;
}

#[tokio::test]
async fn test_token_configured_with_auth_scheme_preference_also_set_in_env_raw_config() {
    test_token_configured_with_auth_scheme_preference_also_set_in_env_impl(
        ClientBuilder::RawConfig,
    )
    .await;
}

async fn test_explicit_service_config_takes_precedence_impl(builder_type: ClientBuilder) {
    let (http_client, captured_request) = capture_request(None);
    let expected_token = "explicit-code-token";

    let client = match builder_type {
        ClientBuilder::SdkConfig => {
            let shared_config = SdkConfig::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .service_config(
                    EnvConfigLoader::builder()
                        .env(Env::from_slice(&[(
                            "AWS_BEARER_TOKEN_BEDROCK",
                            "bedrock-token",
                        )]))
                        .build(),
                )
                .build();
            let conf = aws_sdk_bedrockruntime::config::Builder::from(&shared_config)
                .token_provider(Token::new(expected_token, None))
                .build();
            Client::from_conf(conf)
        }
        ClientBuilder::RawConfig => {
            let conf = Config::builder()
                .region(Region::new("us-west-2"))
                .http_client(http_client)
                .with_env(&[("AWS_BEARER_TOKEN_BEDROCK", "bedrock-token")])
                .token_provider(Token::new(expected_token, None))
                .build();
            Client::from_conf(conf)
        }
    };

    let _ = client
        .get_async_invoke()
        .invocation_arn("arn:aws:bedrock:us-west-2:123456789012:invoke/ExampleModel")
        .send()
        .await;
    let request = captured_request.expect_request();
    let authorization_header = request.headers().get("authorization").unwrap();
    assert!(authorization_header.starts_with(&format!("Bearer {expected_token}")));
}

#[tokio::test]
async fn test_explicit_service_config_takes_precedence() {
    test_explicit_service_config_takes_precedence_impl(ClientBuilder::SdkConfig).await;
}

#[tokio::test]
async fn test_explicit_service_config_takes_precedence_raw_config() {
    test_explicit_service_config_takes_precedence_impl(ClientBuilder::RawConfig).await;
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{collections::HashMap, ffi::OsString};

use aws_config::{provider_config::ProviderConfig, Region};
use aws_runtime::user_agent::test_util::{
    assert_ua_contains_metric_values, get_sdk_metric_str_from_request,
};
use aws_sdk_s3::{Client, Config};
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_http_client::test_util::{capture_request, ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use aws_types::os_shim_internal::{Env, Fs};
use http::header::AUTHORIZATION;

#[tokio::test]
async fn http_ua_feature() {
    let (http_client, request) = capture_request(None);

    let env = Env::from_slice(&[
        (
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
            "http://169.254.170.23/v1/credentials",
        ),
        (
            "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
            "/var/run/secrets/pods.eks.amazonaws.com/serviceaccount/eks-pod-identity-token",
        ),
    ]);
    let fs = Fs::from_raw_map(HashMap::from([(
        OsString::from(
            "/var/run/secrets/pods.eks.amazonaws.com/serviceaccount/eks-pod-identity-token",
        ),
        "Basic password".into(),
    )]));

    let cred_request = http::Request::builder()
        .header(AUTHORIZATION, "Basic password")
        .uri("http://169.254.170.23/v1/credentials")
        .body(SdkBody::empty())
        .unwrap();
    let cred_response = http::Response::builder()
        .status(200)
        .body(SdkBody::from(
            r#" {
                       "AccessKeyId" : "AKID",
                       "SecretAccessKey" : "SECRET",
                       "Token" : "TOKEN....=",
                       "AccountId" : "AID",
                       "Expiration" : "2009-02-13T23:31:30Z"
                     }"#,
        ))
        .unwrap();

    let creds_client = StaticReplayClient::new(vec![ReplayEvent::new(cred_request, cred_response)]);
    let provider_config = ProviderConfig::empty()
        .with_env(env)
        .with_fs(fs)
        .with_http_client(creds_client)
        .with_sleep_impl(TokioSleep::new());
    let provider = aws_config::ecs::Builder::default()
        .configure(&provider_config)
        .build();

    let config = Config::builder()
        .with_test_defaults()
        .region(Region::from_static("fake"))
        .http_client(http_client.clone())
        .credentials_provider(provider)
        .build();

    let client = Client::from_conf(config);

    let _ = client
        .head_bucket()
        .bucket("fake")
        .send()
        .await
        .expect("XXXXXXXXXXX");

    let request = request.expect_request();
    let ua = get_sdk_metric_str_from_request(&request);
    assert_ua_contains_metric_values(ua, &["z"]);
}

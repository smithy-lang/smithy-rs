/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::{
    imds::{
        client::test_util::{
            imds_request, imds_response, make_imds_client, token_request, token_response,
        },
        credentials::ImdsCredentialsProvider,
    },
    provider_config::ProviderConfig,
    Region,
};
use aws_runtime::user_agent::test_util::{
    assert_ua_contains_metric_values, get_sdk_metric_str_from_request,
};
use aws_sdk_s3::{Client, Config};
use aws_smithy_http_client::test_util::capture_request;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};

const TOKEN_A: &str = "AQAEAFTNrA4eEGx0AQgJ1arIq_Cc-t4tWt3fB0Hd8RKhXlKc5ccvhg==";

#[tokio::test]
async fn imds_ua_feature() {
    let (http_client, request) = capture_request(None);

    let imds_http_client = StaticReplayClient::new(vec![
            ReplayEvent::new(
                token_request("http://169.254.169.254", 21600),
                token_response(21600, TOKEN_A),
            ),
            ReplayEvent::new(
                imds_request("http://169.254.169.254/latest/meta-data/iam/security-credentials/", TOKEN_A),
                imds_response(r#"profile-name"#),
            ),
            ReplayEvent::new(
                imds_request("http://169.254.169.254/latest/meta-data/iam/security-credentials/profile-name", TOKEN_A),
                imds_response("{\n  \"Code\" : \"Success\",\n  \"LastUpdated\" : \"2021-09-20T21:42:26Z\",\n  \"Type\" : \"AWS-HMAC\",\n  \"AccessKeyId\" : \"ASIARTEST\",\n  \"SecretAccessKey\" : \"testsecret\",\n  \"Token\" : \"testtoken\",\n  \"Expiration\" : \"2021-09-21T04:16:53Z\"\n}"),
            ),
            ReplayEvent::new(
                imds_request("http://169.254.169.254/latest/meta-data/iam/security-credentials/", TOKEN_A),
                imds_response(r#"different-profile"#),
            ),
            ReplayEvent::new(
                imds_request("http://169.254.169.254/latest/meta-data/iam/security-credentials/different-profile", TOKEN_A),
                imds_response("{\n  \"Code\" : \"Success\",\n  \"LastUpdated\" : \"2021-09-20T21:42:26Z\",\n  \"Type\" : \"AWS-HMAC\",\n  \"AccessKeyId\" : \"ASIARTEST2\",\n  \"SecretAccessKey\" : \"testsecret\",\n  \"Token\" : \"testtoken\",\n  \"Expiration\" : \"2021-09-21T04:16:53Z\"\n}"),
            ),
        ]);
    let imds_client = ImdsCredentialsProvider::builder()
        .imds_client(make_imds_client(&imds_http_client))
        .configure(&ProviderConfig::no_configuration())
        .build();

    let config = Config::builder()
        .with_test_defaults()
        .region(Region::from_static("fake"))
        .http_client(http_client.clone())
        .credentials_provider(imds_client)
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
    assert_ua_contains_metric_values(ua, &["0"]);
}

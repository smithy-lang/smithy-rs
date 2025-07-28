/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::credential_process::CredentialProcessProvider;
use aws_config::Region;
use aws_runtime::user_agent::test_util::{
    assert_ua_contains_metric_values, get_sdk_metric_str_from_request,
};
use aws_sdk_s3::{Client, Config};
use aws_smithy_http_client::test_util::capture_request;

#[tokio::test]
async fn process_ua_feature() {
    let (http_client, request) = capture_request(None);

    let provider = CredentialProcessProvider::new(String::from(
        r#"echo '{ "Version": 1, "AccessKeyId": "ASIARTESTID", "SecretAccessKey": "TESTSECRETKEY", "SessionToken": "TESTSESSIONTOKEN", "AccountId": "123456789001", "Expiration": "2022-05-02T18:36:00+00:00" }'"#,
    ));
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
    assert_ua_contains_metric_values(ua, &["w"]);
}

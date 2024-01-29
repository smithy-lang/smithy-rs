/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::Config;
use aws_smithy_runtime::client::http::test_util::dvr::ReplayingClient;
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;

#[tokio::test]
async fn list_objects_v2() {
    let _logs = capture_test_logs();

    let http_client =
        ReplayingClient::from_file("tests/data/express/list-objects-v2.json").unwrap();
    let config = aws_config::from_env()
        .http_client(http_client.clone())
        .no_credentials()
        .region("us-west-2")
        .load()
        .await;
    let config = Config::from(&config)
        .to_builder()
        .with_test_defaults()
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    http_client
        .validate_body_and_headers(Some(&["x-amz-s3session-token"]), "application/xml")
        .await
        .unwrap();
}

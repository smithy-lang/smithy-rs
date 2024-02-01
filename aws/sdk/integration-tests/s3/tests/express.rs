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

#[tokio::test]
async fn list_objects_v2_in_both_express_and_regular_buckets() {
    let _logs = capture_test_logs();

    let http_client = ReplayingClient::from_file(
        "tests/data/express/list_objects_v2_in_both_express_and_regular_buckets.json",
    )
    .unwrap();
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

    // A call to an S3 Express bucket where we should see two request/response pairs,
    // one for the `create_session` API and the other for `list_objects_v2` in S3 Express bucket.
    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    // A call to a regular bucket, and request headers should not contain `x-amz-s3session-token`.
    let result = client
        .list_objects_v2()
        .bucket("regular-test-bucket")
        .send()
        .await;
    dbg!(result).expect("success");

    // A call to another S3 Express bucket where we should again see two request/response pairs,
    // one for the `create_session` API and the other for `list_objects_v2` in S3 Express bucket.
    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket-2--usw2-az3--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    // This call should be an identity cache hit for the first S3 Express bucket,
    // thus no HTTP request should be sent to the `create_session` API.
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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::types::ObjectLockRetentionMode;
use aws_smithy_runtime::client::http::test_util::dvr::ReplayingClient;

/// Tests fix for https://github.com/awslabs/aws-sdk-rust/issues/1065
///
/// The @httpPayload "Retention" member of GetObjectRetentionOutput has
/// the XML root "Retention" instead of "ObjectLockRetention".
#[tokio::test]
async fn test_get_object_retention() {
    let http_client = ReplayingClient::from_file("tests/get_object_retention.json").unwrap();
    let config = aws_sdk_s3::Config::builder()
        .with_test_defaults()
        .region(aws_sdk_s3::config::Region::from_static("us-west-2"))
        .http_client(http_client.clone())
        .build();
    let s3 = aws_sdk_s3::Client::from_conf(config);

    let result = s3
        .get_object_retention()
        .bucket("test-bucket")
        .key("mathematics-bacon.png")
        .send()
        .await
        .expect("should succeed");
    assert_eq!(
        ObjectLockRetentionMode::Governance,
        result.retention.unwrap().mode.unwrap()
    );
    http_client
        .validate_body_and_headers(None, "application/xml")
        .await
        .unwrap();
}

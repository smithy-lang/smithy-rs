/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_client::never::NeverConnector;

#[tokio::test]
#[should_panic(
    expected = r#"ConstructionFailure(MissingField { field: "upload_id", details: "cannot be empty or unset" })"#
)]
async fn test_mandatory_query_param_is_unset() {
    let conf = aws_sdk_s3::Config::builder().build();
    let conn = NeverConnector::new();

    let client = aws_sdk_s3::Client::from_conf_conn(conf, conn.clone());

    client
        .abort_multipart_upload()
        .bucket("a-bucket")
        .key("a-key")
        .send()
        .await
        .unwrap();
}

#[tokio::test]
#[should_panic(
    expected = r#"ConstructionFailure(MissingField { field: "upload_id", details: "cannot be empty or unset" })"#
)]
async fn test_mandatory_query_param_is_empty() {
    let conf = aws_sdk_s3::Config::builder().build();
    let conn = NeverConnector::new();

    let client = aws_sdk_s3::Client::from_conf_conn(conf, conn.clone());

    client
        .abort_multipart_upload()
        .bucket("a-bucket")
        .key("a-key")
        .upload_id("")
        .send()
        .await
        .unwrap();
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::SdkConfig;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use aws_smithy_client::test_connection::capture_request;

#[tokio::test]
async fn dont_dispatch_when_bucket_is_unset() {
    let (conn, rcvr) = capture_request(None);
    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    let client = Client::new(&sdk_config);
    let err = client
        .list_objects_v2()
        .send()
        .await
        .expect_err("bucket not set");
    assert_eq!(format!("{}", err), "failed to construct request");
    rcvr.expect_no_request();
}

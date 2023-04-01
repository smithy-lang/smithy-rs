/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::{Credentials, Region};

mod interceptors;

#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    // TODO(orchestrator-testing): Replace the connector with a fake request/response
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let _ = dbg!(
        client
            .get_object()
            .bucket("test-bucket")
            .key("test-file.txt")
            .send_v2()
            .await
    );
}

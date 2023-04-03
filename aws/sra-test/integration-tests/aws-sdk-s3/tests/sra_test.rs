/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::SdkBody;
use aws_smithy_client::test_connection::TestConnection;

mod interceptors;

#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=ae78f74d26b6b0c3a403d9e8cc7ec3829d6264a2b33db672bf2b151bbb901786")
            .uri("https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder().status(200).body("").unwrap(),
    )]);

    // TODO(orchestrator-testing): Replace the connector with a fake request/response
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let _ = dbg!(
        client
            .list_objects_v2()
            .bucket("test-bucket")
            .prefix("prefix~")
            .send_v2()
            .await
    );

    conn.assert_requests_match(&[]);
}

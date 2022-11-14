/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::{Duration, UNIX_EPOCH};

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::{Credentials, Region};
use aws_sdk_s3::middleware::DefaultMiddleware;
use aws_sdk_s3::operation::AbortMultipartUpload;
use aws_smithy_client::Client as CoreClient;
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;

pub type Client<C> = CoreClient<C, DefaultMiddleware>;

fn abort_multipart_upload_response_with_empty_upload_id() -> http::Request<SdkBody> {
    http::Request::builder()
        .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=82f2d8f8b0e7e05dc08a243abe6bf30ca5b4399d46f89408a8cdbcc331948dc7")
        .uri("https://s3.us-east-1.amazonaws.com/test-bucket/test.txt?x-id=AbortMultipartUpload&uploadId=")
        .body(SdkBody::empty())
        .unwrap()
}

fn empty_ok_response() -> http::Response<&'static str> {
    http::Response::builder().status(200).body("").unwrap()
}

#[tokio::test]
async fn test_mandatory_query_param_is_unset() {
    let creds = Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(Region::new("us-east-1"))
        .build();
    let conn = TestConnection::new(vec![(abort_multipart_upload_response_with_empty_upload_id(), empty_ok_response())]);
    let client = Client::new(conn.clone());
    let mut op = AbortMultipartUpload::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .unwrap();
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    client.call(op).await.expect("empty responses are OK");
    conn.assert_requests_match(&[]);
}

#[tokio::test]
async fn test_mandatory_query_param_is_set_but_empty() {
    let creds = Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(Region::new("us-east-1"))
        .build();
    let conn = TestConnection::new(vec![(abort_multipart_upload_response_with_empty_upload_id(), empty_ok_response())]);
    let client = Client::new(conn.clone());
    let mut op = AbortMultipartUpload::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .upload_id("")
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .unwrap();
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    client.call(op).await.expect("empty responses are OK");
    conn.assert_requests_match(&[]);
}

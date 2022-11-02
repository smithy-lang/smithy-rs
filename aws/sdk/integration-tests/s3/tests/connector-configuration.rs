/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::{Client, Credentials, Region};
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;
use aws_types::credentials::SharedCredentialsProvider;
use aws_types::SdkConfig;
use http::Uri;
use std::convert::Infallible;
use std::time::{Duration, UNIX_EPOCH};

static INIT_LOGGER: std::sync::Once = std::sync::Once::new();

fn init_logger() {
    INIT_LOGGER.call_once(|| {
        tracing_subscriber::fmt::init();
    });
}

#[tokio::test]
async fn test_http_connector_is_settable_in_config() {
    init_logger();

    let creds = Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conn = TestConnection::new(vec![
        (http::Request::builder()
             .header("x-amz-checksum-mode", "ENABLED")
             .header("user-agent", "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
             .header("x-amz-date", "20210618T170728Z")
             .header("x-amz-content-sha256", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
             .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
             .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-checksum-mode;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=eb9e58fa4fb04c8e6f160705017fdbb497ccff0efee4227b3a56f900006c3882")
             .uri(Uri::from_static("https://s3.us-east-1.amazonaws.com/some-test-bucket/test.txt?x-id=GetObject")).body(SdkBody::empty()).unwrap(),
         http::Response::builder()
             .header("x-amz-request-id", "4B4NGF0EAWN0GE63")
             .header("content-length", "11")
             .header("etag", "\"3e25960a79dbc69b674cd4ec67a72c62\"")
             .header("x-amz-checksum-crc32", "i9aeUg==")
             .header("content-type", "application/octet-stream")
             .header("server", "AmazonS3")
             .header("content-encoding", "")
             .header("last-modified", "Tue, 21 Jun 2022 16:29:14 GMT")
             .header("date", "Tue, 21 Jun 2022 16:29:23 GMT")
             .header("x-amz-id-2", "kPl+IVVZAwsN8ePUyQJZ40WD9dzaqtr4eNESArqE68GSKtVvuvCTDe+SxhTT+JTUqXB1HL4OxNM=")
             .header("accept-ranges", "bytes")
             .status(http::StatusCode::from_u16(200).unwrap())
             .body(r#"Hello world"#).unwrap()),
    ]);
    let conf = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .region(Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    let client = Client::new(&conf);

    let op = client
        .get_object()
        .bucket("some-test-bucket")
        .key("test.txt")
        .checksum_mode(aws_sdk_s3::model::ChecksumMode::Enabled)
        .customize()
        .await
        .unwrap();

    let res = op
        .map_operation(|mut op| {
            op.properties_mut()
                .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
            op.properties_mut().insert(AwsUserAgent::for_tests());

            Result::<_, Infallible>::Ok(op)
        })
        .unwrap()
        .send()
        .await
        .unwrap();

    conn.assert_requests_match(&[http::header::HeaderName::from_static("x-amz-checksum-mode")]);
    let body = String::from_utf8(res.body.collect().await.unwrap().to_vec()).unwrap();
    assert_eq!("Hello world", body);
}

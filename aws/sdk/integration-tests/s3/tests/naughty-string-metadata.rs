/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::{operation::PutObject, Credentials, Region};
use http::HeaderValue;
use smithy_client::test_connection::TestConnection;
use smithy_http::body::SdkBody;
use std::time::UNIX_EPOCH;
use tokio::time::Duration;

const NAUGHTY_STRINGS: &str = include_str!("../../blns/blns.txt");

// // A useful way to find leaks in the signing system that requires an actual S3 bucket to test with
// // If you want to use this, change the bucket name to your bucket
// #[tokio::test]
// async fn test_metadata_field_against_naughty_strings_list() -> Result<(), aws_sdk_s3::Error> {
//     use std::collections::HashMap;
//     use aws_sdk_s3::{SdkError, error::PutObjectError};
//
//     let config = aws_config::load_from_env().await;
//     let client = aws_sdk_s3::Client::new(&config);
//
//     let mut req = client.
//         put_object()
//         .bucket("your-test-bucket-goes-here")
//         .key("test.txt")
//         .body(aws_sdk_s3::ByteStream::from_static(b"some test text"));
//
//     for (idx, line) in NAUGHTY_STRINGS.split('\n').enumerate() {
//         // add lines to metadata unless they're a comment or empty
//         // Some naughty strings aren't valid HeaderValues so we skip those too
//         if !line.starts_with("#") && !line.is_empty() && HeaderValue::from_str(line).is_ok() {
//             let key = format!("line-{}", idx);
//
//             req = req.metadata(key, line);
//         }
//     };
//
//     // If this fails due to signing then the signer choked on a bad string. To find out which string,
//     // send one request per line instead of adding all lines as metadata for one request.
//     let _ = req.send().unwrap();
// }

// TODO figure out how to actually make this test work
#[tokio::test]
async fn test_signer_with_naughty_strings() -> Result<(), aws_sdk_s3::Error> {
    let creds = Credentials::from_keys(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(Region::new("us-east-1"))
        .build();
    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=c3f78ce4969bd55cbb90ba91f46e4fcd14d08dae858f1ac9e508712997eabde7")
            .uri("https://test-bucket.s3.us-east-1.amazonaws.com/text.txt")
            .body(SdkBody::from("some test text"))
            .unwrap(),
        http::Response::builder().status(200).body("").unwrap(),
    )]);
    let client = aws_hyper::Client::new(conn.clone());
    let mut builder = PutObject::builder()
        .bucket("test-bucket")
        .key("text.txt")
        .body(aws_sdk_s3::ByteStream::from_static(b"some test text"));

    for (idx, line) in NAUGHTY_STRINGS.split('\n').enumerate() {
        // add lines to metadata unless they're a comment or empty
        // Some naughty strings aren't valid HeaderValues so we skip those too
        if !line.starts_with("#") && !line.is_empty() && HeaderValue::from_str(line).is_ok() {
            let key = format!("line-{}", idx);

            builder = builder.metadata(key, line);
        }
    }

    let mut op = builder.build().unwrap().make_operation(&conf).unwrap();
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    client.call(op).await.expect_err("empty response");
    conn.assert_requests_match(&[]);
    Ok(())
}

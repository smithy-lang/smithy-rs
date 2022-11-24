/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::SdkConfig;
use aws_sdk_s3::types::ByteStream;
use aws_sdk_s3::{Client, Credentials, Region};
use aws_smithy_client::dvr::{Event, ReplayingConnection};
use aws_smithy_protocol_test::{assert_ok, validate_body, MediaType};
use aws_types::credentials::SharedCredentialsProvider;
use std::convert::Infallible;
use std::time::{Duration, UNIX_EPOCH};

#[tokio::test]
async fn test_operation_should_keep_dot_segments_intact_and_dedupe_leading_forward_slashes() {
    let events: Vec<Event> = serde_json::from_str(include_str!("normalize-uri-path.json")).unwrap();
    let replayer = ReplayingConnection::new(events);
    let sdk_config = SdkConfig::builder()
        .region(Region::from_static("us-east-1"))
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            "test", "test", None, None, "test",
        )))
        .http_connector(replayer.clone())
        .build();

    let client = Client::new(&sdk_config);

    let bucket_name = "test-bucket-ad7c9f01-7f7b-4669-b550-75cc6d4df0f1";

    // Why would anyone prepend a "/" to a bucket name?
    // Well, [`aws_sdk_s3::output::CreateBucketOutput::location`] returns the name of a bucket but with a forward slash prepended to it.
    // The user can then inadvertently pass it to [`aws_sdk_s3::client::fluent_builders::PutObject::bucket`].
    // In that case, the URI path portion of a `Request<SdkBody>` given the code below will look like
    // "/%2Ftest-bucket-ad7c9f01-7f7b-4669-b550-75cc6d4df0f1/a/.././b.txt?x-id=PutObject\x01"
    let _output = client
        .put_object()
        .bucket(format!("/{bucket_name}"))
        .key("a/.././b.txt")
        .body(ByteStream::from_static("Hello, world".as_bytes()))
        .customize()
        .await
        .unwrap()
        .map_operation(|mut op| {
            op.properties_mut()
                .insert(UNIX_EPOCH + Duration::from_secs(1669257290));
            Result::Ok::<_, Infallible>(op)
        })
        .unwrap()
        .send()
        .await
        .unwrap();

    replayer
        .validate(&["authorization"], body_comparer)
        .await
        .unwrap();
}

fn body_comparer(expected: &[u8], actual: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let expected = std::str::from_utf8(expected).unwrap();
    let actual = std::str::from_utf8(actual).unwrap();
    assert_ok(validate_body(
        actual,
        expected,
        MediaType::Other("octet-stream".to_owned()),
    ));
    Ok(())
}

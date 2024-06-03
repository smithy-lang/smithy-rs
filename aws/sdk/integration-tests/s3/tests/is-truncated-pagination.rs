/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(feature = "test-util")]

use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::Config;
use aws_sdk_s3::{config::Credentials, config::Region, Client};
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;

fn mk_response(part_marker: u8) -> http::Response<SdkBody> {
    let (part_num_marker, next_num_marker, is_truncated) = if part_marker < 3 {
        (part_marker, part_marker + 1, true)
    } else {
        (part_marker, 0, false)
    };
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n
    <ListPartsResult
        xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">
        <Bucket>foo-bar-bucket</Bucket>
        <Key>test.txt</Key>
        <UploadId>N0taR34lUpl0adId</UploadId>
        <PartNumberMarker>{part_num_marker}</PartNumberMarker>
        <NextPartNumberMarker>{next_num_marker}</NextPartNumberMarker>
        <MaxParts>1</MaxParts>
        <IsTruncated>{is_truncated}</IsTruncated>
        <Part>
            <PartNumber>4</PartNumber>
            <LastModified>2024-06-03T16:01:05.000Z</LastModified>
            <ETag>&quot;1234&quot;</ETag>
            <Size>5242880</Size>
        </Part>
    </ListPartsResult>"
    );
    http::Response::builder().body(SdkBody::from(body)).unwrap()
}

fn mk_request() -> http::Request<SdkBody> {
    http::Request::builder()
        .uri("https://some-test-bucket.s3.us-east-1.amazonaws.com/test.txt?part-number-marker=PartNumberMarker&uploadId=UploadId")
        .body(SdkBody::empty())
        .unwrap()
}

#[tokio::test]
async fn is_truncated_pagination_does_not_loop() {
    let http_client = StaticReplayClient::new(vec![
        ReplayEvent::new(mk_request(), mk_response(0)),
        ReplayEvent::new(mk_request(), mk_response(1)),
        ReplayEvent::new(mk_request(), mk_response(2)),
        ReplayEvent::new(mk_request(), mk_response(3)),
        //The events below should never be called because the pagination should
        //terminate with the event above
        ReplayEvent::new(mk_request(), mk_response(0)),
        ReplayEvent::new(mk_request(), mk_response(1)),
    ]);

    let config = Config::builder()
        .credentials_provider(SharedCredentialsProvider::new(
            Credentials::for_tests_with_session_token(),
        ))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .with_test_defaults()
        .build();
    let client = Client::from_conf(config);

    let list_parts_res = client
        .list_parts()
        .bucket("some-test-bucket")
        .key("test.txt")
        .upload_id("N0taR34lUpl0adId")
        .max_parts(1)
        .into_paginator()
        .send()
        .collect::<Vec<_>>()
        .await;

    // Confirm that the pagination stopped calling the http client after the
    // first page with is_truncated = false
    assert_eq!(list_parts_res.len(), 4)
}

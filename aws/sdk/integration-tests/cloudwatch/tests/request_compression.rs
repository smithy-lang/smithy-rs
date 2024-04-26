/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(test)]

use aws_sdk_cloudwatch::config::{
    BehaviorVersion, Config, Credentials, Region, SharedCredentialsProvider,
};
use aws_sdk_cloudwatch::types::MetricDatum;
use aws_sdk_cloudwatch::Client;
use aws_smithy_runtime::client::http::test_util::capture_request;
use flate2::read::GzDecoder;
use http_body::Body;
use std::io::Read;

#[tokio::test]
async fn test_request_compression() {
    let (http_client, captured_request) = capture_request(None);
    let config = Config::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("not-a-real-region"))
        .http_client(http_client)
        .behavior_version(BehaviorVersion::latest())
        .with_test_defaults()
        .build();
    let client = Client::from_conf(config);
    let _res = client
        .put_metric_data()
        .metric_data(MetricDatum::builder().build())
        .send()
        .await;

    let request = captured_request.expect_request();
    // Check that the content-encoding header is set to "gzip"
    assert_eq!("gzip", request.headers().get("content-encoding").unwrap());

    let compressed_data = {
        let mut body = request.into_body();
        let mut data = Vec::new();
        while let Some(b) = body.data().await {
            data.extend(b.expect("failed to read body"));
        }
        data
    };
    let unzipped_data = {
        let mut decoder = GzDecoder::new(&compressed_data[..]);
        let mut v = Vec::new();
        decoder.read_to_end(&mut v).unwrap();
        v
    };
    let s = String::from_utf8_lossy(&unzipped_data);
    // Check unzipped data is what we expect (it's in AWS Query format)
    assert_eq!("\u{1f}�\u{8}\0\0\0\0\0\0�\u{1}'\0��Action=PutMetricData&Version=2010-08-01f\u{c}%\u{1f}'\0\0\0", s);
}

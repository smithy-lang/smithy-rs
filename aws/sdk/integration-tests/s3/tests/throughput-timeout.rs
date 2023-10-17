/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_sts::error::DisplayErrorContext;
use aws_smithy_async::rt::sleep::{default_async_sleep, AsyncSleep};
use aws_smithy_async::test_util::instant_time_and_sleep;
use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_runtime::client::http::body::minimum_throughput::MinimumThroughputBody;
use aws_smithy_runtime::client::http::test_util::wire::{ReplayedEvent, WireMockServer};
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use aws_smithy_runtime::{ev, match_events};
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::region::Region;
use aws_types::SdkConfig;
use bytes::Bytes;
use once_cell::sync::Lazy;
use std::convert::Infallible;
use std::time::{Duration, UNIX_EPOCH};

#[should_panic = "minimum throughput was specified at 2 B/s, but throughput of 1.5 B/s was observed"]
#[tokio::test]
async fn test_throughput_timeout_less_than() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);

    let shared_sleep = sleep.clone();
    // Will send ~1 byte per second.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = shared_sleep.clone();
        async move {
            if state > 255 {
                None
            } else {
                sleep.sleep(Duration::from_secs(1)).await;
                Some((
                    Result::<_, Infallible>::Ok(Bytes::from(vec![state as u8])),
                    state + 1,
                ))
            }
        }
    });
    let body = ByteStream::from(hyper::body::Body::wrap_stream(stream));
    let body = body.map(move |body| {
        let ts = time_source.clone();
        // Throw an error if the stream sends less than 2 bytes per second
        let minimum_throughput = (2u64, Duration::from_secs(1));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(ts, body, minimum_throughput),
        ))
    });
    let res = body.collect().await;

    match res {
        Ok(_) => panic!("Expected an error due to slow stream but no error occurred."),
        Err(e) => panic!("{}", DisplayErrorContext(e)),
    }
}

const EXPECTED_BYTES: Lazy<Vec<u8>> = Lazy::new(|| (1..=255).map(|i| i as u8).collect::<Vec<_>>());

#[tokio::test]
async fn test_throughput_timeout_equal_to() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);
    let time_clone = time_source.clone();
    let sleep_clone = sleep.clone();

    // Will send ~1 byte per second.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = sleep_clone.clone();
        async move {
            if state > 255 {
                None
            } else {
                sleep.sleep(Duration::from_secs(1)).await;
                Some((
                    Result::<_, Infallible>::Ok(Bytes::from(vec![state as u8])),
                    state + 1,
                ))
            }
        }
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(move |body| {
        let time_source = time_clone.clone();
        // Throw an error if the stream sends less than 1 byte per second
        let minimum_throughput = (1u64, Duration::from_secs(1));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(time_source, body, minimum_throughput),
        ))
    });

    let res = body
        .collect()
        .await
        .expect("no streaming error occurs because data is sent fast enough")
        .to_vec();
    assert_eq!(255.0, time_source.seconds_since_unix_epoch());
    assert_eq!(Duration::from_secs(255), sleep.total_duration());
    assert_eq!(*EXPECTED_BYTES, res);
}

#[tokio::test]
async fn test_throughput_timeout_greater_than() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);
    let time_clone = time_source.clone();
    let sleep_clone = sleep.clone();

    // Will send ~1 byte per second.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = sleep_clone.clone();
        async move {
            if state > 255 {
                None
            } else {
                sleep.sleep(Duration::from_secs(1)).await;
                Some((
                    Result::<_, Infallible>::Ok(Bytes::from(vec![state as u8])),
                    state + 1,
                ))
            }
        }
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(move |body| {
        let time_source = time_clone.clone();
        // Throw an error if the stream sends less than 1 byte per 2s
        let minimum_throughput = (1u64, Duration::from_secs(2));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(time_source, body, minimum_throughput),
        ))
    });
    let res = body
        .collect()
        .await
        .expect("no streaming error occurs because data is sent fast enough")
        .to_vec();
    assert_eq!(255.0, time_source.seconds_since_unix_epoch());
    assert_eq!(Duration::from_secs(255), sleep.total_duration());
    assert_eq!(*EXPECTED_BYTES, res);
}

// One mebibyte of zeroes.
const LARGE_BODY: &[u8] = &[0u8; 1024 * 1024];

#[tokio::test]
async fn test_throughput_timeout_for_download() {
    let mock = WireMockServer::start(vec![ReplayedEvent::with_body(LARGE_BODY)]).await;

    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .endpoint_url(mock.endpoint_url())
        .http_client(mock.http_client())
        .region(Region::new("us-east-1"))
        .time_source(SystemTimeSource::new())
        .sleep_impl(default_async_sleep().expect("default async sleep exists"))
        .retry_config(RetryConfig::standard())
        .timeout_config(
            TimeoutConfig::builder()
                // The request requires its own timeout, separate from the body timeout.
                .operation_timeout(Duration::from_secs(3))
                .build(),
        )
        .build();

    let time_source = sdk_config
        .time_source()
        .expect("default time source is set");

    let client = aws_sdk_s3::Client::new(&sdk_config);
    let res = client
        .get_object()
        .bucket("bucket")
        .key("key")
        .send()
        .await
        .unwrap();

    let body_fut = res
        .body
        .map(move |body| {
            let time_source = time_source.clone();
            // Throw an error if the stream fails to send the entirety of the data in 1s.
            let minimum_throughput = (1024 * 1024, Duration::from_secs(1));
            SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
                MinimumThroughputBody::new(time_source, body, minimum_throughput),
            ))
        })
        .collect();
    // In the event that the minimum throughput body timeout doesn't work,
    // we need an outer timeout to ensure the test won't run forever.
    let body = tokio::time::timeout(Duration::from_secs(2), body_fut)
        .await
        .unwrap()
        .unwrap()
        .to_vec();

    assert_eq!(body, LARGE_BODY);

    match_events!(ev!(dns), ev!(connect), ev!(http(200)))(&mock.events());
}

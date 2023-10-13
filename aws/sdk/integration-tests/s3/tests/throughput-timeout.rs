/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_sts::error::DisplayErrorContext;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::test_util::instant_time_and_sleep;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_runtime::client::http::body::minimum_throughput::MinimumThroughputBody;
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use aws_smithy_runtime_api::shared::IntoShared;
use aws_types::sdk_config::SharedAsyncSleep;
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
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
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
    let time_source: SharedTimeSource = time_source.into_shared();
    let sleep: SharedAsyncSleep = sleep.into_shared();

    // Will send ~1 byte per second.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = sleep.clone();
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
        let time_source = time_source.clone();
        // Throw an error if the stream sends less than 1 byte per second
        let minimum_throughput = (1u64, Duration::from_secs(1));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(time_source, body, minimum_throughput),
        ))
    });
    // assert_eq!(255.0, time_source.seconds_since_unix_epoch());
    // assert_eq!(Duration::from_secs(255), sleep.total_duration());
    let res = body
        .collect()
        .await
        .expect("no streaming error occurs because data is sent fast enough")
        .to_vec();
    assert_eq!(*EXPECTED_BYTES, res);
}

#[tokio::test]
async fn test_throughput_timeout_greater_than() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);
    let time_source: SharedTimeSource = time_source.into_shared();
    let sleep: SharedAsyncSleep = sleep.into_shared();

    // Will send ~1 byte per second.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = sleep.clone();
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
        let time_source = time_source.clone();
        // Throw an error if the stream sends less than 1 byte per 2s
        let minimum_throughput = (1u64, Duration::from_secs(2));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(time_source, body, minimum_throughput),
        ))
    });
    // assert_eq!(255.0, time_source.seconds_since_unix_epoch());
    // assert_eq!(Duration::from_secs(255), sleep.total_duration());
    let res = body
        .collect()
        .await
        .expect("no streaming error occurs because data is sent fast enough")
        .to_vec();
    assert_eq!(*EXPECTED_BYTES, res);
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_sts::error::DisplayErrorContext;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::test_util::instant_time_and_sleep;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_runtime::client::http::body::minimum_throughput::MinimumThroughputBody;
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use std::convert::Infallible;
use std::time::{Duration, UNIX_EPOCH};

#[should_panic = "minimum throughput was specified at 2 B/s, but throughput of 1.5 B/s was observed"]
#[tokio::test]
async fn test_throughput_timeout_happens_for_slow_stream() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);

    let shared_sleep = sleep.clone();
    // Will send ~1 byte per second because ASCII digits have a size
    // of 1 byte and we sleep for 1 second after every digit we send.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = shared_sleep.clone();
        async move {
            if state > 100 {
                None
            } else {
                sleep.sleep(Duration::from_secs(1)).await;
                Some((
                    Result::<String, Infallible>::Ok(state.to_string()),
                    state + 1,
                ))
            }
        }
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(move |body| {
        let ts = time_source.clone();
        // Throw an error if the stream sends less than 2 bytes per second at any point
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

#[tokio::test]
async fn test_throughput_timeout_doesnt_happen_for_fast_stream() {
    let _logs = capture_test_logs();
    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH);

    let shared_sleep = sleep.clone();
    // Will send ~1 byte per second because ASCII digits have a size
    // of 1 byte and we sleep for 1 second after every digit we send.
    let stream = futures_util::stream::unfold(1, move |state| {
        let sleep = shared_sleep.clone();
        async move {
            if state > 100 {
                None
            } else {
                sleep.sleep(Duration::from_secs(1)).await;
                Some((
                    Result::<String, Infallible>::Ok(state.to_string()),
                    state + 1,
                ))
            }
        }
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(move |body| {
        let ts = time_source.clone();
        // Throw an error if the stream sends less than 1 bytes per 2s at any point
        let minimum_throughput = (1u64, Duration::from_secs(2));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(ts, body, minimum_throughput),
        ))
    });
    assert_eq!(Duration::from_secs(100), sleep.total_duration());
    let _res = body
        .collect()
        .await
        .expect("no streaming error occurs because data is sent fast enough");
}

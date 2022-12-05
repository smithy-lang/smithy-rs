/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::model::{Delete, ObjectIdentifier};
use aws_sdk_s3::types::ByteStream;
use aws_sdk_s3::Client;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::endpoint::Endpoint;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::credentials::SharedCredentialsProvider;
use aws_types::region::Region;
use aws_types::{Credentials, SdkConfig};
use bytes::BytesMut;
use futures::future;
use hdrhistogram::sync::SyncHistogram;
use hdrhistogram::Histogram;
use std::fs::File;
use std::future::Future;
use std::iter::repeat_with;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{Duration, Instant};
use tracing::{debug, debug_span, info, Dispatch, Instrument};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter, EnvFilter, Layer};

#[tokio::test(flavor = "current_thread")]
async fn test_concurrency_on_multi_thread() {
    test_concurrency_with_dummy_server().await
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrency_on_single_thread() {
    test_concurrency_with_dummy_server().await
}

async fn test_concurrency_with_dummy_server() {
    const TASK_COUNT: usize = 10_000;
    // At 130 and above, this test will fail with a `ConnectorError` from `hyper`. I've seen:
    // - ConnectorError { kind: Io, source: hyper::Error(Canceled, hyper::Error(Io, Os { code: 54, kind: ConnectionReset, message: "Connection reset by peer" })) }
    // - ConnectorError { kind: Io, source: hyper::Error(BodyWrite, Os { code: 32, kind: BrokenPipe, message: "Broken pipe" }) }
    // These errors don't necessarily occur when actually running against S3 with concurrency levels
    // above 129. You can test it for yourself by running the
    // `test_concurrency_put_object_against_live` test that appears at the bottom of this file.
    const CONCURRENCY_LIMIT: usize = 129;

    let (server, server_addr) = start_agreeable_server().await;
    let _ = tokio::spawn(server);

    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            "ANOTREAL",
            "notrealrnrELgWzOk3IfjzDKtFBhDby",
            Some("notarealsessiontoken".to_string()),
            None,
            "test",
        )))
        .region(Region::new("us-east-1"))
        .endpoint_resolver(
            Endpoint::immutable(format!("http://{server_addr}")).expect("valid endpoint"),
        )
        .build();

    let client = Client::new(&sdk_config);

    // a tokio semaphore can be used to ensure we only run up to <CONCURRENCY_LIMIT> requests
    // at once.
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));
    let futures: Vec<_> = (0..TASK_COUNT)
        // create a PutObject request for each payload
        .map(|i| {
            let client = client.clone();
            let key = format!("concurrency/test_object_{:05}", i);
            let fut = client
                .put_object()
                .bucket("doesnt-matter")
                .key(key)
                .body(ByteStream::new(SdkBody::empty()))
                .send();

            // make a clone of the semaphore that can live in the future
            let semaphore = semaphore.clone();
            // because we wait on a permit from the semaphore, only <CONCURRENCY_LIMIT> futures
            // will be run at once. Otherwise, we'd quickly get rate-limited by S3.
            async move {
                let permit = semaphore
                    .acquire()
                    .await
                    .expect("we'll get one if we wait long enough");
                let res = fut.await.expect("server always returns '200 OK'");
                drop(permit);

                res
            }
        })
        .collect();

    let res: Vec<_> = future::join_all(futures).await;
    // assert all tasks were run
    assert_eq!(TASK_COUNT, res.len());
}

// This server is agreeable because it always replies with `OK`
async fn start_agreeable_server() -> (impl Future<Output = ()>, SocketAddr) {
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::sleep;

    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("socket is free");
    let bind_addr = listener.local_addr().unwrap();

    async fn process_socket(socket: TcpStream) {
        let mut buf = BytesMut::new();
        let response: &[u8] = b"HTTP/1.1 200 OK\r\n\r\n";
        let mut time_to_respond = false;
        let mut bytes_left_to_write = response.len();

        loop {
            match socket.try_read_buf(&mut buf) {
                // Ok(0) => {
                //     unreachable!(
                //         "The connection will be closed before this branch is ever reached"
                //     );
                // }
                Ok(_) => {
                    // Check for CRLF to see if we've received the entire HTTP request.
                    if buf.ends_with(b"\r\n\r\n") {
                        time_to_respond = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    debug!("reading would block, sleeping for 1ms and then trying again");
                    sleep(Duration::from_millis(1)).await;
                }
                Err(err) => {
                    panic!("{}", err)
                }
            }

            if socket.writable().await.is_ok() {
                if time_to_respond {
                    let bytes_written = socket.try_write(&response).unwrap();
                    bytes_left_to_write = bytes_left_to_write - bytes_written;
                    if bytes_left_to_write == 0 {
                        break;
                    }
                }
            }
        }
    }

    let fut = async move {
        loop {
            let (socket, addr) = listener
                .accept()
                .await
                .expect("listener can accept new connections");
            debug!("server received new connection from {addr:?}");
            let start = std::time::Instant::now();
            process_socket(socket).await;
            debug!(
                "connection to {addr:?} closed after {:.02?}",
                start.elapsed()
            );
        }
    };

    (fut, bind_addr)
}

#[ignore = "this test runs against S3 and requires credentials"]
#[tokio::test(flavor = "current_thread")]
async fn test_concurrency_put_object_against_live() {
    const TASK_COUNT: usize = 10_000;
    const TASK_PAYLOAD_LENGTH: usize = 100_000;
    const CONCURRENCY_LIMIT: usize = 100;
    const BUCKET: &str = "your-test-bucket";
    const REQUEST_SPAN_NAME: &str = "PutObjectRequest";
    const CONCURRENCY_LOG_PATH: &str = "./concurrency_log.json";

    let log_file = File::create(CONCURRENCY_LOG_PATH).unwrap();
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    let histogram_log = tracing_subscriber::fmt::layer()
        .json()
        .with_span_events(FmtSpan::CLOSE)
        .with_target(false)
        .with_level(false)
        .with_writer(non_blocking)
        .with_filter(filter::filter_fn(|metadata| {
            metadata.name() == REQUEST_SPAN_NAME
        }));

    let tracing_registry = Dispatch::new(
        tracing_subscriber::registry()
            .with(histogram_log)
            .with(tracing_subscriber::fmt::layer())
            .with(EnvFilter::from_default_env()),
    );

    tracing_registry.clone().init();

    let sdk_config = aws_config::from_env()
        .timeout_config(
            TimeoutConfig::builder()
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(30))
                .build(),
        )
        .load()
        .await;
    let client = Client::new(&sdk_config);

    let size_per_chunk = byte_size(TASK_PAYLOAD_LENGTH);
    let total_size = byte_size(TASK_PAYLOAD_LENGTH * TASK_COUNT);
    // Larger requests take longer to send, which means we'll consume more network resources per
    // request, which means we can't have as many concurrent connections to S3. This Ratio makes it
    // easier to estimate the number of concurrent requests to send.
    let concurrency_payload_size_ratio =
        (CONCURRENCY_LIMIT as f64 / TASK_PAYLOAD_LENGTH as f64) * 1000.0;
    info!(
        size_per_chunk,
        total_size,
        concurrency_payload_size_ratio,
        "preparing to send {} chunks of data to S3...",
        TASK_COUNT
    );

    let semaphore_histogram =
        Histogram::new_with_bounds(1, Duration::from_secs(60 * 60).as_nanos() as u64, 3)
            .unwrap()
            .into_sync();

    let req_histogram =
        Histogram::new_with_bounds(1, Duration::from_secs(60 * 60).as_nanos() as u64, 3)
            .unwrap()
            .into_sync();

    // a tokio semaphore can be used to ensure we only run up to <CONCURRENCY_LIMIT> requests
    // at once.
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));

    let mut object_keys = Vec::with_capacity(TASK_COUNT);
    let task_creation_start = Instant::now();
    // create all the PutObject requests
    let futures: Vec<_> = (0..TASK_COUNT)
        // create <TASK_COUNT> random alphanumeric strings of <TASK_PAYLOAD_LENGTH> length
        .map(|_| {
            repeat_with(fastrand::alphanumeric)
                .take(TASK_PAYLOAD_LENGTH)
                .collect()
        })
        // enumerate the keys so we can tell the objects apart
        .enumerate()
        // create a PutObject request for each payload
        .map(|(i, body): (usize, String)| {
            let client = client.clone();
            let key = format!("concurrency/test_object_{:05}", i);
            object_keys.push(key.clone());
            let body = ByteStream::from(SdkBody::from(body.as_str()));
            let fut = client
                .put_object()
                .bucket(BUCKET)
                .key(key.clone())
                .body(body)
                .send();
            // make a clone of the semaphore that can live in the future
            let semaphore = semaphore.clone();
            let mut semaphore_histogram_recorder = semaphore_histogram.recorder();
            let mut req_histogram_recorder = req_histogram.recorder();

            // because we wait on a permit from the semaphore, only <CONCURRENCY_LIMIT> futures
            // will be run at once. Otherwise, we'd quickly get rate-limited by S3.
            async move {
                // Track time to acquire permit
                let start = Instant::now();
                let permit = semaphore
                    .acquire()
                    .await
                    .expect("we'll get one if we wait long enough");
                semaphore_histogram_recorder.saturating_record(start.elapsed().as_nanos() as u64);
                debug!("semaphore_duration = {:?}", start.elapsed(),);
                // Track time to send request and receive the response
                let start = Instant::now();
                let res = fut.await.expect("request should succeed");
                debug!("put_object_duration = {:?}", start.elapsed(),);

                req_histogram_recorder.saturating_record(start.elapsed().as_nanos() as u64);
                drop(permit);
                res
            }
            .instrument(debug_span!(REQUEST_SPAN_NAME, key))
        })
        .collect();

    let task_creation_elapsed = task_creation_start.elapsed();

    debug!(
        "created {} PutObject futures in {task_creation_elapsed:?}",
        TASK_COUNT
    );
    debug!("running futures concurrently with future::join_all");
    let start = Instant::now();
    let res: Vec<_> = future::join_all(futures).await;
    debug!("future_joined_successfully, asserting that all tasks were run...");
    assert_eq!(TASK_COUNT, res.len());
    info!(
        "all {TASK_COUNT} tasks completed after {:?}",
        start.elapsed()
    );

    debug!("success! now, deleting test objects from S3...");
    // Only 1,000 things can be deleted at once so the keys need to be divided into chunks
    let chunked_keys: Vec<_> = object_keys.chunks(1000).collect();
    for chunk in chunked_keys {
        let mut delete_builder = Delete::builder();
        for key in chunk {
            delete_builder = delete_builder.objects(ObjectIdentifier::builder().key(key).build());
        }

        client
            .delete_objects()
            .bucket(BUCKET)
            .delete(delete_builder.build())
            .send()
            .await
            .unwrap();
    }

    info!("test data has been deleted from S3");

    display_histogram(
        "Semaphore Latency",
        semaphore_histogram,
        "s",
        Duration::from_secs(1).as_nanos() as f64,
    );
    display_histogram(
        "Request Latency",
        req_histogram,
        "s",
        Duration::from_secs(1).as_nanos() as f64,
    );
}

fn display_histogram(name: &str, mut h: SyncHistogram<u64>, unit: &str, scale: f64) {
    // Refreshing is required or else we won't see any results at all
    h.refresh();
    debug!("displaying {} results from {name} histogram", h.len());

    info!(
        "{name}\n\
        \tmean:\t{:.1}{unit},\n\
        \tp50:\t{:.1}{unit},\n\
        \tp90:\t{:.1}{unit},\n\
        \tp99:\t{:.1}{unit},\n\
        \tmax:\t{:.1}{unit}",
        h.mean() / scale,
        h.value_at_quantile(0.5) as f64 / scale,
        h.value_at_quantile(0.9) as f64 / scale,
        h.value_at_quantile(0.99) as f64 / scale,
        h.max() as f64 / scale,
    );
}

fn byte_size(bytes: usize) -> String {
    let base = (bytes as f64).log(10.0) / (1000_f64).log(10.0);
    let suffix_index = base.floor() as usize;
    let suffix = ["", "kb", "MB", "GB"][suffix_index];
    let n = 1000_f64.powf(base - base.floor());

    format!("{n:.2}{suffix}")
}

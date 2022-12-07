/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::iter::repeat_with;
use std::net::SocketAddr;
use std::sync::Arc;

use aws_sdk_s3::Client;
use aws_smithy_http::endpoint::Endpoint;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_types::credentials::SharedCredentialsProvider;
use aws_types::region::Region;
use aws_types::{Credentials, SdkConfig};
use bytes::BytesMut;
use futures_util::future;
use hdrhistogram::sync::SyncHistogram;
use hdrhistogram::Histogram;
use tokio::sync::Semaphore;
use tokio::time::{Duration, Instant};

const TASK_COUNT: usize = 10_000;
// Larger requests take longer to send, which means we'll consume more network resources per
// request, which means we can't support as many concurrent connections to S3.
const TASK_PAYLOAD_LENGTH: usize = 100_000;
// At 130 and above, this test will fail with a `ConnectorError` from `hyper`. I've seen:
// - ConnectorError { kind: Io, source: hyper::Error(Canceled, hyper::Error(Io, Os { code: 54, kind: ConnectionReset, message: "Connection reset by peer" })) }
// - ConnectorError { kind: Io, source: hyper::Error(BodyWrite, Os { code: 32, kind: BrokenPipe, message: "Broken pipe" }) }
// These errors don't necessarily occur when actually running against S3 with concurrency levels
// above 129. You can test it for yourself by running the
// `test_concurrency_put_object_against_live` test that appears at the bottom of this file.
const CONCURRENCY_LIMIT: usize = 129;

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_on_multi_thread_against_dummy_server() {
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

    test_concurrency(sdk_config).await
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrency_on_single_thread_against_dummy_server() {
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

    test_concurrency(sdk_config).await
}

#[ignore = "this test runs against S3 and requires credentials"]
#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_on_multi_thread_against_s3() {
    let sdk_config = aws_config::from_env()
        .timeout_config(
            TimeoutConfig::builder()
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(30))
                .build(),
        )
        .load()
        .await;

    test_concurrency(sdk_config).await
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
                Ok(_) => {
                    // Check for CRLF to see if we've received the entire HTTP request.
                    if buf.ends_with(b"\r\n\r\n") {
                        time_to_respond = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // reading would block, sleeping for 1ms and then trying again
                    sleep(Duration::from_millis(1)).await;
                }
                Err(err) => {
                    panic!("{}", err)
                }
            }

            if socket.writable().await.is_ok() && time_to_respond {
                let bytes_written = socket.try_write(response).unwrap();
                bytes_left_to_write -= bytes_written;
                if bytes_left_to_write == 0 {
                    break;
                }
            }
        }
    }

    let fut = async move {
        loop {
            let (socket, _addr) = listener
                .accept()
                .await
                .expect("listener can accept new connections");
            process_socket(socket).await;
        }
    };

    (fut, bind_addr)
}

async fn test_concurrency(sdk_config: SdkConfig) {
    let client = Client::new(&sdk_config);

    let histogram =
        Histogram::new_with_bounds(1, Duration::from_secs(60 * 60).as_nanos() as u64, 3)
            .unwrap()
            .into_sync();

    // This semaphore ensures we only run up to <CONCURRENCY_LIMIT> requests at once.
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));
    let futures = (0..TASK_COUNT).map(|i| {
        let client = client.clone();
        let key = format!("concurrency/test_object_{:05}", i);
        let body: Vec<_> = repeat_with(fastrand::alphanumeric)
            .take(TASK_PAYLOAD_LENGTH)
            .map(|c| c as u8)
            .collect();
        let fut = client
            .put_object()
            .bucket("your-test-bucket-here")
            .key(key)
            .body(body.into())
            .send();
        // make a clone of the semaphore and the recorder that can live in the future
        let semaphore = semaphore.clone();
        let mut histogram_recorder = histogram.recorder();

        // because we wait on a permit from the semaphore, only <CONCURRENCY_LIMIT> futures
        // will be run at once. Otherwise, we'd quickly get rate-limited by S3.
        async move {
            let permit = semaphore
                .acquire()
                .await
                .expect("we'll get one if we wait long enough");
            let start = Instant::now();
            let res = fut.await.expect("request should succeed");
            histogram_recorder.saturating_record(start.elapsed().as_nanos() as u64);
            drop(permit);
            res
        }
    });

    let res: Vec<_> = future::join_all(futures).await;
    // Assert we ran all the tasks
    assert_eq!(TASK_COUNT, res.len());

    display_metrics(
        "Request Latency",
        histogram,
        "s",
        Duration::from_secs(1).as_nanos() as f64,
    );
}

fn display_metrics(name: &str, mut h: SyncHistogram<u64>, unit: &str, scale: f64) {
    // Refreshing is required or else we won't see any results at all
    h.refresh();
    println!("displaying {} results from {name} histogram", h.len());
    println!(
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

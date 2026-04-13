/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tests that `max_connections` limits the number of concurrent in-flight requests.
//!
//! The limited tests use `start_paused = true` so sleeps advance instantly,
//! making them deterministic. The unbounded control test uses `multi_thread`
//! with real time to ensure genuine concurrency.

use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_smithy_http_client::{tls, Builder};
use aws_types::region::Region;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::Duration;

const MAX_CONNS: usize = 3;
const TASK_COUNT: usize = 10;

const S3_RESPONSE_BODY: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
    <Buckets></Buckets></ListAllMyBucketsResult>";

fn s3_response() -> String {
    format!(
        "HTTP/1.1 200 OK\r\nconnection: keep-alive\r\n\
         content-type: application/xml\r\ncontent-length: {}\r\n\r\n{}",
        S3_RESPONSE_BODY.len(),
        std::str::from_utf8(S3_RESPONSE_BODY).unwrap()
    )
}

async fn start_server(
    hold_duration: Duration,
) -> (impl Future<Output = ()>, SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peak_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));
    let peak = peak_concurrent.clone();
    let current = current_concurrent.clone();
    let response = s3_response();

    let fut = async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let response = response.clone();
            let current = current.clone();
            let peak = peak.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    let mut total = 0;
                    loop {
                        match stream.read(&mut buf[total..]).await {
                            Ok(0) => return,
                            Ok(n) => {
                                total += n;
                                if std::str::from_utf8(&buf[..total])
                                    .unwrap_or("")
                                    .contains("\r\n\r\n")
                                {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }

                    let prev = current.fetch_add(1, Ordering::SeqCst);
                    peak.fetch_max(prev + 1, Ordering::SeqCst);

                    tokio::time::sleep(hold_duration).await;

                    current.fetch_sub(1, Ordering::SeqCst);

                    if stream.write_all(response.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    };

    (fut, addr, peak_concurrent)
}

fn s3_client(
    addr: SocketAddr,
    http_client: aws_smithy_runtime_api::client::http::SharedHttpClient,
) -> Client {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .region(Region::from_static("us-east-1"))
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .endpoint_url(format!("http://{addr}"))
        .http_client(http_client)
        .build();
    Client::from_conf(config)
}

async fn fire_concurrent_requests(client: &Client, count: usize) {
    let futs: Vec<_> = (0..count)
        .map(|i| {
            let client = client.clone();
            async move {
                client
                    .list_buckets()
                    .send()
                    .await
                    .unwrap_or_else(|e| panic!("request {i} failed: {e:?}"));
            }
        })
        .collect();
    futures_util::future::join_all(futs).await;
}

/// With `max_connections(3)` and 10 concurrent tasks, peak concurrency must not exceed 3.
#[tokio::test(start_paused = true)]
async fn max_connections_limits_concurrency() {
    let (server, addr, peak) = start_server(Duration::from_millis(100)).await;
    let _server = tokio::spawn(server);

    let http_client = Builder::new()
        .max_connections(MAX_CONNS)
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https();

    let client = s3_client(addr, http_client);
    fire_concurrent_requests(&client, TASK_COUNT).await;

    let observed = peak.load(Ordering::SeqCst);
    assert!(
        observed <= MAX_CONNS,
        "peak concurrent requests ({observed}) exceeded max_connections ({MAX_CONNS})"
    );
    assert!(
        observed > 1,
        "expected some concurrency but peak was {observed}"
    );
}

/// With `max_connections(1)`, requests must be fully serialized.
#[tokio::test(start_paused = true)]
async fn max_connections_one_serializes_requests() {
    let (server, addr, peak) = start_server(Duration::from_millis(50)).await;
    let _server = tokio::spawn(server);

    let http_client = Builder::new()
        .max_connections(1)
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https();

    let client = s3_client(addr, http_client);
    fire_concurrent_requests(&client, 5).await;

    let observed = peak.load(Ordering::SeqCst);
    assert_eq!(
        observed, 1,
        "with max_connections(1), peak must be exactly 1, got {observed}"
    );
}

/// Without `max_connections`, all requests overlap freely.
/// Uses `multi_thread` with real time to get genuine concurrency.
#[tokio::test(flavor = "multi_thread")]
async fn without_max_connections_concurrency_is_unbounded() {
    let (server, addr, peak) = start_server(Duration::from_millis(200)).await;
    let _server = tokio::spawn(server);

    let http_client = Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https();

    let client = s3_client(addr, http_client);
    fire_concurrent_requests(&client, TASK_COUNT).await;

    let observed = peak.load(Ordering::SeqCst);
    assert!(
        observed > MAX_CONNS,
        "without max_connections, expected peak ({observed}) to exceed {MAX_CONNS}"
    );
}

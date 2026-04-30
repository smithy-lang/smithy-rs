/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(all(feature = "wire-mock", feature = "default-client"))]

use aws_smithy_http_client::test_util::wire::connection::{
    ConnectionBehavior, ConnectionTestHarness,
};
use std::net::{IpAddr, Ipv4Addr};

const IP1: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
const IP2: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

/// Check if a loopback address is bindable on this system.
/// On macOS, addresses other than 127.0.0.1 require explicit loopback aliases.
async fn is_bindable(ip: IpAddr) -> bool {
    tokio::net::TcpListener::bind((ip, 0u16)).await.is_ok()
}

#[tokio::test]
async fn test_harness_multi_ip_endpoints() {
    if !is_bindable(IP2).await {
        eprintln!("skipping test: 127.0.0.2 not bindable (loopback alias not configured)");
        return;
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"hello",
            }],
        )
        .endpoint(
            IP2,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"world",
            }],
        )
        .dns_all("test.example.com")
        .build()
        .await;

    // Verify endpoints are on different IPs but same port
    let eps = &harness.endpoints;
    assert_ne!(eps[0].ip(), eps[1].ip());
    assert_eq!(eps[0].port(), eps[1].port());

    // Connect to each endpoint directly to verify they work
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    for ep in &harness.endpoints {
        let mut stream = TcpStream::connect(ep.addr()).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(
            response.contains("200"),
            "Expected 200 response, got: {}",
            response
        );
    }

    assert_eq!(harness.tcp_accepted_count(), 2);
    assert_eq!(harness.tcp_accepted_by(IP1), 1);
    assert_eq!(harness.tcp_accepted_by(IP2), 1);
}

#[tokio::test]
async fn test_harness_reset_on_connect() {
    let harness = ConnectionTestHarness::builder()
        .endpoint(IP1, vec![ConnectionBehavior::ResetOnConnect])
        .build()
        .await;

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(harness.endpoints[0].addr())
        .await
        .unwrap();
    // Try to read — should get connection reset or EOF
    let mut buf = vec![0u8; 1024];
    let result = stream.read(&mut buf).await;
    assert!(
        result.is_err() || result.unwrap() == 0,
        "Expected connection reset or EOF"
    );

    assert_eq!(harness.tcp_accepted_count(), 1);
}

#[tokio::test]
async fn test_mock_dns_resolver() {
    if !is_bindable(IP2).await {
        eprintln!("skipping test: 127.0.0.2 not bindable (loopback alias not configured)");
        return;
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"ok",
            }],
        )
        .endpoint(
            IP2,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"ok",
            }],
        )
        .dns("s3.amazonaws.com", vec![IP1, IP2])
        .build()
        .await;

    use aws_smithy_runtime_api::client::dns::ResolveDns;
    let resolver = harness.dns_resolver();
    let ips = resolver.resolve_dns("s3.amazonaws.com").await.unwrap();
    assert_eq!(ips.len(), 2);
    assert!(ips.contains(&IP1));
    assert!(ips.contains(&IP2));
    assert_eq!(harness.dns_lookup_count(), 1);
}

#[tokio::test]
async fn test_harness_keep_alive_reuse() {
    // Multiple responses on a single connection (keep-alive)
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"first",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 201,
                    body: b"second",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 202,
                    body: b"third",
                },
            ],
        )
        .build()
        .await;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(harness.endpoints[0].addr())
        .await
        .unwrap();

    for (i, expected_status) in ["200", "201", "202"].iter().enumerate() {
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(
            response.contains(expected_status),
            "Request {i}: expected {expected_status}, got: {response}"
        );
    }

    // Only 1 TCP connection was accepted — all 3 requests reused it
    assert_eq!(harness.tcp_accepted_count(), 1);
}

#[tokio::test]
async fn test_harness_hold_then_close() {
    use std::time::{Duration, Instant};

    let hold_duration = Duration::from_millis(100);
    let harness = ConnectionTestHarness::builder()
        .endpoint(IP1, vec![ConnectionBehavior::HoldThenClose(hold_duration)])
        .build()
        .await;

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpStream;

    let start = Instant::now();
    let mut stream = TcpStream::connect(harness.endpoints[0].addr())
        .await
        .unwrap();
    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let elapsed = start.elapsed();

    // Connection held open then closed — read returns 0 (EOF)
    assert_eq!(n, 0, "Expected EOF after hold-then-close");
    assert!(
        elapsed >= hold_duration,
        "Expected at least {hold_duration:?} hold, got {elapsed:?}"
    );
    assert_eq!(harness.tcp_accepted_count(), 1);
}

#[tokio::test]
async fn test_harness_dns_all_includes_all_endpoints() {
    if !is_bindable(IP2).await {
        eprintln!("skipping test: 127.0.0.2 not bindable (loopback alias not configured)");
        return;
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"a",
            }],
        )
        .endpoint(
            IP2,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"b",
            }],
        )
        .dns_all("example.com")
        .build()
        .await;

    use aws_smithy_runtime_api::client::dns::ResolveDns;
    let ips = harness
        .dns_resolver()
        .resolve_dns("example.com")
        .await
        .unwrap();
    assert_eq!(ips.len(), 2);
    assert!(ips.contains(&IP1));
    assert!(ips.contains(&IP2));
}

#[tokio::test]
async fn test_mock_dns_unknown_host_returns_empty() {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"ok",
            }],
        )
        .dns("known.com", vec![IP1])
        .build()
        .await;

    use aws_smithy_runtime_api::client::dns::ResolveDns;
    let ips = harness
        .dns_resolver()
        .resolve_dns("unknown.com")
        .await
        .unwrap();
    assert!(ips.is_empty(), "Unknown host should return empty IP list");
    assert_eq!(
        harness.dns_lookup_count(),
        1,
        "Lookup should still be recorded"
    );
}

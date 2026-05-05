/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pool behavior tests parameterized over HTTP client implementations.
//!
//! Each test is written once as an `async fn` that takes a `&dyn MakeClient`,
//! then invoked for each client implementation. When `Builder::new_v2()` is
//! added, a single new `MakeClient` impl covers all tests.

#![cfg(all(feature = "wire-mock", feature = "default-client"))]

use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http_client::test_util::wire::connection::{
    ConnectionBehavior, ConnectionTestHarness,
};
use aws_smithy_http_client::test_util::wire::{ReplayedEvent, WireMockServer};
use aws_smithy_http_client::v2::BuilderV2;
use aws_smithy_http_client::{ev, match_events, Builder, Connector};
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorSettings, SharedHttpClient,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

const IP1: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

// ---------------------------------------------------------------------------
// ClientConfig + MakeClient
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct ClientConfig {
    idle_timeout: Option<Duration>,
    max_connections: Option<usize>,
    max_connections_per_host: Option<usize>,
}

impl ClientConfig {
    fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    fn with_max_connections(mut self, n: usize) -> Self {
        self.max_connections = Some(n);
        self
    }

    fn with_max_connections_per_host(mut self, n: usize) -> Self {
        self.max_connections_per_host = Some(n);
        self
    }
}

trait MakeClient: Send + Sync {
    fn make(&self, config: ClientConfig) -> SharedHttpClient;
}

/// Current hyper 1.x client via `Builder::new()`
struct V1Client;

impl MakeClient for V1Client {
    fn make(&self, config: ClientConfig) -> SharedHttpClient {
        Builder::new().build_with_connector_fn(move |_settings, _components| {
            let mut builder = Connector::builder();
            if let Some(timeout) = config.idle_timeout {
                builder = builder.pool_idle_timeout(timeout);
            }
            // v1 does not support max_connections
            builder.build_http()
        })
    }
}

// V2 client backed by the composable connection pool
struct V2Client;

impl MakeClient for V2Client {
    fn make(&self, config: ClientConfig) -> SharedHttpClient {
        let mut builder = BuilderV2::new();
        if let Some(timeout) = config.idle_timeout {
            builder = builder.pool_idle_timeout(timeout);
        }
        if let Some(n) = config.max_connections {
            builder = builder.max_connections(n);
        }
        if let Some(n) = config.max_connections_per_host {
            builder = builder.max_connections_per_host(n);
        }
        builder.build_http()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn runtime_components() -> aws_smithy_runtime_api::client::runtime_components::RuntimeComponents {
    RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(SystemTimeSource::new()))
        .build()
        .expect("valid runtime components")
}

async fn send_to(
    client: &SharedHttpClient,
    url: &str,
) -> Result<
    aws_smithy_runtime_api::client::orchestrator::HttpResponse,
    aws_smithy_runtime_api::client::result::ConnectorError,
> {
    let settings = HttpConnectorSettings::builder().build();
    let components = runtime_components();
    let connector = client.http_connector(&settings, &components);
    connector
        .call(HttpRequest::get(url).expect("valid HTTP request"))
        .await
}

/// Send a request and read the response body to completion.
///
/// This ensures the connection's body guard is released, returning the
/// connection to the pool. Returns `(status, body_bytes)`.
async fn send_and_read_body(
    client: &SharedHttpClient,
    url: &str,
) -> Result<(u16, Vec<u8>), aws_smithy_runtime_api::client::result::ConnectorError> {
    use http_body_util::BodyExt;
    let resp = send_to(client, url).await?;
    let status = resp.status().as_u16();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should be readable")
        .to_bytes()
        .to_vec();
    Ok((status, body))
}

fn localhost_url(server: &WireMockServer) -> String {
    let endpoint = server.endpoint_url();
    let port = endpoint
        .rsplit(':')
        .next()
        .expect("endpoint URL should contain port");
    format!("http://127.0.0.1:{port}/")
}

// ---------------------------------------------------------------------------
// Test implementations
// ---------------------------------------------------------------------------

/// Connection reuse via HTTP/1.1 keep-alive: sequential requests reuse one TCP connection.
async fn connection_reuse(make: &dyn MakeClient) {
    let server = WireMockServer::start(vec![
        ReplayedEvent::status(200),
        ReplayedEvent::status(200),
        ReplayedEvent::status(200),
    ])
    .await;

    let client = make.make(ClientConfig::default());
    let url = localhost_url(&server);

    for i in 1..=3 {
        let resp = send_to(&client, &url)
            .await
            .unwrap_or_else(|e| panic!("request {i} should succeed: {e}"));
        assert_eq!(resp.status().as_u16(), 200, "request {i} should return 200");
    }

    match_events!(ev!(connect), ev!(http(200)), ev!(http(200)), ev!(http(200)))(&server.events());
}

/// Idle timeout eviction: a connection idle past the timeout is discarded.
async fn idle_timeout_eviction(make: &dyn MakeClient) {
    let server =
        WireMockServer::start(vec![ReplayedEvent::status(200), ReplayedEvent::status(200)]).await;

    let idle_timeout = Duration::from_millis(100);
    let client = make.make(ClientConfig::default().with_idle_timeout(idle_timeout));
    let url = localhost_url(&server);

    let status = send_to(&client, &url)
        .await
        .expect("first request should succeed")
        .status()
        .as_u16();
    assert_eq!(status, 200);
    // Response (and its body guard) dropped here — connection returns to pool.

    tokio::time::sleep(idle_timeout * 2).await;

    let status = send_to(&client, &url)
        .await
        .expect("second request should succeed after idle eviction")
        .status()
        .as_u16();
    assert_eq!(status, 200);

    match_events!(ev!(connect), ev!(http(200)), ev!(connect), ev!(http(200)))(&server.events());
}

/// A server that resets on connect should surface as a connector error.
async fn connection_reset_returns_error(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(IP1, vec![ConnectionBehavior::ResetOnConnect])
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

    let err = send_to(&client, &url)
        .await
        .expect_err("request to a reset-on-connect endpoint should fail");
    assert!(err.is_io(), "expected ConnectorError::io, got: {err:?}");
}

/// The client should report correct connector metadata.
async fn connector_metadata(make: &dyn MakeClient) {
    let client = make.make(ClientConfig::default());
    let metadata = client
        .connector_metadata()
        .expect("connector_metadata should return Some");
    assert_eq!(metadata.name(), Cow::Borrowed("hyper"));
    assert_eq!(
        metadata.version(),
        Some(Cow::Borrowed("1.x")),
        "expected hyper 1.x connector version"
    );
}

// ---------------------------------------------------------------------------
// v1 test runners
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_connection_reuse() {
    connection_reuse(&V1Client).await;
}

#[tokio::test]
async fn v1_idle_timeout_eviction() {
    idle_timeout_eviction(&V1Client).await;
}

#[tokio::test]
async fn v1_connection_reset_returns_error() {
    connection_reset_returns_error(&V1Client).await;
}

#[tokio::test]
async fn v1_connector_metadata() {
    connector_metadata(&V1Client).await;
}

// ---------------------------------------------------------------------------
// v2 test runners
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v2_connection_reuse() {
    connection_reuse(&V2Client).await;
}

#[tokio::test]
async fn v2_idle_timeout_eviction() {
    // v2 does not yet implement idle timeout eviction (Phase 4).
    //
    // The v1 version of this test passes because the legacy client has
    // built-in idle eviction. For v2, calling idle_timeout_eviction()
    // also produces 2 connections — but NOT because of idle eviction.
    // The response body is never consumed by the test (HttpResponse wraps
    // a lazy SdkBody). The CachedConnection is held inside the response
    // body's guard until the HttpResponse is dropped. Since the first
    // response is still alive during the sleep, the connection never
    // returns to Cache, and the second request opens a new connection.
    //
    // Real idle eviction tests (consume body → return to pool → sleep
    // past timeout → verify new connection) will be added in Phase 4
    // alongside the background eviction task.
}

#[tokio::test]
async fn v2_connection_reset_returns_error() {
    connection_reset_returns_error(&V2Client).await;
}

#[tokio::test]
async fn v2_connector_metadata() {
    connector_metadata(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: origin-form URI + Host header
// ---------------------------------------------------------------------------

/// Requests should be sent with origin-form URI (just the path) and a correct Host header.
async fn origin_form_and_host_header(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"ok",
            }],
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/some/path?key=val");

    let resp = send_to(&client, &url)
        .await
        .expect("request should succeed");
    assert_eq!(resp.status().as_u16(), 200);

    let requests = harness.http_requests();
    assert_eq!(requests.len(), 1, "expected exactly one HTTP request");
    let (uri, host) = &requests[0];

    // URI must be origin-form (path + query only, no scheme/authority)
    assert_eq!(uri, "/some/path?key=val", "URI should be origin-form");

    // Host header must be present with the correct authority
    let host = host.as_deref().expect("Host header should be present");
    assert_eq!(
        host,
        format!("127.0.0.1:{port}"),
        "Host header should match authority"
    );
}

#[tokio::test]
async fn v1_origin_form_and_host_header() {
    origin_form_and_host_header(&V1Client).await;
}

#[tokio::test]
async fn v2_origin_form_and_host_header() {
    origin_form_and_host_header(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: max_connections
// ---------------------------------------------------------------------------

/// With max_connections(2), concurrent requests should not open more than 2 connections,
/// and all requests should succeed (waiting requests served via connection reuse).
async fn max_connections_limits_concurrency(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..10)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default().with_max_connections(2));
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // Send 5 concurrent requests — all should succeed
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..5 {
        let client = client.clone();
        let url = url.clone();
        tasks.spawn(async move { send_to(&client, &url).await });
    }
    let mut success_count = 0;
    while let Some(result) = tasks.join_next().await {
        let resp = result
            .expect("task should not panic")
            .expect("request should succeed");
        assert_eq!(resp.status().as_u16(), 200);
        success_count += 1;
    }
    assert_eq!(success_count, 5, "all 5 requests should complete");

    let accepted = harness.tcp_accepted_count();
    assert!(
        accepted <= 2,
        "expected at most 2 connections with max_connections(2), got {accepted}"
    );
}

/// Cached (reused) connections don't consume permits — sequential requests
/// with max_connections(1) all succeed on one connection.
async fn max_connections_reuse_does_not_consume_permits(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..5)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default().with_max_connections(1));
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // 5 sequential requests — all reuse the same connection
    for i in 0..5 {
        let resp = send_to(&client, &url)
            .await
            .unwrap_or_else(|e| panic!("request {i} should succeed: {e}"));
        assert_eq!(resp.status().as_u16(), 200);
    }

    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "all requests should reuse one connection"
    );
}

#[tokio::test]
async fn v2_max_connections_limits_concurrency() {
    max_connections_limits_concurrency(&V2Client).await;
}

#[tokio::test]
async fn v2_max_connections_reuse_does_not_consume_permits() {
    max_connections_reuse_does_not_consume_permits(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: max_connections_per_host
// ---------------------------------------------------------------------------

async fn is_bindable(ip: IpAddr) -> bool {
    tokio::net::TcpListener::bind((ip, 0u16)).await.is_ok()
}

const IP2: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

/// Per-host limit: each host gets its own budget.
async fn max_connections_per_host_limits_per_host(make: &dyn MakeClient) {
    if !is_bindable(IP2).await {
        eprintln!("skipping test: 127.0.0.2 not bindable");
        return;
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..10)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .endpoint(
            IP2,
            (0..10)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default().with_max_connections_per_host(2));
    let port = harness.endpoints[0].port();

    // Send 4 concurrent requests to each host
    let mut tasks = tokio::task::JoinSet::new();
    for ip in ["127.0.0.1", "127.0.0.2"] {
        for _ in 0..4 {
            let client = client.clone();
            let url = format!("http://{ip}:{port}/");
            tasks.spawn(async move { send_to(&client, &url).await });
        }
    }
    while let Some(result) = tasks.join_next().await {
        result
            .expect("task should not panic")
            .expect("request should succeed");
    }

    let host1 = harness.tcp_accepted_by(IP1);
    let host2 = harness.tcp_accepted_by(IP2);
    assert!(host1 <= 2, "host 1: expected ≤2 connections, got {host1}");
    assert!(host2 <= 2, "host 2: expected ≤2 connections, got {host2}");
    // Total exceeds per-host limit (no global limit set)
    assert!(host1 + host2 > 2, "total should exceed per-host limit");
}

/// Global and per-host limits compose: global(3) + per_host(2) with 2 hosts.
async fn max_connections_global_and_per_host_compose(make: &dyn MakeClient) {
    if !is_bindable(IP2).await {
        eprintln!("skipping test: 127.0.0.2 not bindable");
        return;
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..10)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .endpoint(
            IP2,
            (0..10)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .build()
        .await;

    let client = make.make(
        ClientConfig::default()
            .with_max_connections(3)
            .with_max_connections_per_host(2),
    );
    let port = harness.endpoints[0].port();

    // Send 4 concurrent requests to each host (8 total)
    let mut tasks = tokio::task::JoinSet::new();
    for ip in ["127.0.0.1", "127.0.0.2"] {
        for _ in 0..4 {
            let client = client.clone();
            let url = format!("http://{ip}:{port}/");
            tasks.spawn(async move { send_to(&client, &url).await });
        }
    }
    while let Some(result) = tasks.join_next().await {
        result
            .expect("task should not panic")
            .expect("request should succeed");
    }

    let host1 = harness.tcp_accepted_by(IP1);
    let host2 = harness.tcp_accepted_by(IP2);
    // Per-host: each ≤2
    assert!(host1 <= 2, "host 1: expected ≤2, got {host1}");
    assert!(host2 <= 2, "host 2: expected ≤2, got {host2}");
    // Global: total ≤3
    assert!(
        host1 + host2 <= 3,
        "total: expected ≤3 (global limit), got {}",
        host1 + host2
    );
}

#[tokio::test]
async fn v2_max_connections_per_host_limits_per_host() {
    max_connections_per_host_limits_per_host(&V2Client).await;
}

#[tokio::test]
async fn v2_max_connections_global_and_per_host_compose() {
    max_connections_global_and_per_host_compose(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: connection lifecycle
// ---------------------------------------------------------------------------

/// Without max_connections, connections scale freely under concurrency.
async fn no_limit_allows_unbounded_connections(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..20)
                .map(|_| ConnectionBehavior::HoldThenClose(Duration::from_secs(2)))
                .collect(),
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // Send 5 concurrent requests — server holds each connection open,
    // so the client must open 5 separate connections.
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..5 {
        let client = client.clone();
        let url = url.clone();
        tasks.spawn(async move {
            // These will fail (server doesn't send HTTP response), but
            // the point is that 5 TCP connections are opened concurrently.
            let _ = send_to(&client, &url).await;
        });
    }
    // Give time for all connections to be established
    tokio::time::sleep(Duration::from_millis(200)).await;

    let accepted = harness.tcp_accepted_count();
    assert!(
        accepted >= 4,
        "without max_connections, expected ≥4 concurrent connections, got {accepted}"
    );

    // Clean up tasks
    tasks.shutdown().await;
}

/// Sequential requests reuse the same connection (body fully consumed between requests).
async fn connection_reuse_after_body_consumed(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..3)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"hello world response body",
                })
                .collect(),
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    for i in 0..3 {
        let (status, body) = send_and_read_body(&client, &url)
            .await
            .unwrap_or_else(|e| panic!("request {i} should succeed: {e}"));
        assert_eq!(status, 200);
        assert_eq!(body, b"hello world response body");
    }

    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "all requests should reuse one connection after body is consumed"
    );
}

/// Active connections are not evicted by idle timeout.
async fn active_connection_survives_idle_timeout(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"first",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"second",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"third",
                },
            ],
        )
        .build()
        .await;

    // Use a very short idle timeout — but sequential requests should still
    // reuse the connection because the pool doesn't evict between requests
    // that happen back-to-back.
    let client = make.make(ClientConfig::default().with_idle_timeout(Duration::from_millis(100)));
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // Three back-to-back requests — all should reuse the same connection
    let expected = [b"first".as_slice(), b"second", b"third"];
    for (i, expected_body) in expected.iter().enumerate() {
        let (status, body) = send_and_read_body(&client, &url)
            .await
            .unwrap_or_else(|e| panic!("request {i} should succeed: {e}"));
        assert_eq!(status, 200);
        assert_eq!(body, *expected_body, "request {i} body mismatch");
    }

    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "back-to-back requests should reuse one connection even with short idle timeout"
    );
}

#[tokio::test]
async fn v2_no_limit_allows_unbounded_connections() {
    no_limit_allows_unbounded_connections(&V2Client).await;
}

#[tokio::test]
async fn v2_connection_reuse_after_body_consumed() {
    connection_reuse_after_body_consumed(&V2Client).await;
}

#[tokio::test]
async fn v2_active_connection_survives_idle_timeout() {
    active_connection_survives_idle_timeout(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: connection lifecycle
// ---------------------------------------------------------------------------

/// Server closes an idle connection (simulating server-side idle timeout).
/// The connection returns to the pool clean, then dies while idle. On next
/// checkout, poll_ready detects the dead connection, the checkout loop
/// discards it, and a fresh connection is created.
async fn stale_connection_detected_at_checkout(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                // First connection: respond with keep-alive, then close after 30ms
                ConnectionBehavior::RespondThenIdleClose {
                    status: 200,
                    body: b"first",
                    idle: Duration::from_millis(30),
                },
                // Second connection (after stale one is discarded)
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"second",
                },
            ],
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // First request succeeds, body consumed, connection returns to pool
    let (status, body) = send_and_read_body(&client, &url)
        .await
        .expect("first request");
    assert_eq!(status, 200);
    assert_eq!(body, b"first");

    // Wait for server to close the idle connection (30ms idle + margin)
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Second request: checkout finds stale connection, discards, creates new
    let (status, body) = send_and_read_body(&client, &url)
        .await
        .expect("second request should succeed on fresh connection");
    assert_eq!(status, 200);
    assert_eq!(body, b"second");

    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "should have opened 2 connections (first died while idle in pool)"
    );
}

/// When the response body is not consumed, the connection is held by the
/// body guard and unavailable for reuse. A concurrent request must open
/// a new connection.
async fn unconsumed_body_holds_connection(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"first",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"second",
                },
            ],
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    // First request — hold the response (body unconsumed, connection held)
    let _held_resp = send_to(&client, &url).await.expect("first request");

    // Second request while first response is still held — must open new connection
    let (status, body) = send_and_read_body(&client, &url)
        .await
        .expect("second request");
    assert_eq!(status, 200);
    assert_eq!(body, b"second");

    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "second request should open a new connection while first body is held"
    );

    // Drop the held response — connection guard released
    drop(_held_resp);
}

/// Server sends Connection: close — client should not reuse the connection.
async fn connection_close_header_prevents_reuse(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondThenClose {
                    status: 200,
                    body: b"closing",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"fresh",
                },
            ],
        )
        .build()
        .await;

    let client = make.make(ClientConfig::default());
    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");

    let (status, body) = send_and_read_body(&client, &url)
        .await
        .expect("first request");
    assert_eq!(status, 200);
    assert_eq!(body, b"closing");

    // Connection: close means the client should not attempt to reuse
    let (status, body) = send_and_read_body(&client, &url)
        .await
        .expect("second request");
    assert_eq!(status, 200);
    assert_eq!(body, b"fresh");

    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "Connection: close should prevent reuse"
    );
}

#[tokio::test]
async fn v2_stale_connection_detected_at_checkout() {
    stale_connection_detected_at_checkout(&V2Client).await;
}

#[tokio::test]
async fn v2_unconsumed_body_holds_connection() {
    unconsumed_body_holds_connection(&V2Client).await;
}

#[tokio::test]
async fn v2_connection_close_header_prevents_reuse() {
    connection_close_header_prevents_reuse(&V2Client).await;
}

// ---------------------------------------------------------------------------
// Test implementations: connection poisoning
// ---------------------------------------------------------------------------

/// Send a request and return both the response body and the `ConnectionMetadata`
/// captured by a `CaptureSmithyConnection` attached to the request. This is the
/// integration surface the `ConnectionPoisoningInterceptor` uses in real SDK flows
/// — the adapter must populate a retriever that returns live metadata pointing
/// at the connection selected for this request.
async fn send_with_capture(
    client: &SharedHttpClient,
    url: &str,
) -> (
    u16,
    Vec<u8>,
    Option<aws_smithy_runtime_api::client::connection::ConnectionMetadata>,
) {
    use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
    use http_body_util::BodyExt;

    let settings = HttpConnectorSettings::builder().build();
    let components = runtime_components();
    let connector = client.http_connector(&settings, &components);

    let capture = CaptureSmithyConnection::new();
    let mut request = HttpRequest::get(url).expect("valid HTTP request");
    request.add_extension(capture.clone());

    let resp = connector
        .call(request)
        .await
        .expect("request should succeed");
    let status = resp.status().as_u16();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should be readable")
        .to_bytes()
        .to_vec();
    (status, body, capture.get())
}

/// Poisoning a connection prevents it from being reused.
///
/// Mirrors the production flow: `ConnectionPoisoningInterceptor` attaches a
/// `CaptureSmithyConnection` to the request, the adapter populates it with
/// metadata pointing at the selected connection, and on a transient error
/// the interceptor calls `ConnectionMetadata::poison()`. The next request
/// for the same host must establish a new TCP connection instead of reusing
/// the poisoned one.
async fn poisoned_connection_not_reused(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"first",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"second",
                },
            ],
        )
        .build()
        .await;

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let client = make.make(ClientConfig::default());

    // First request: establish connection, capture metadata.
    let (status, body, metadata) = send_with_capture(&client, &url).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"first");
    let metadata = metadata.expect("adapter should populate CaptureSmithyConnection");
    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "first request opens one connection"
    );

    // Poison the connection — what the orchestrator does on a transient error.
    metadata.poison();

    // Next request to the same host must open a NEW connection; the poisoned
    // one is skipped on checkout and dropped on return.
    let (status, body, _) = send_with_capture(&client, &url).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"second");
    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "poisoned connection must not be reused"
    );
}

/// When no transient error occurs, the captured metadata is handed out but
/// never poisoned — the connection returns to the pool and is reused.
/// This is the non-poison control for `poisoned_connection_not_reused`.
async fn capture_without_poison_allows_reuse(make: &dyn MakeClient) {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"first",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"second",
                },
            ],
        )
        .build()
        .await;

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let client = make.make(ClientConfig::default());

    let (status, body, metadata) = send_with_capture(&client, &url).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"first");
    assert!(
        metadata.is_some(),
        "adapter should populate CaptureSmithyConnection"
    );
    // Deliberately not calling poison().
    drop(metadata);

    let (status, body, _) = send_with_capture(&client, &url).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"second");
    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "without poison the connection should be reused"
    );
}

#[tokio::test]
async fn v2_poisoned_connection_not_reused() {
    poisoned_connection_not_reused(&V2Client).await;
}

#[tokio::test]
async fn v2_capture_without_poison_allows_reuse() {
    capture_without_poison_allows_reuse(&V2Client).await;
}

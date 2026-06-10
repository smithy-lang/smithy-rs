/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pool behavior tests parameterized over HTTP client implementations.
//!
//! Each test is written once as an `async fn` that takes a `&dyn MakeClient`,
//! then invoked for each client implementation.

#![cfg(all(feature = "wire-mock", feature = "default-client"))]

use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http_client::pool::{Client as PoolClient, SharedPool};
use aws_smithy_http_client::test_util::wire::connection::{
    ConnectionBehavior, ConnectionTestHarness,
};
use aws_smithy_http_client::test_util::wire::{ReplayedEvent, WireMockServer};
use aws_smithy_http_client::{ev, match_events, Builder, Connector};
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorSettings, SharedHttpClient,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::shared::IntoShared;
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
        let mut builder = SharedPool::builder();
        if let Some(timeout) = config.idle_timeout {
            builder = builder.pool_idle_timeout(timeout);
        }
        if let Some(n) = config.max_connections {
            builder = builder.max_connections(n);
        }
        if let Some(n) = config.max_connections_per_host {
            builder = builder.max_connections_per_host(n);
        }
        let pool = builder.build_http();
        PoolClient::new(&pool).into_shared()
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

// ---------------------------------------------------------------------------
// Test implementations: per-operation timeouts
// ---------------------------------------------------------------------------

/// Read timeout fires when the server accepts TCP but never sends a response.
///
/// Exercises the v2 adapter's per-op timeout wrapping end-to-end: the
/// `HttpConnectorSettings::read_timeout` flows through
/// `HttpClient::http_connector` into `PooledConnector`, wraps
/// `pool.send_request`, fires because `HoldThenClose` never replies, and
/// produces a `ConnectorError::timeout` classified by `downcast_error`.
async fn v2_read_timeout_fires_on_silent_server() {
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::HoldThenClose(Duration::from_secs(30))],
        )
        .build()
        .await;

    let client = V2Client.make(ClientConfig::default());
    let components =
        aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(SystemTimeSource::new()))
            .with_sleep_impl(Some(SharedAsyncSleep::new(TokioSleep::new())))
            .build()
            .expect("valid runtime components");
    let settings = HttpConnectorSettings::builder()
        .read_timeout(Duration::from_millis(200))
        .build();
    let connector = client.http_connector(&settings, &components);

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let start = std::time::Instant::now();
    let err = connector
        .call(HttpRequest::get(&url).expect("valid HTTP request"))
        .await
        .expect_err("read timeout should fire against a non-responsive server");
    let elapsed = start.elapsed();

    assert!(
        err.is_timeout(),
        "expected timeout classification, got {err:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "read timeout did not fire in time (took {elapsed:?})"
    );
}

#[tokio::test]
async fn v2_read_timeout() {
    v2_read_timeout_fires_on_silent_server().await;
}

// ---------------------------------------------------------------------------
// Tracing output assertions
// ---------------------------------------------------------------------------
//
// These tests verify the pool emits useful structured tracing events.
// They use a thread-local subscriber and `current_thread` tokio so spawned
// tasks (eviction) run on the same thread. Must be run with
// `--test-threads=1` to avoid subscriber interference from parallel tests.

fn capture_pool_logs() -> (
    tracing::subscriber::DefaultGuard,
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
) {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    struct BufWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let buf_clone = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || BufWriter(buf_clone.clone()))
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (guard, buf)
}

fn captured_str(buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).expect("captured logs are utf-8")
}

/// Background eviction task emits structured tracing events including
/// pool init, connection established, eviction with reason, and host
/// entry removal.
///
/// Requires `--test-threads=1` due to thread-local subscriber; run via:
/// `cargo test --features default-client,test-util,wire-mock --test pool_behavior_test -- --ignored --test-threads=1`
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial(tracing)]
#[ignore]
async fn v2_background_eviction_emits_tracing_events() {
    let (_guard, logs) = capture_pool_logs();

    let server = WireMockServer::start(vec![ReplayedEvent::status(200)]).await;

    let idle_timeout = Duration::from_millis(100);
    let client = V2Client.make(ClientConfig::default().with_idle_timeout(idle_timeout));
    let url = localhost_url(&server);

    let status = send_to(&client, &url)
        .await
        .expect("first request should succeed")
        .status()
        .as_u16();
    assert_eq!(status, 200);

    // Wait past idle timeout + eviction tick intervals.
    tokio::time::sleep(idle_timeout * 5).await;

    let captured = captured_str(&logs);
    assert!(
        captured.contains("pool: initialized"),
        "expected pool init log. captured:\n{captured}"
    );
    assert!(
        captured.contains("pool: eviction task spawned"),
        "expected eviction task spawn log. captured:\n{captured}"
    );
    assert!(
        captured.contains("pool: connection established"),
        "expected connection-established log. captured:\n{captured}"
    );
    assert!(
        captured.contains("pool: connection evicted"),
        "expected connection-evicted log. captured:\n{captured}"
    );
    assert!(
        captured.contains("pool: host entry removed"),
        "expected host-entry-removed log. captured:\n{captured}"
    );
}

/// conn_id is stable (same across a connection's lifetime) and monotonically
/// increasing (new connection after eviction gets the next id).
///
/// Requires `--test-threads=1`; see `v2_background_eviction_emits_tracing_events`.
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial(tracing)]
#[ignore]
async fn v2_conn_id_is_stable_and_monotonic() {
    let (_guard, logs) = capture_pool_logs();

    let server =
        WireMockServer::start(vec![ReplayedEvent::status(200), ReplayedEvent::status(200)]).await;

    let idle_timeout = Duration::from_millis(100);
    let client = V2Client.make(ClientConfig::default().with_idle_timeout(idle_timeout));
    let url = localhost_url(&server);

    send_to(&client, &url).await.expect("first request");
    tokio::time::sleep(idle_timeout * 5).await;
    send_to(&client, &url).await.expect("second request");

    let captured = captured_str(&logs);
    assert!(
        captured.contains("conn_id=0"),
        "first connection should be conn_id=0. captured:\n{captured}"
    );
    assert!(
        captured.contains("conn_id=1"),
        "second connection (after eviction) should be conn_id=1. captured:\n{captured}"
    );
}

// ---------------------------------------------------------------------------
// Connect timeout
// ---------------------------------------------------------------------------

/// Proves that `connect_timeout` from `HttpConnectorSettings` fires when the
/// TCP connection cannot be established within the deadline. Uses TEST-NET-1
/// (192.0.2.1), a non-routable address guaranteed to black-hole SYN packets.
#[tokio::test]
async fn v2_connect_timeout() {
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};

    let client = V2Client.make(ClientConfig::default());
    let components =
        aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(SystemTimeSource::new()))
            .with_sleep_impl(Some(SharedAsyncSleep::new(TokioSleep::new())))
            .build()
            .expect("valid runtime components");
    let settings = HttpConnectorSettings::builder()
        .connect_timeout(Duration::from_millis(500))
        .build();
    let connector = client.http_connector(&settings, &components);

    let start = std::time::Instant::now();
    let err = connector
        .call(HttpRequest::get("http://192.0.2.1:1234/unreachable").expect("valid request"))
        .await
        .expect_err("connect timeout should fire against non-routable address");
    let elapsed = start.elapsed();

    assert!(
        err.is_timeout(),
        "expected timeout classification, got {err:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(450),
        "timeout fired too early ({elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout took too long ({elapsed:?})"
    );
}

/// `ConnectionMetadata::connection_id()` surfaces the pool-assigned id
/// through `CaptureSmithyConnection`. Sequential requests on the same
/// connection share the same id; a new connection after eviction gets a
/// different id.
#[tokio::test]
async fn v2_connection_id_surfaced_through_metadata() {
    use aws_smithy_runtime_api::client::connection::ConnectionId;

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"a",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"b",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"c",
                },
            ],
        )
        .build()
        .await;

    let client = V2Client.make(ClientConfig {
        idle_timeout: Some(Duration::from_millis(80)),
        ..Default::default()
    });
    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

    // First two requests reuse the same connection.
    let (_, _, meta1) = send_with_capture(&client, &url).await;
    let (_, _, meta2) = send_with_capture(&client, &url).await;
    let id1 = meta1
        .unwrap()
        .connection_id()
        .expect("v2 sets connection_id");
    let id2 = meta2
        .unwrap()
        .connection_id()
        .expect("v2 sets connection_id");
    assert_eq!(id1, id2, "same connection should have same id");
    assert_eq!(id1, ConnectionId::new(0));

    // Wait for eviction, then the next request gets a new connection.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (_, _, meta3) = send_with_capture(&client, &url).await;
    let id3 = meta3
        .unwrap()
        .connection_id()
        .expect("v2 sets connection_id");
    assert_ne!(
        id1, id3,
        "new connection after eviction should have different id"
    );
    assert_eq!(id3, ConnectionId::new(1));
}

// ---- ConnectionEventListener tests ----

mod listener_tests {
    use super::*;
    use aws_smithy_http_client::pool::{
        CloseReason, ConnectionClosedEvent, ConnectionCreatedEvent, ConnectionEventListener,
        ConnectionFailedEvent, ConnectionReusedEvent,
    };
    use std::sync::{Arc, Mutex};

    /// Records connection lifecycle events for test assertions.
    #[derive(Debug, Clone, Default)]
    struct RecordingListener {
        /// (conn_id, authority) for each connection created
        created: Arc<Mutex<Vec<(u64, String)>>>,
        /// (conn_id, authority) for each connection reused from idle
        reused: Arc<Mutex<Vec<(u64, String)>>>,
        /// (conn_id, authority, reason) for each connection closed
        closed: Arc<Mutex<Vec<(u64, String, CloseReason)>>>,
        /// authority for each failed connection attempt
        failed: Arc<Mutex<Vec<String>>>,
    }

    impl ConnectionEventListener for RecordingListener {
        fn on_created(&self, event: &ConnectionCreatedEvent) {
            self.created.lock().unwrap().push((
                event.conn_id().to_string().parse().unwrap(),
                event.authority().as_str().to_string(),
            ));
        }
        fn on_reused(&self, event: &ConnectionReusedEvent) {
            self.reused.lock().unwrap().push((
                event.conn_id().to_string().parse().unwrap(),
                event.authority().as_str().to_string(),
            ));
        }
        fn on_closed(&self, event: &ConnectionClosedEvent) {
            self.closed.lock().unwrap().push((
                event.conn_id().to_string().parse().unwrap(),
                event.authority().as_str().to_string(),
                event.reason(),
            ));
        }
        fn on_connection_failed(&self, event: &ConnectionFailedEvent) {
            self.failed
                .lock()
                .unwrap()
                .push(event.authority().as_str().to_string());
        }
    }

    fn make_v2_with_listener(
        harness: &ConnectionTestHarness,
        idle_timeout: Option<Duration>,
        listener: Arc<dyn ConnectionEventListener>,
    ) -> SharedHttpClient {
        let mut builder = aws_smithy_http_client::pool::SharedPool::builder()
            .connection_event_listener(listener)
            .dns_resolver(harness.dns_resolver());
        if let Some(timeout) = idle_timeout {
            builder = builder.pool_idle_timeout(timeout);
        }
        let pool = builder.build_http();
        PoolClient::new(&pool).into_shared()
    }

    /// Listener receives created on first request, reused on second, and
    /// closed(IdleTimeout) after eviction.
    #[tokio::test]
    async fn listener_lifecycle_created_reused_closed() {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                vec![
                    ConnectionBehavior::RespondKeepAlive {
                        status: 200,
                        body: b"a",
                    },
                    ConnectionBehavior::RespondKeepAlive {
                        status: 200,
                        body: b"b",
                    },
                ],
            )
            .build()
            .await;

        let listener = Arc::new(RecordingListener::default());
        let client =
            make_v2_with_listener(&harness, Some(Duration::from_millis(80)), listener.clone());
        let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

        // First request: on_created
        send_and_read_body(&client, &url).await;
        assert_eq!(listener.created.lock().unwrap().len(), 1);
        assert_eq!(listener.created.lock().unwrap()[0].0, 0);
        assert!(listener.created.lock().unwrap()[0].1.contains("127.0.0.1"));

        // Second request: on_reused
        send_and_read_body(&client, &url).await;
        assert_eq!(listener.reused.lock().unwrap().len(), 1);
        assert_eq!(listener.reused.lock().unwrap()[0].0, 0);

        // Wait for eviction
        tokio::time::sleep(Duration::from_millis(200)).await;

        let closed = listener.closed.lock().unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].0, 0);
        assert_eq!(closed[0].2, CloseReason::IdleTimeout);
    }

    /// Poisoning a connection fires on_closed with Poisoned reason.
    #[tokio::test]
    async fn listener_poisoned_connection_fires_on_closed() {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                vec![
                    ConnectionBehavior::RespondKeepAlive {
                        status: 200,
                        body: b"ok",
                    },
                    ConnectionBehavior::RespondKeepAlive {
                        status: 200,
                        body: b"ok",
                    },
                ],
            )
            .build()
            .await;

        let listener = Arc::new(RecordingListener::default());
        let client = make_v2_with_listener(&harness, None, listener.clone());
        let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

        // Make a request and poison the connection
        let (_, _, meta) = super::send_with_capture(&client, &url).await;
        meta.unwrap().poison();

        // The poisoned connection is discarded on next checkout attempt
        send_and_read_body(&client, &url).await;

        let closed = listener.closed.lock().unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].2, CloseReason::Poisoned);
    }

    /// Connection failure fires on_connection_failed with the authority.
    #[tokio::test]
    async fn listener_connection_failed() {
        let listener = Arc::new(RecordingListener::default());

        // Use a harness with no endpoints so the connection will fail
        let harness = ConnectionTestHarness::builder()
            .endpoint(IP1, vec![ConnectionBehavior::ResetOnConnect])
            .build()
            .await;

        let client = make_v2_with_listener(&harness, None, listener.clone());
        let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

        let settings = HttpConnectorSettings::builder().build();
        let components = runtime_components();
        let connector = client.http_connector(&settings, &components);
        let request = HttpRequest::get(&url).expect("valid request");
        let _ = connector.call(request).await;

        let failed = listener.failed.lock().unwrap();
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("127.0.0.1"));
    }

    /// Authority is correctly populated on events.
    #[tokio::test]
    async fn listener_authority_populated() {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                vec![ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"x",
                }],
            )
            .build()
            .await;

        let listener = Arc::new(RecordingListener::default());
        let client = make_v2_with_listener(&harness, None, listener.clone());
        let port = harness.endpoints[0].port();
        let url = format!("http://127.0.0.1:{}/path", port);

        send_and_read_body(&client, &url).await;

        let created = listener.created.lock().unwrap();
        assert_eq!(created[0].1, format!("127.0.0.1:{}", port));
    }

    /// A connection that the server closed while idle fires on_closed
    /// with Unusable reason when detected at checkout.
    #[tokio::test]
    async fn listener_unusable_connection_fires_on_closed() {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                vec![
                    // First request succeeds, then server closes the connection.
                    ConnectionBehavior::RespondThenClose {
                        status: 200,
                        body: b"ok",
                    },
                    // Second connection for the retry.
                    ConnectionBehavior::RespondKeepAlive {
                        status: 200,
                        body: b"ok",
                    },
                ],
            )
            .build()
            .await;

        let listener = Arc::new(RecordingListener::default());
        let client = make_v2_with_listener(&harness, None, listener.clone());
        let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

        // First request succeeds; connection returns to pool.
        send_and_read_body(&client, &url).await;

        // Brief pause for the server-side close to propagate.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Second request: pool checks out the dead connection, detects it
        // in poll_ready, fires on_closed(Unusable), then creates a new one.
        send_and_read_body(&client, &url).await;

        let closed = listener.closed.lock().unwrap();
        let unusable = closed.iter().find(|c| c.2 == CloseReason::Unusable);
        assert!(
            unusable.is_some(),
            "expected on_closed with Unusable reason, got: {:?}",
            *closed
        );
        assert_eq!(unusable.unwrap().0, 0, "should be the first connection");
    }

    /// ConnectionCreatedEvent carries non-zero connect_duration.
    #[tokio::test]
    async fn listener_timing_populated_on_created() {
        let timing: Arc<Mutex<Option<Duration>>> = Arc::new(Mutex::new(None));

        struct TimingListener(Arc<Mutex<Option<Duration>>>);
        impl ConnectionEventListener for TimingListener {
            fn on_created(&self, event: &ConnectionCreatedEvent) {
                *self.0.lock().unwrap() = Some(event.timing().connect_duration());
            }
        }

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

        let listener: Arc<dyn ConnectionEventListener> = Arc::new(TimingListener(timing.clone()));
        let client = make_v2_with_listener(&harness, None, listener);
        let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

        send_and_read_body(&client, &url).await;

        let duration = timing.lock().unwrap().expect("timing should be set");
        assert!(duration > Duration::ZERO, "connect_duration should be > 0");
    }
}

/// Prove that a partition's declared spawner is used to spawn the connection
/// driver, not the free `TokioExecutor::new().execute(…)` helper that would
/// target whatever runtime is current at connect time.
#[tokio::test]
async fn partition_spawners_are_isolated() {
    use aws_smithy_http_client::pool::{DriverSpawner, Partition, PartitionId, TokioDriverSpawner};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A spawner that records how many times `spawn` was called, then
    /// delegates to an inner `TokioDriverSpawner` so the driver runs.
    #[derive(Debug)]
    struct RecordingSpawner {
        inner: TokioDriverSpawner,
        spawned: Arc<AtomicUsize>,
    }

    impl RecordingSpawner {
        fn new(spawned: Arc<AtomicUsize>) -> Self {
            Self {
                inner: TokioDriverSpawner::current(),
                spawned,
            }
        }
    }

    impl DriverSpawner for RecordingSpawner {
        fn spawn(
            &self,
            driver: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
        ) {
            self.spawned.fetch_add(1, Ordering::Relaxed);
            self.inner.spawn(driver);
        }
    }

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"hello",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"hello",
                },
            ],
        )
        .build()
        .await;

    // Two partitions, each with its own recording spawner.
    let count0 = Arc::new(AtomicUsize::new(0));
    let count1 = Arc::new(AtomicUsize::new(0));
    let p0 = Partition::new(
        PartitionId::from_index(0),
        RecordingSpawner::new(count0.clone()),
    );
    let p1 = Partition::new(
        PartitionId::from_index(1),
        RecordingSpawner::new(count1.clone()),
    );

    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .partitions([p0, p1])
        .build_http();

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

    // A request on partition 0 must drive its connection through spawner 0
    // only — spawner 1 stays untouched (no cross-partition leakage).
    let client0 = PoolClient::from_partition(&pool, PartitionId::from_index(0)).into_shared();
    let (status, _) = send_and_read_body(&client0, &url)
        .await
        .expect("p0 request should succeed");
    assert_eq!(status, 200);
    assert!(
        count0.load(Ordering::Relaxed) >= 1,
        "partition 0's spawner should have driven its connection"
    );
    assert_eq!(
        count1.load(Ordering::Relaxed),
        0,
        "partition 1's spawner must not be touched by a partition 0 request"
    );

    // A request on partition 1 drives through spawner 1; spawner 0's count
    // does not change (each partition opens its own connection).
    let count0_before = count0.load(Ordering::Relaxed);
    let client1 = PoolClient::from_partition(&pool, PartitionId::from_index(1)).into_shared();
    let (status, _) = send_and_read_body(&client1, &url)
        .await
        .expect("p1 request should succeed");
    assert_eq!(status, 200);
    assert!(
        count1.load(Ordering::Relaxed) >= 1,
        "partition 1's spawner should have driven its connection"
    );
    assert_eq!(
        count0.load(Ordering::Relaxed),
        count0_before,
        "partition 0's spawner must not be touched by a partition 1 request"
    );
}

// ---------------------------------------------------------------------------
// Test implementations: cross-partition active reclaim (Never policy)
// ---------------------------------------------------------------------------

/// Under cap pressure, a starved partition reclaims an over-supplied peer's
/// idle connection (freeing its permit) and connects locally — rather than
/// waiting for passive idle eviction.
///
/// Setup distinguishes *active* reclaim from passive eviction: the pool's
/// idle timeout is set very long, so the eviction tick will not fire during
/// the test. P1 establishes an idle connection holding the single global
/// permit; P0 (same authority, same NIC group) then requests, finds the cap
/// bound, reclaims P1's *non-expired* idle, and connects on a fresh
/// connection. Proven via the `ConnectionEventListener`:
///   - P1's connection closes with `CloseReason::Reclaimed` (not
///     `IdleTimeout` — the tick never ran),
///   - P0's connection is freshly created (distinct `conn_id`),
///   - the server accepted two connections (P1's reclaimed, P0's new),
///   - P0 completed well under the idle timeout.
#[tokio::test]
async fn v2_cross_partition_reclaim_frees_peer_idle() {
    use aws_smithy_http_client::pool::{
        CloseReason, ConnectionClosedEvent, ConnectionCreatedEvent, ConnectionEventListener,
        ConnectionFailedEvent, ConnectionReusedEvent, Partition, PartitionId, TokioDriverSpawner,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, Default)]
    struct RecordingListener {
        created: Arc<Mutex<Vec<(u64, String)>>>,
        closed: Arc<Mutex<Vec<(u64, String, CloseReason)>>>,
    }
    impl ConnectionEventListener for RecordingListener {
        fn on_created(&self, event: &ConnectionCreatedEvent) {
            self.created.lock().unwrap().push((
                event.conn_id().to_string().parse().unwrap(),
                event.authority().as_str().to_string(),
            ));
        }
        fn on_reused(&self, _event: &ConnectionReusedEvent) {}
        fn on_closed(&self, event: &ConnectionClosedEvent) {
            self.closed.lock().unwrap().push((
                event.conn_id().to_string().parse().unwrap(),
                event.authority().as_str().to_string(),
                event.reason(),
            ));
        }
        fn on_connection_failed(&self, _event: &ConnectionFailedEvent) {}
    }

    // One endpoint, two connections total: P1's (later reclaimed) and P0's.
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p1",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p0",
                },
            ],
        )
        .build()
        .await;

    let listener = Arc::new(RecordingListener::default());

    // Two partitions sharing one NIC group (so they are reclaim peers),
    // global cap of 1 (so the second partition is cap-bound), and a long
    // idle timeout so passive eviction never fires during the test.
    let p0 = Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current())
        .interface("eth-test");
    let p1 = Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current())
        .interface("eth-test");
    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .connection_event_listener(listener.clone() as Arc<dyn ConnectionEventListener>)
        .max_connections(1)
        .pool_idle_timeout(Duration::from_secs(3600))
        .partitions([p0, p1])
        .build_http();

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let client0 = PoolClient::from_partition(&pool, PartitionId::from_index(0)).into_shared();
    let client1 = PoolClient::from_partition(&pool, PartitionId::from_index(1)).into_shared();

    // P1 establishes a connection and returns it idle to its cache, holding
    // the single global permit.
    let (_, _, meta1) = send_with_capture(&client1, &url).await;
    let p1_conn_id: u64 = meta1
        .expect("p1 metadata")
        .connection_id()
        .expect("p1 conn id")
        .to_string()
        .parse()
        .unwrap();

    // P0 requests the same authority. The global permit is held by P1's
    // idle connection, so P0 is cap-bound; it reclaims P1's idle inline,
    // freeing the permit, then connects locally. Bounded so a hang (failed
    // reclaim → indefinite block) surfaces as a test timeout rather than
    // a stall.
    let started = std::time::Instant::now();
    let (status, body, meta0) =
        tokio::time::timeout(Duration::from_secs(5), send_with_capture(&client0, &url))
            .await
            .expect("p0 must not block indefinitely — reclaim should free a permit");
    assert_eq!(status, 200);
    assert_eq!(body, b"p0");
    let p0_conn_id: u64 = meta0
        .expect("p0 metadata")
        .connection_id()
        .expect("p0 conn id")
        .to_string()
        .parse()
        .unwrap();

    // P0 completed promptly — far under the 3600s idle timeout, so this was
    // active reclaim, not passive eviction.
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "p0 should complete promptly via reclaim, took {:?}",
        started.elapsed()
    );

    // P0 ran on a *fresh local* connection, not P1's (that is reclaim, not
    // borrow — borrow would reuse P1's exact connection).
    assert_ne!(
        p0_conn_id, p1_conn_id,
        "reclaim gives P0 its own fresh connection, distinct from P1's"
    );

    // The server accepted two connections: P1's (reclaimed) and P0's (new).
    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "expected P1's connection + P0's fresh connection"
    );

    // The listener proves the mechanism directly: P1's connection closed
    // with `Reclaimed` (not `IdleTimeout` — the tick never ran), and P0's
    // connection was created fresh.
    let closed = listener.closed.lock().unwrap();
    let reclaimed: Vec<_> = closed
        .iter()
        .filter(|(_, _, reason)| matches!(reason, CloseReason::Reclaimed))
        .collect();
    assert_eq!(
        reclaimed.len(),
        1,
        "exactly one connection should close with Reclaimed, got: {closed:?}"
    );
    assert_eq!(
        reclaimed[0].0, p1_conn_id,
        "the reclaimed connection must be P1's"
    );
    assert!(
        closed
            .iter()
            .all(|(_, _, reason)| !matches!(reason, CloseReason::IdleTimeout)),
        "no IdleTimeout close — the long idle timeout means reclaim, not passive eviction"
    );

    let created = listener.created.lock().unwrap();
    assert!(
        created.iter().any(|(id, _)| *id == p0_conn_id),
        "P0's fresh connection should fire on_created"
    );
}

// ---------------------------------------------------------------------------
// Test implementations: cross-partition borrow (PreferLocal policy)
// ---------------------------------------------------------------------------

/// Under cap pressure with `PreferLocal`, a starved partition borrows a
/// same-NIC peer's idle connection and dispatches its request through it —
/// no new connection, no permit. This is the opposite disposition from
/// reclaim: the connection stays the peer's (P0 runs on P1's exact
/// connection), proven by `p0_conn_id == p1_conn_id` and a single TCP
/// accept (P1's connection serves both requests via keep-alive).
#[tokio::test]
async fn v2_cross_partition_borrow_reuses_peer_connection() {
    use aws_smithy_http_client::pool::{
        CrossPartitionPolicy, Partition, PartitionId, TokioDriverSpawner,
    };

    // One endpoint, ONE connection serving TWO requests (P1's, then P0's
    // borrowed request via the keep-alive loop).
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p1",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p0",
                },
            ],
        )
        .build()
        .await;

    // Two partitions, same NIC group (borrow peers), global cap 1, long
    // idle timeout so the eviction tick never interferes.
    let p0 = Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current())
        .interface("eth-test");
    let p1 = Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current())
        .interface("eth-test");
    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .cross_partition_policy(CrossPartitionPolicy::PreferLocal)
        .max_connections(1)
        .pool_idle_timeout(Duration::from_secs(3600))
        .partitions([p0, p1])
        .build_http();

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let client0 = PoolClient::from_partition(&pool, PartitionId::from_index(0)).into_shared();
    let client1 = PoolClient::from_partition(&pool, PartitionId::from_index(1)).into_shared();

    // P1 establishes a connection, drains it, and returns it idle to its
    // cache holding the single global permit.
    let (_, _, meta1) = send_with_capture(&client1, &url).await;
    let p1_conn_id: u64 = meta1
        .expect("p1 metadata")
        .connection_id()
        .expect("p1 conn id")
        .to_string()
        .parse()
        .unwrap();

    // P0 requests the same authority. The permit is held by P1's idle
    // connection, so P0 is cap-bound; under PreferLocal it borrows P1's
    // connection and dispatches through it. Bounded so a hang surfaces as
    // a failure.
    let (status, body, meta0) =
        tokio::time::timeout(Duration::from_secs(5), send_with_capture(&client0, &url))
            .await
            .expect("p0 must not block — borrow should reuse P1's connection");
    assert_eq!(status, 200);
    assert_eq!(body, b"p0");
    let p0_conn_id: u64 = meta0
        .expect("p0 metadata")
        .connection_id()
        .expect("p0 conn id")
        .to_string()
        .parse()
        .unwrap();

    // The direct proof of borrow: P0 ran on P1's *exact* connection.
    assert_eq!(
        p0_conn_id, p1_conn_id,
        "PreferLocal borrow dispatches P0's request through P1's connection"
    );

    // One TCP accept total — P1's connection served both requests. A fresh
    // local connection for P0 (reclaim, or no borrow) would be 2.
    assert_eq!(
        harness.tcp_accepted_count(),
        1,
        "borrow reuses the peer's connection; no new connection is opened"
    );
}

/// Borrow is NIC-bounded: a peer on a different NIC is not a borrow
/// candidate. With `PreferLocal` but P0 and P1 on different NICs, P0
/// cannot borrow P1's connection — nor reclaim its permit (reclaim
/// candidates are also drawn from the NIC group). P0's cap-bound wait is
/// instead released when P1's idle connection is evicted (scenario B), and
/// P0 then connects locally. Proven by `p0_conn_id != p1_conn_id` and two
/// TCP accepts — the opposite of the same-NIC borrow case, which reuses
/// P1's exact connection.
#[tokio::test]
async fn v2_cross_partition_borrow_respects_nic_boundary() {
    use aws_smithy_http_client::pool::{
        CrossPartitionPolicy, Partition, PartitionId, TokioDriverSpawner,
    };

    // One endpoint, two connections: P1's, then P0's fresh local one.
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p1",
                },
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"p0",
                },
            ],
        )
        .build()
        .await;

    // Two partitions on DIFFERENT NICs — not borrow peers (and not reclaim
    // peers). A short idle timeout lets P0's cap-bound wait be released by
    // eviction of P1's idle connection, rather than hanging.
    let p0 = Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current())
        .interface("eth-zero");
    let p1 = Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current())
        .interface("eth-one");
    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .cross_partition_policy(CrossPartitionPolicy::PreferLocal)
        .max_connections(1)
        .pool_idle_timeout(Duration::from_millis(150))
        .partitions([p0, p1])
        .build_http();

    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());
    let client0 = PoolClient::from_partition(&pool, PartitionId::from_index(0)).into_shared();
    let client1 = PoolClient::from_partition(&pool, PartitionId::from_index(1)).into_shared();

    let (_, _, meta1) = send_with_capture(&client1, &url).await;
    let p1_conn_id: u64 = meta1
        .expect("p1 metadata")
        .connection_id()
        .expect("p1 conn id")
        .to_string()
        .parse()
        .unwrap();

    // P0 is cap-bound and cannot borrow across the NIC boundary; it waits
    // for P1's idle to be evicted, then connects locally. Bounded so a true
    // hang still surfaces as a failure.
    let (status, body, meta0) =
        tokio::time::timeout(Duration::from_secs(5), send_with_capture(&client0, &url))
            .await
            .expect("p0 should proceed once P1's idle is evicted");
    assert_eq!(status, 200);
    assert_eq!(body, b"p0");
    let p0_conn_id: u64 = meta0
        .expect("p0 metadata")
        .connection_id()
        .expect("p0 conn id")
        .to_string()
        .parse()
        .unwrap();

    // P1 is on a different NIC → not a borrow candidate. P0 does NOT run on
    // P1's connection; it gets its own.
    assert_ne!(
        p0_conn_id, p1_conn_id,
        "no borrow across NICs — P0 runs on its own connection"
    );

    // Two TCP accepts: P1's connection plus P0's fresh local one.
    assert_eq!(
        harness.tcp_accepted_count(),
        2,
        "NIC boundary blocks borrow; P0 opens its own connection"
    );
}

// ---------------------------------------------------------------------------
// Test implementations: cross-partition concurrency stress (TSan target)
// ---------------------------------------------------------------------------

/// Drives concurrent cross-partition borrow, reclaim, and eviction against
/// a shared authority under a binding cap. The assertion is intentionally
/// weak (every request completes); the value is the interleaving:
/// concurrent `try_borrow_on` / `try_reclaim_on` touch a peer's
/// `authorities` and cache locks while that peer serves its own requests
/// and the eviction task runs `retain` on the same caches. `additional-ci`
/// additionally runs it under ThreadSanitizer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn v2_cross_partition_concurrency_stress() {
    use aws_smithy_http_client::pool::{
        CrossPartitionPolicy, Partition, PartitionId, TokioDriverSpawner,
    };

    const PARTITIONS: usize = 4;
    const ROUNDS: usize = 8;
    const REQUESTS_PER_ROUND: usize = 16;

    // Single loopback endpoint serving many keep-alive requests. The
    // cross-partition contention comes from multiple partitions sharing
    // one authority under a binding cap, not from IP spread.
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            (0..512)
                .map(|_| ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .build()
        .await;

    // No NIC binding (the common case): all partitions land in the single
    // implicit NIC group, so they are borrow + reclaim peers. A small
    // global cap forces cross-partition contention; a short idle timeout
    // makes the eviction task churn concurrently with borrow/reclaim.
    let parts = (0..PARTITIONS)
        .map(|i| Partition::new(PartitionId::from_index(i), TokioDriverSpawner::current()));
    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .cross_partition_policy(CrossPartitionPolicy::PreferLocal)
        .max_connections(PARTITIONS) // bind the cap below the offered load
        .pool_idle_timeout(Duration::from_millis(20))
        .partitions(parts)
        .build_http();

    let clients: Vec<SharedHttpClient> = (0..PARTITIONS)
        .map(|i| PoolClient::from_partition(&pool, PartitionId::from_index(i)).into_shared())
        .collect();
    let url = format!("http://127.0.0.1:{}/", harness.endpoints[0].port());

    for _ in 0..ROUNDS {
        let mut tasks = tokio::task::JoinSet::new();
        for r in 0..REQUESTS_PER_ROUND {
            // Spread requests across partitions so borrow/reclaim peers
            // are all live at once.
            let client = clients[r % PARTITIONS].clone();
            let url = url.clone();
            tasks.spawn(async move { send_and_read_body(&client, &url).await });
        }
        while let Some(result) = tasks.join_next().await {
            let (status, _) = result
                .expect("task should not panic")
                .expect("request should succeed under cross-partition contention");
            assert_eq!(status, 200);
        }
        // Let the eviction tick fire between rounds so the next round
        // races fresh connects against reclaim/borrow on partly-evicted
        // caches.
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

// ---------------------------------------------------------------------------
// Test implementations: stats read API
// ---------------------------------------------------------------------------

/// SharedPool::stats returns sparse per-partition snapshots: only partitions
/// that have touched an authority appear. After a request completes and the
/// body is consumed, counters reflect the idle state.
#[tokio::test]
async fn v2_stats_reports_per_partition_sparse() {
    use aws_smithy_http_client::pool::{Authority, Partition, PartitionId, TokioDriverSpawner};

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"stats-test",
            }],
        )
        .build()
        .await;

    let p0 = Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current());
    let p1 = Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current());

    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .partitions([p0, p1])
        .build_http();

    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");
    let authority = Authority::from_host(format!("127.0.0.1:{port}"));

    // Before any request, stats are empty for this authority.
    let stats = pool.stats(&authority);
    assert!(
        stats.is_empty(),
        "no partition should have touched this authority yet"
    );

    // Send a request on partition 0, consume the body so the connection idles.
    let client0 = PoolClient::from_partition(&pool, PartitionId::from_index(0)).into_shared();
    let (status, body) = send_and_read_body(&client0, &url)
        .await
        .expect("p0 request should succeed");
    assert_eq!(status, 200);
    assert_eq!(body, b"stats-test");

    // After the request idles, partition 0 should appear, partition 1 should not.
    let stats = pool.stats(&authority);
    assert_eq!(stats.len(), 1, "only partition 0 should appear (sparse)");

    let p0_stats = stats
        .get(PartitionId::from_index(0))
        .expect("partition 0 should have stats");
    assert_eq!(p0_stats.established, 1, "one connection established");
    assert_eq!(p0_stats.establishing, 0, "no handshakes in flight");
    assert_eq!(p0_stats.active, 0, "connection is idle after body consumed");
    assert_eq!(p0_stats.idle(), 1, "one idle connection");
    // H1 cell: capacity_hint is Some(idle)
    assert_eq!(p0_stats.capacity_hint(), Some(1));

    assert!(
        stats.get(PartitionId::from_index(1)).is_none(),
        "partition 1 has not touched this authority"
    );
}

/// `active` tracks an in-flight request end-to-end. The H1 checkout guard
/// rides the response body (`GuardedBody`): it is held while the response
/// value is alive and releases when the body is consumed/dropped, returning
/// the connection to the pool. So `active == 1` is observable for as long as
/// the caller holds the response, and drops to 0 once the body is drained.
#[tokio::test]
async fn v2_stats_active_tracks_in_flight_request() {
    use aws_smithy_http_client::pool::{Authority, PartitionId};
    use http_body_util::BodyExt;

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![ConnectionBehavior::RespondKeepAlive {
                status: 200,
                body: b"in-flight",
            }],
        )
        .build()
        .await;

    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .build_http();

    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");
    let authority = Authority::from_host(format!("127.0.0.1:{port}"));
    let partition = PartitionId::default();

    let client = PoolClient::new(&pool).into_shared();

    // Issue the request but hold the response without draining the body. The
    // connection is checked out: its guard is alive inside `resp`'s body.
    let resp = send_to(&client, &url)
        .await
        .expect("request should succeed");
    assert_eq!(resp.status().as_u16(), 200);

    let stats = pool.stats(&authority);
    let in_flight = stats
        .get(partition)
        .expect("partition should have touched this authority");
    assert_eq!(in_flight.established, 1, "one connection established");
    assert_eq!(
        in_flight.active, 1,
        "request is in flight, connection checked out"
    );
    assert_eq!(in_flight.idle(), 0, "no idle connection while in flight");

    // Drain the body: the GuardedBody drops, CachedConnection::Drop fires,
    // active decrements and the connection returns to the pool as idle.
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should be readable")
        .to_bytes()
        .to_vec();
    assert_eq!(body, b"in-flight");

    let stats = pool.stats(&authority);
    let idle = stats
        .get(partition)
        .expect("partition still present after request completes");
    assert_eq!(idle.established, 1, "connection still established (idle)");
    assert_eq!(idle.active, 0, "no in-flight request after body drained");
    assert_eq!(idle.idle(), 1, "connection is idle and reusable");
}

/// Eviction decrements `established` and prunes the stats-index cell through
/// its real trigger — the background eviction task — not a direct prune call.
/// After an idle connection is evicted, the host entry is removed and the
/// eviction-triggered prune drops the now-dead cell from the index, so
/// `stats()` reports the authority as untracked.
#[tokio::test]
async fn v2_stats_pruned_after_eviction() {
    use aws_smithy_http_client::pool::{Authority, PartitionId};

    let idle_timeout = Duration::from_millis(100);

    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            vec![
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"evict-me",
                },
                // A second connection is available if eviction forces a reconnect;
                // the test asserts on stats, not connection count.
                ConnectionBehavior::RespondKeepAlive {
                    status: 200,
                    body: b"evict-me",
                },
            ],
        )
        .build()
        .await;

    let pool = SharedPool::builder()
        .dns_resolver(harness.dns_resolver())
        .pool_idle_timeout(idle_timeout)
        .build_http();

    let port = harness.endpoints[0].port();
    let url = format!("http://127.0.0.1:{port}/");
    let authority = Authority::from_host(format!("127.0.0.1:{port}"));

    let client = PoolClient::new(&pool).into_shared();

    // Request completes and the connection idles. This also lazily spawns the
    // eviction task (pool_idle_timeout is set).
    let (status, _) = send_and_read_body(&client, &url)
        .await
        .expect("request should succeed");
    assert_eq!(status, 200);

    let stats = pool.stats(&authority);
    assert_eq!(
        stats
            .get(PartitionId::default())
            .expect("partition present after request")
            .established,
        1,
        "one established idle connection before eviction"
    );

    // Wait past the idle timeout: the eviction task drops the idle connection,
    // removes the host entry, and prunes the now-dead index cell.
    tokio::time::sleep(idle_timeout * 3).await;

    let stats = pool.stats(&authority);
    assert!(
        stats.is_empty(),
        "eviction should have decremented established and pruned the cell"
    );
}

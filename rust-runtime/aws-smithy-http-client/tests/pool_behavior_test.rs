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

    let resp = send_to(&client, &url)
        .await
        .expect("first request should succeed");
    assert_eq!(resp.status().as_u16(), 200);

    tokio::time::sleep(idle_timeout * 2).await;

    let resp = send_to(&client, &url)
        .await
        .expect("second request should succeed after idle eviction");
    assert_eq!(resp.status().as_u16(), 200);

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
    idle_timeout_eviction(&V2Client).await;
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
            vec![ConnectionBehavior::Respond {
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
                .map(|_| ConnectionBehavior::Respond {
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
                .map(|_| ConnectionBehavior::Respond {
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
                .map(|_| ConnectionBehavior::Respond {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .endpoint(
            IP2,
            (0..10)
                .map(|_| ConnectionBehavior::Respond {
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
                .map(|_| ConnectionBehavior::Respond {
                    status: 200,
                    body: b"ok",
                })
                .collect(),
        )
        .endpoint(
            IP2,
            (0..10)
                .map(|_| ConnectionBehavior::Respond {
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

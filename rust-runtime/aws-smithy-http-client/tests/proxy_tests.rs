/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Integration tests for proxy functionality
//!
//! These tests verify that proxy configuration works end-to-end with real HTTP requests
//! using mock proxy servers. This follows the testing strategy outlined in the design docs.
#![cfg(feature = "default-client")]

use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http_client::{proxy::ProxyConfig, Connector};
use aws_smithy_runtime_api::client::http::{
    http_client_fn, HttpClient, HttpConnector, HttpConnectorSettings,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use base64::Engine;
use http_1x::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

// ================================================================================================
// Test Utilities (Mock Proxy Server)
// ================================================================================================

/// Mock HTTP server that acts as a proxy endpoint for testing
#[derive(Debug)]
struct MockProxyServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    request_log: Arc<Mutex<Vec<RecordedRequest>>>,
}

/// A recorded request received by the mock proxy server
#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    uri: String,
    headers: HashMap<String, String>,
}

impl MockProxyServer {
    /// Create a new mock proxy server with a custom request handler
    async fn new<F>(handler: F) -> Self
    where
        F: Fn(RecordedRequest) -> Response<String> + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let request_log = Arc::new(Mutex::new(Vec::new()));
        let request_log_clone = request_log.clone();

        let handler = Arc::new(handler);

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let handler = handler.clone();
                                let request_log = request_log_clone.clone();

                                tokio::spawn(async move {
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let handler = handler.clone();
                                        let request_log = request_log.clone();

                                        async move {
                                            // Record the request
                                            let recorded = RecordedRequest {
                                                method: req.method().to_string(),
                                                uri: req.uri().to_string(),
                                                headers: req.headers().iter()
                                                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                                                    .collect(),
                                            };

                                            request_log.lock().unwrap().push(recorded.clone());

                                            // Call the handler
                                            let response = handler(recorded);

                                            // Convert to hyper response
                                            let (parts, body) = response.into_parts();
                                            let hyper_response = Response::from_parts(parts, body);

                                            Ok::<_, Infallible>(hyper_response)
                                        }
                                    });

                                    if let Err(err) = hyper::server::conn::http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        eprintln!("Mock proxy server connection error: {}", err);
                                    }
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            request_log,
        }
    }

    /// Create a simple mock proxy that returns a fixed response
    async fn with_response(status: StatusCode, body: &str) -> Self {
        let body = body.to_string();
        Self::new(move |_req| {
            Response::builder()
                .status(status)
                .body(body.clone())
                .unwrap()
        })
        .await
    }

    /// Create a mock proxy that validates basic authentication
    async fn with_auth_validation(expected_user: &str, expected_pass: &str) -> Self {
        let expected_auth = format!(
            "Basic {}",
            base64::prelude::BASE64_STANDARD.encode(format!("{}:{}", expected_user, expected_pass))
        );

        Self::new(move |req| {
            if let Some(auth_header) = req.headers.get("proxy-authorization") {
                if auth_header == &expected_auth {
                    Response::builder()
                        .status(StatusCode::OK)
                        .body("authenticated".to_string())
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
                        .body("invalid credentials".to_string())
                        .unwrap()
                }
            } else {
                Response::builder()
                    .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
                    .header("proxy-authenticate", "Basic realm=\"proxy\"")
                    .body("authentication required".to_string())
                    .unwrap()
            }
        })
        .await
    }

    /// Get the address this server is listening on
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get all requests received by this server
    fn requests(&self) -> Vec<RecordedRequest> {
        self.request_log.lock().unwrap().clone()
    }
}

impl Drop for MockProxyServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Utility for running tests with specific environment variables
async fn with_env_vars<F, Fut, R>(vars: &[(&str, &str)], test: F) -> R
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = R>,
{
    // Use a static mutex to serialize environment variable tests
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_MUTEX.lock().unwrap();

    // Save original environment
    let original_vars: Vec<_> = vars
        .iter()
        .map(|(key, _)| (*key, std::env::var(key)))
        .collect();

    // Set test environment variables
    for (key, value) in vars {
        std::env::set_var(key, value);
    }

    // Run the test
    let result = test().await;

    // Restore original environment
    for (key, original_value) in original_vars {
        match original_value {
            Ok(val) => std::env::set_var(key, val),
            Err(_) => std::env::remove_var(key),
        }
    }

    result
}

/// Helper function to make HTTP requests through a proxy-configured connector
async fn make_http_request_through_proxy(
    proxy_config: ProxyConfig,
    target_url: &str,
) -> Result<(StatusCode, String), Box<dyn std::error::Error + Send + Sync>> {
    // Create an HttpClient using http_client_fn with proxy-configured connector
    let http_client = http_client_fn(move |settings, _components| {
        let connector = Connector::builder()
            .proxy_config(proxy_config.clone())
            .connector_settings(settings.clone())
            .build_http();

        aws_smithy_runtime_api::client::http::SharedHttpConnector::new(connector)
    });

    // Set up runtime components (following smoke_test_client pattern)
    let connector_settings = HttpConnectorSettings::builder().build();
    let runtime_components = RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(SystemTimeSource::new()))
        .build()
        .unwrap();

    // Get the HTTP connector from the client
    let http_connector = http_client.http_connector(&connector_settings, &runtime_components);

    // Create and make the HTTP request
    let request = HttpRequest::get(target_url)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    let response = http_connector.call(request).await?;

    // Extract status and body
    let status = response.status();
    let body_bytes = response.into_body().collect().await?.to_bytes();
    let body_string = String::from_utf8(body_bytes.to_vec())?;

    Ok((status.into(), body_string))
}

// ================================================================================================
// Integration Tests
// ================================================================================================

#[tokio::test]
async fn test_http_proxy_basic_request() {
    // Create a mock proxy server that validates the request was routed through it
    let mock_proxy = MockProxyServer::new(|req| {
        // Validate that this looks like a proxy request
        assert_eq!(req.method, "GET");
        // For HTTP proxy, the URI should be the full target URL
        assert_eq!(req.uri, "http://aws.amazon.com/api/data");

        // Return a successful response that we can identify
        Response::builder()
            .status(StatusCode::OK)
            .body("proxied response from mock server".to_string())
            .unwrap()
    })
    .await;

    // Configure connector with HTTP proxy
    let proxy_config = ProxyConfig::http(format!("http://{}", mock_proxy.addr())).unwrap();

    // Make an HTTP request through the proxy - use safe domain
    let target_url = "http://aws.amazon.com/api/data";
    let result = make_http_request_through_proxy(proxy_config, target_url).await;

    let (status, body) = result.expect("HTTP request through proxy should succeed");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "proxied response from mock server");

    // Verify the mock proxy received the expected request
    let requests = mock_proxy.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].uri, target_url);
}

#[tokio::test]
async fn test_proxy_authentication() {
    // Create a mock proxy that requires authentication
    let mock_proxy = MockProxyServer::with_auth_validation("testuser", "testpass").await;

    // Configure connector with authenticated proxy
    let proxy_config = ProxyConfig::http(format!("http://{}", mock_proxy.addr()))
        .unwrap()
        .with_basic_auth("testuser", "testpass");

    // Make request through authenticated proxy - use safe domain
    let target_url = "http://aws.amazon.com/protected/resource";
    let result = make_http_request_through_proxy(proxy_config, target_url).await;

    let (status, body) = result.expect("Authenticated proxy request should succeed");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "authenticated");

    // Verify the proxy received the request with correct auth
    let requests = mock_proxy.requests();
    assert_eq!(requests.len(), 1);

    let expected_auth = format!(
        "Basic {}",
        base64::prelude::BASE64_STANDARD.encode("testuser:testpass")
    );
    assert_eq!(
        requests[0].headers.get("proxy-authorization"),
        Some(&expected_auth)
    );
}

#[tokio::test]
async fn test_proxy_from_environment_variables() {
    let mock_proxy = MockProxyServer::with_response(StatusCode::OK, "env proxy response").await;

    with_env_vars(
        &[
            ("HTTP_PROXY", &format!("http://{}", mock_proxy.addr())),
            ("NO_PROXY", "localhost,127.0.0.1"),
        ],
        || async {
            // Create connector with environment-based proxy config
            let proxy_config = ProxyConfig::from_env();

            // Make request through environment-configured proxy
            let target_url = "http://aws.amazon.com/v1/data";
            let result = make_http_request_through_proxy(proxy_config, target_url).await;

            let (status, body) = result.expect("Environment proxy request should succeed");

            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "env proxy response");

            // Verify the proxy received the request
            let requests = mock_proxy.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].uri, target_url);
        },
    )
    .await;
}

/// Tests that NO_PROXY bypass rules work correctly
/// Verifies that requests to bypassed hosts do not go through the proxy
#[tokio::test]
async fn test_no_proxy_bypass_rules() {
    let mock_proxy = MockProxyServer::with_response(StatusCode::OK, "should not reach here").await;

    // Create a second mock server that will act as the "direct" target
    let direct_server = MockProxyServer::with_response(StatusCode::OK, "direct connection").await;

    // Configure proxy with NO_PROXY rules that include the direct server's address
    // Use just the IP address for the NO_PROXY rule
    let direct_ip = "127.0.0.1";
    let proxy_config = ProxyConfig::http(format!("http://{}", mock_proxy.addr()))
        .unwrap()
        .no_proxy(direct_ip);

    // Make request to the direct server (should bypass proxy due to NO_PROXY rule)
    let result = make_http_request_through_proxy(
        proxy_config,
        &format!("http://{}/test", direct_server.addr()),
    )
    .await;

    let (status, body) = result.expect("Direct connection should succeed");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "direct connection");

    // Verify the mock proxy received no requests (bypassed)
    let proxy_requests = mock_proxy.requests();
    assert_eq!(
        proxy_requests.len(),
        0,
        "Proxy should not have received any requests due to NO_PROXY bypass"
    );

    // Verify the direct server received the request
    let direct_requests = direct_server.requests();
    assert_eq!(
        direct_requests.len(),
        1,
        "Direct server should have received the request"
    );
}

/// Tests that disabled proxy configuration results in direct connections
/// Verifies that ProxyConfig::disabled() bypasses all proxy logic
#[tokio::test]
async fn test_proxy_disabled() {
    // Create a direct target server
    let direct_server = MockProxyServer::with_response(StatusCode::OK, "direct connection").await;

    // Create a disabled proxy configuration
    let proxy_config = ProxyConfig::disabled();

    // Make request with disabled proxy (should go direct to our mock server)
    let result = make_http_request_through_proxy(
        proxy_config,
        &format!("http://{}/get", direct_server.addr()),
    )
    .await;

    let (status, body) = result.expect("Direct connection should succeed");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "direct connection");

    // Verify the direct server received the request
    let requests = direct_server.requests();
    assert_eq!(
        requests.len(),
        1,
        "Direct server should have received the request"
    );
    assert_eq!(requests[0].method, "GET");
    // For direct connections, the URI might be just the path part
    assert!(
        requests[0].uri == format!("http://{}/get", direct_server.addr())
            || requests[0].uri == "/get",
        "URI should be either full URL or path, got: {}",
        requests[0].uri
    );
}

/// Tests HTTPS-only proxy configuration
/// Verifies that HTTP requests bypass HTTPS-only proxies
#[tokio::test]
async fn test_https_proxy_configuration() {
    let mock_proxy = MockProxyServer::with_response(StatusCode::OK, "https proxy response").await;

    // Create a direct target server for HTTP requests
    let direct_server =
        MockProxyServer::with_response(StatusCode::OK, "direct http connection").await;

    // Configure HTTPS-only proxy
    let proxy_config = ProxyConfig::https(format!("http://{}", mock_proxy.addr())).unwrap();

    // Test: HTTP request should NOT go through HTTPS-only proxy, should go direct
    let target_url = format!("http://{}/api", direct_server.addr());
    let result = make_http_request_through_proxy(proxy_config.clone(), &target_url).await;

    // The HTTP request should succeed by going directly to our mock server
    let (status, body) = result.expect("HTTP request should succeed via direct connection");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "direct http connection");

    // Verify the HTTPS-only proxy received no requests
    let proxy_requests = mock_proxy.requests();
    assert_eq!(
        proxy_requests.len(),
        0,
        "HTTP request should not go through HTTPS-only proxy"
    );

    // Verify the direct server received the request
    let direct_requests = direct_server.requests();
    assert_eq!(
        direct_requests.len(),
        1,
        "Direct server should have received the HTTP request"
    );

    // Test 2: For HTTPS, we can't easily test with our current HTTP-only infrastructure
    // since HTTPS would require CONNECT tunneling. The important thing is that HTTP
    // requests don't go through an HTTPS-only proxy, which we've verified above.
}

/// Tests all-traffic proxy configuration
/// Verifies that both HTTP and HTTPS requests go through all-traffic proxies
#[tokio::test]
async fn test_all_traffic_proxy() {
    let mock_proxy = MockProxyServer::with_response(StatusCode::OK, "all traffic proxy").await;

    // Configure proxy for all traffic
    let proxy_config = ProxyConfig::all(format!("http://{}", mock_proxy.addr())).unwrap();

    // Test 1: HTTP request should go through the proxy
    let target_url = "http://aws.amazon.com/api/endpoint";
    let result = make_http_request_through_proxy(proxy_config.clone(), target_url).await;

    let (status, body) = result.expect("HTTP request through all-traffic proxy should succeed");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "all traffic proxy");

    // Verify the proxy received the HTTP request
    let requests = mock_proxy.requests();
    assert_eq!(
        requests.len(),
        1,
        "Proxy should have received exactly one request"
    );
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].uri, target_url);

    // Test 2: The important behavior is that HTTP requests DO go through all-traffic proxy
    // We've already verified this above. For HTTPS, we can't easily test with our current
    // HTTP-only infrastructure, but the "all" configuration should handle both HTTP and HTTPS.
}

// ================================================================================================
// Error Handling Tests
// ================================================================================================

/// Tests proxy connection failure handling
/// Verifies that unreachable proxy servers result in appropriate connection errors
#[tokio::test]
async fn test_proxy_connection_failure() {
    // Configure proxy pointing to non-existent server
    let proxy_config = ProxyConfig::http("http://127.0.0.1:1").unwrap(); // Port 1 should be unavailable

    // Make request through non-existent proxy - use a safe domain that won't cause issues
    let target_url = "http://aws.amazon.com/api/test";
    let result = make_http_request_through_proxy(proxy_config, target_url).await;

    // The request should fail with a connection error
    assert!(
        result.is_err(),
        "Request should fail when proxy is unreachable"
    );

    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();

    // Verify it's a connection-related error (not a different kind of error)
    assert!(
        error_msg.contains("connection")
            || error_msg.contains("refused")
            || error_msg.contains("unreachable")
            || error_msg.contains("timeout")
            || error_msg.contains("connect")
            || error_msg.contains("io error"), // Include generic IO errors
        "Error should be connection-related, got: {}",
        error
    );
}

/// Tests proxy authentication failure handling
/// Verifies that incorrect proxy credentials result in 407 Proxy Authentication Required
#[tokio::test]
async fn test_proxy_authentication_failure() {
    let mock_proxy = MockProxyServer::with_auth_validation("correct", "password").await;

    // Configure proxy with wrong credentials
    let proxy_config = ProxyConfig::http(format!("http://{}", mock_proxy.addr()))
        .unwrap()
        .with_basic_auth("wrong", "credentials");

    // Make request with wrong credentials - use safe domain
    let target_url = "http://aws.amazon.com/secure/api";
    let result = make_http_request_through_proxy(proxy_config, target_url).await;

    // The request should return 407 Proxy Authentication Required
    let (status, _body) = result.expect("Request should complete (even with auth failure)");
    assert_eq!(status, StatusCode::PROXY_AUTHENTICATION_REQUIRED);

    // Verify the proxy received the request (even though auth failed)
    let requests = mock_proxy.requests();
    assert_eq!(requests.len(), 1, "Proxy should have received the request");

    // Verify the wrong credentials were sent
    let expected_wrong_auth = format!(
        "Basic {}",
        base64::prelude::BASE64_STANDARD.encode("wrong:credentials")
    );
    assert_eq!(
        requests[0].headers.get("proxy-authorization"),
        Some(&expected_wrong_auth)
    );
}

// ================================================================================================
// Mock Server Tests (Test the Test Infrastructure)
// ================================================================================================

#[tokio::test]
async fn test_mock_proxy_server_basic() {
    let server = MockProxyServer::with_response(StatusCode::OK, "test response").await;

    // The server should be listening
    assert!(server.addr().port() > 0);

    // Make a simple HTTP request to verify the server works
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let uri: http_1x::Uri = format!("http://{}/test", server.addr()).parse().unwrap();
    let request = Request::builder().uri(uri).body(String::new()).unwrap();

    let response = timeout(Duration::from_secs(5), client.request(request))
        .await
        .expect("Request should complete within timeout")
        .expect("Request should succeed");

    assert_eq!(response.status(), StatusCode::OK);

    // Verify request was logged
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].uri, "/test");
}

#[tokio::test]
async fn test_mock_proxy_auth_validation() {
    let server = MockProxyServer::with_auth_validation("user", "pass").await;
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    // Test without authentication - should get 407
    let uri: http_1x::Uri = format!("http://{}/test", server.addr()).parse().unwrap();
    let request = Request::builder().uri(uri).body(String::new()).unwrap();

    let response = client.request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);

    // Test with correct authentication - should get 200
    let uri: http_1x::Uri = format!("http://{}/test", server.addr()).parse().unwrap();
    let auth_header = format!(
        "Basic {}",
        base64::prelude::BASE64_STANDARD.encode("user:pass")
    );
    let request = Request::builder()
        .uri(uri)
        .header("proxy-authorization", auth_header)
        .body(String::new())
        .unwrap();

    let response = client.request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_with_env_vars_utility() {
    // Test that environment variables are properly set and restored
    let original_value = std::env::var("TEST_PROXY_VAR");

    with_env_vars(&[("TEST_PROXY_VAR", "test_value")], || async {
        assert_eq!(std::env::var("TEST_PROXY_VAR").unwrap(), "test_value");
    })
    .await;

    // Environment should be restored
    assert_eq!(std::env::var("TEST_PROXY_VAR"), original_value);
}

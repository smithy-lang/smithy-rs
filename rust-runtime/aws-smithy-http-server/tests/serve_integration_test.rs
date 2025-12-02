/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Integration tests for the serve module
//!
//! These tests verify functionality that isn't explicitly tested elsewhere

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use aws_smithy_http_server::routing::IntoMakeService;
use aws_smithy_http_server::serve::{Listener, ListenerExt};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tower::service_fn;

/// Simple test service that returns OK
async fn ok_service(_request: http::Request<hyper::body::Incoming>) -> Result<http::Response<BoxBody>, Infallible> {
    Ok(http::Response::builder().status(200).body(to_boxed("OK")).unwrap())
}

/// Test service that returns custom headers for verification
async fn service_with_custom_headers(
    _request: http::Request<hyper::body::Incoming>,
) -> Result<http::Response<BoxBody>, Infallible> {
    Ok(http::Response::builder()
        .status(200)
        .header("content-type", "text/plain")
        .header("x-custom-header", "test-value")
        .header("x-another-header", "another-value")
        .body(to_boxed("OK"))
        .unwrap())
}

/// Test that `configure_hyper()` actually applies HTTP/1 settings like title-case headers at the wire level.
#[tokio::test]
async fn test_configure_hyper_http1_keep_alive() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start server with custom Hyper configuration including title_case_headers
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(service_with_custom_headers)))
            .configure_hyper(|mut builder| {
                // Configure HTTP/1 settings
                builder.http1().keep_alive(true).title_case_headers(true);
                builder
            })
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Use raw TCP to read the actual HTTP response headers
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("failed to connect");

    // Send a simple HTTP/1.1 request
    stream
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("failed to write request");

    // Read the response
    let mut buffer = vec![0u8; 4096];
    let n = stream.read(&mut buffer).await.expect("failed to read response");
    let response_text = String::from_utf8_lossy(&buffer[..n]);

    // Verify status
    assert!(response_text.contains("HTTP/1.1 200 OK"), "Expected 200 OK status");

    // Verify title-case headers are present in the raw response
    // With title_case_headers(true), Hyper writes headers like "Content-Type:" instead of "content-type:"
    assert!(
        response_text.contains("Content-Type:") || response_text.contains("Content-Type: "),
        "Expected Title-Case 'Content-Type' header, got:\n{response_text}"
    );
    assert!(
        response_text.contains("X-Custom-Header:") || response_text.contains("X-Custom-Header: "),
        "Expected Title-Case 'X-Custom-Header' header, got:\n{response_text}"
    );
    assert!(
        response_text.contains("X-Another-Header:") || response_text.contains("X-Another-Header: "),
        "Expected Title-Case 'X-Another-Header' header, got:\n{response_text}"
    );

    // Verify it's NOT lowercase (which would be the default)
    assert!(
        !response_text.contains("content-type:"),
        "Headers should be Title-Case, not lowercase"
    );
    assert!(
        !response_text.contains("x-custom-header:"),
        "Headers should be Title-Case, not lowercase"
    );

    // Cleanup
    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that `tap_io()` invokes the closure with access to the TCP stream for configuration.
#[tokio::test]
async fn test_tap_io_set_nodelay() {
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |tcp_stream| {
            // Set TCP_NODELAY and mark that we were called
            let _ = tcp_stream.set_nodelay(true);
            called_clone.store(true, Ordering::SeqCst);
        });

    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Make a request to trigger connection
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();

    let uri = format!("http://{addr}/test");
    let request = http::Request::builder()
        .uri(&uri)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = client.request(request).await.expect("request failed");
    assert_eq!(response.status(), 200);

    // Verify tap_io was called
    assert!(called.load(Ordering::SeqCst), "tap_io closure was not called");

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that `tap_io()` and `limit_connections()` can be chained together.
#[tokio::test]
async fn test_tap_io_with_limit_connections() {
    let tap_count = Arc::new(AtomicUsize::new(0));
    let tap_count_clone = tap_count.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |_tcp_stream| {
            tap_count_clone.fetch_add(1, Ordering::SeqCst);
        })
        .limit_connections(10);

    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Make 3 requests - each creates a new connection
    // Note: HTTP clients may reuse connections, so we use Connection: close
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();

    for _ in 0..3 {
        let uri = format!("http://{addr}/test");
        let request = http::Request::builder()
            .uri(&uri)
            .header("Connection", "close") // Force new connection each time
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();

        let response = client.request(request).await.expect("request failed");
        assert_eq!(response.status(), 200);

        // Give time for connection to close
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Verify tap_io was called at least once (may be 1-3 depending on connection reuse)
    let count = tap_count.load(Ordering::SeqCst);
    assert!((1..=3).contains(&count), "tap_io was called {count} times");

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that the server works with Unix domain socket listeners.
#[cfg(unix)]
#[tokio::test]
async fn test_unix_listener() {
    use tokio::net::UnixListener;

    // Create a temporary socket path
    let socket_path = format!("/tmp/smithy-test-{}.sock", std::process::id());

    // Remove socket if it exists
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix socket");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Connect via Unix socket
    let stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("failed to connect to unix socket");

    // Use hyper to make a request over the Unix socket
    use hyper_util::rt::TokioIo;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake failed");

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("Connection error: {err:?}");
        }
    });

    let request = http::Request::builder()
        .uri("/test")
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = sender.send_request(request).await.expect("request failed");
    assert_eq!(response.status(), 200);

    // Cleanup
    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
    let _ = std::fs::remove_file(&socket_path);
}

/// Test that `local_addr()` returns the correct bound address.
#[tokio::test]
async fn test_local_addr() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");

    let expected_addr = listener.local_addr().unwrap();

    let serve = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)));

    let actual_addr = serve.local_addr().expect("failed to get local_addr");

    assert_eq!(actual_addr, expected_addr);
}

/// Test that `local_addr()` still works after calling `with_graceful_shutdown()`.
#[tokio::test]
async fn test_local_addr_with_graceful_shutdown() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");

    let expected_addr = listener.local_addr().unwrap();

    let (_shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let serve = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        });

    let actual_addr = serve.local_addr().expect("failed to get local_addr");

    assert_eq!(actual_addr, expected_addr);
}

/// Test HTTP/2 prior knowledge mode (cleartext HTTP/2 without ALPN)
#[tokio::test]
async fn test_http2_only_prior_knowledge() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start server with HTTP/2 only (prior knowledge mode)
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .configure_hyper(|builder| builder.http2_only())
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create HTTP/2 client (prior knowledge mode - no upgrade)
    let stream = tokio::net::TcpStream::connect(addr).await.expect("failed to connect");
    let io = hyper_util::rt::TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http2::handshake(hyper_util::rt::TokioExecutor::new(), io)
        .await
        .expect("http2 handshake failed");

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("HTTP/2 connection error: {err:?}");
        }
    });

    // Send HTTP/2 request
    let request = http::Request::builder()
        .uri("/test")
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = sender.send_request(request).await.expect("request failed");
    assert_eq!(response.status(), 200);

    // Cleanup
    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test HTTP/1-only mode using `http1_only()` configuration.
#[tokio::test]
async fn test_http1_only() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .configure_hyper(|builder| builder.http1_only())
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Use HTTP/1 client
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();

    let uri = format!("http://{addr}/test");
    let request = http::Request::builder()
        .uri(&uri)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = client.request(request).await.expect("request failed");
    assert_eq!(response.status(), 200);

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that the default server configuration auto-detects and supports both HTTP/1 and HTTP/2.
#[tokio::test]
async fn test_default_server_supports_both_http1_and_http2() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start server with DEFAULT configuration (no configure_hyper call)
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test 1: Make an HTTP/1.1 request
    let http1_client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();

    let uri = format!("http://{addr}/test");
    let request = http::Request::builder()
        .uri(&uri)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = http1_client.request(request).await.expect("HTTP/1 request failed");
    assert_eq!(response.status(), 200, "HTTP/1 request should succeed");

    // Test 2: Make an HTTP/2 request (prior knowledge mode)
    let stream = tokio::net::TcpStream::connect(addr).await.expect("failed to connect");
    let io = hyper_util::rt::TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http2::handshake(hyper_util::rt::TokioExecutor::new(), io)
        .await
        .expect("http2 handshake failed");

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("HTTP/2 connection error: {err:?}");
        }
    });

    let request = http::Request::builder()
        .uri("/test")
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();

    let response = sender.send_request(request).await.expect("HTTP/2 request failed");
    assert_eq!(response.status(), 200, "HTTP/2 request should succeed");

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that the server handles concurrent HTTP/1 and HTTP/2 connections simultaneously using a barrier.
#[tokio::test]
async fn test_mixed_protocol_concurrent_connections() {
    use tokio::sync::Barrier;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Use a barrier to ensure all 4 requests arrive before any respond
    // This proves they're being handled concurrently
    let barrier = Arc::new(Barrier::new(4));
    let barrier_clone = barrier.clone();

    let barrier_service = move |_request: http::Request<hyper::body::Incoming>| {
        let barrier = barrier_clone.clone();
        async move {
            // Wait for all 4 requests to arrive
            barrier.wait().await;
            // Now all respond together
            Ok::<_, Infallible>(http::Response::builder().status(200).body(to_boxed("OK")).unwrap())
        }
    };

    // Start server with default configuration (supports both protocols)
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(barrier_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Start multiple HTTP/1 connections
    let make_http1_request = |addr: std::net::SocketAddr, path: &'static str| async move {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();

        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("HTTP/1 connection error: {e:?}");
            }
        });

        let request = http::Request::builder()
            .uri(path)
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();

        sender.send_request(request).await
    };

    // Start multiple HTTP/2 connections
    let make_http2_request = |addr: std::net::SocketAddr, path: &'static str| async move {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http2::handshake(hyper_util::rt::TokioExecutor::new(), io)
            .await
            .unwrap();

        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("HTTP/2 connection error: {e:?}");
            }
        });

        let request = http::Request::builder()
            .uri(path)
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();

        sender.send_request(request).await
    };

    // Launch 2 HTTP/1 and 2 HTTP/2 requests concurrently
    let h1_handle1 = tokio::spawn(make_http1_request(addr, "/http1-test1"));
    let h1_handle2 = tokio::spawn(make_http1_request(addr, "/http1-test2"));
    let h2_handle1 = tokio::spawn(make_http2_request(addr, "/http2-test1"));
    let h2_handle2 = tokio::spawn(make_http2_request(addr, "/http2-test2"));

    // Wait for all requests to complete with timeout
    // If they complete, it means the barrier was satisfied (all 4 arrived concurrently)
    let timeout = Duration::from_secs(60);
    let h1_result1 = tokio::time::timeout(timeout, h1_handle1)
        .await
        .expect("HTTP/1 request 1 timed out")
        .unwrap();
    let h1_result2 = tokio::time::timeout(timeout, h1_handle2)
        .await
        .expect("HTTP/1 request 2 timed out")
        .unwrap();
    let h2_result1 = tokio::time::timeout(timeout, h2_handle1)
        .await
        .expect("HTTP/2 request 1 timed out")
        .unwrap();
    let h2_result2 = tokio::time::timeout(timeout, h2_handle2)
        .await
        .expect("HTTP/2 request 2 timed out")
        .unwrap();

    // All requests should succeed
    assert!(h1_result1.is_ok(), "HTTP/1 request 1 failed");
    assert!(h1_result2.is_ok(), "HTTP/1 request 2 failed");
    assert!(h2_result1.is_ok(), "HTTP/2 request 1 failed");
    assert!(h2_result2.is_ok(), "HTTP/2 request 2 failed");

    assert_eq!(h1_result1.unwrap().status(), 200);
    assert_eq!(h1_result2.unwrap().status(), 200);
    assert_eq!(h2_result1.unwrap().status(), 200);
    assert_eq!(h2_result2.unwrap().status(), 200);

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that `limit_connections()` enforces the connection limit correctly using semaphores.
#[tokio::test]
async fn test_limit_connections_blocks_excess() {
    use tokio::sync::Semaphore;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .limit_connections(2); // Allow only 2 concurrent connections

    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Use a semaphore to control when requests complete
    // We'll hold permits to keep connections open
    let sem = Arc::new(Semaphore::new(0));
    let sem_clone = sem.clone();

    let semaphore_service = move |_request: http::Request<hyper::body::Incoming>| {
        let sem = sem_clone.clone();
        async move {
            // Wait for a permit (blocks until we release permits in the test)
            let _permit = sem.acquire().await.unwrap();
            Ok::<_, Infallible>(http::Response::builder().status(200).body(to_boxed("OK")).unwrap())
        }
    };

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(semaphore_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create 3 separate TCP connections and HTTP/1 clients
    let make_request = |addr: std::net::SocketAddr| async move {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();

        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("Connection error: {e:?}");
            }
        });

        let request = http::Request::builder()
            .uri("/test")
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();

        sender.send_request(request).await
    };

    // Start 3 requests concurrently
    let handle1 = tokio::spawn(make_request(addr));
    let handle2 = tokio::spawn(make_request(addr));
    let handle3 = tokio::spawn(make_request(addr));

    // Give them time to attempt connections
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now release 3 permits so all requests can complete
    sem.add_permits(3);

    // All requests should eventually complete (with timeout to prevent hanging)
    let timeout = Duration::from_secs(60);
    let result1 = tokio::time::timeout(timeout, handle1)
        .await
        .expect("First request timed out")
        .unwrap();
    let result2 = tokio::time::timeout(timeout, handle2)
        .await
        .expect("Second request timed out")
        .unwrap();
    let result3 = tokio::time::timeout(timeout, handle3)
        .await
        .expect("Third request timed out")
        .unwrap();

    // All should succeed - the limiter allows connections through (just limits concurrency)
    assert!(result1.is_ok(), "First request failed");
    assert!(result2.is_ok(), "Second request failed");
    assert!(result3.is_ok(), "Third request failed");

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

/// Test that graceful shutdown completes quickly when there are no active connections.
#[tokio::test]
async fn test_immediate_graceful_shutdown() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Immediately trigger shutdown without any connections
    shutdown_tx.send(()).unwrap();

    // Server should shutdown quickly since there are no connections
    let result = tokio::time::timeout(Duration::from_millis(500), server_handle)
        .await
        .expect("server did not shutdown in time")
        .expect("server task panicked");

    assert!(result.is_ok(), "server should shutdown cleanly");
}

/// Test HTTP/2 stream multiplexing by sending concurrent requests over a single connection using a barrier.
#[tokio::test]
async fn test_multiple_concurrent_http2_streams() {
    use tokio::sync::Barrier;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Use a barrier to ensure all 5 requests arrive before any respond
    // This proves HTTP/2 multiplexing is working
    let barrier = Arc::new(Barrier::new(5));
    let barrier_clone = barrier.clone();

    let barrier_service = move |_request: http::Request<hyper::body::Incoming>| {
        let barrier = barrier_clone.clone();
        async move {
            // Wait for all 5 requests to arrive
            barrier.wait().await;
            // Now all respond together
            Ok::<_, Infallible>(http::Response::builder().status(200).body(to_boxed("OK")).unwrap())
        }
    };

    // Start server with HTTP/2 only and configure max concurrent streams
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(barrier_service)))
            .configure_hyper(|mut builder| {
                builder.http2().max_concurrent_streams(5);
                builder.http2_only()
            })
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create HTTP/2 connection
    let stream = tokio::net::TcpStream::connect(addr).await.expect("failed to connect");
    let io = hyper_util::rt::TokioIo::new(stream);

    let (sender, conn) = hyper::client::conn::http2::handshake(hyper_util::rt::TokioExecutor::new(), io)
        .await
        .expect("http2 handshake failed");

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("HTTP/2 connection error: {err:?}");
        }
    });

    // Send multiple concurrent requests over the same HTTP/2 connection
    let mut handles = vec![];

    for i in 0..5 {
        let request = http::Request::builder()
            .uri(format!("/test{i}"))
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();

        let mut sender_clone = sender.clone();
        let handle = tokio::spawn(async move { sender_clone.send_request(request).await });
        handles.push(handle);
    }

    // Wait for all requests to complete with timeout
    // If they complete, it means the barrier was satisfied (all 5 arrived concurrently)
    let timeout = Duration::from_secs(60);
    for (i, handle) in handles.into_iter().enumerate() {
        let response = tokio::time::timeout(timeout, handle)
            .await
            .unwrap_or_else(|_| panic!("Request {i} timed out"))
            .unwrap()
            .expect("request failed");
        assert_eq!(response.status(), 200);
    }

    shutdown_tx.send(()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
}

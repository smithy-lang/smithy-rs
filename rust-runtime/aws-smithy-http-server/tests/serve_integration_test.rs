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

#[tokio::test]
async fn test_serve_defaults_to_16k_connections() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)));

    assert!(
        format!("{server:?}").contains("max_connections: Some(16384)"),
        "serve should default to 16,384 concurrent connections"
    );
}

#[tokio::test]
async fn test_serve_max_connections_blocks_excess_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let accepted_connections = Arc::new(AtomicUsize::new(0));
    let accepted_connections_clone = accepted_connections.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |_| {
            accepted_connections_clone.fetch_add(1, Ordering::SeqCst);
        });
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .max_connections(1)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let first_connection = tokio::net::TcpStream::connect(addr)
        .await
        .expect("failed to open first connection");
    for _ in 0..20 {
        if accepted_connections.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(accepted_connections.load(Ordering::SeqCst), 1);

    let mut second_connection = tokio::net::TcpStream::connect(addr)
        .await
        .expect("failed to open second connection");
    second_connection
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("failed to write second request");

    let mut buffer = vec![0u8; 4096];
    assert!(
        tokio::time::timeout(Duration::from_millis(200), second_connection.read(&mut buffer))
            .await
            .is_err(),
        "second request should wait while the first connection holds the only slot"
    );
    assert_eq!(accepted_connections.load(Ordering::SeqCst), 1);

    drop(first_connection);

    let n = tokio::time::timeout(Duration::from_secs(2), second_connection.read(&mut buffer))
        .await
        .expect("second request should complete after the first connection closes")
        .expect("failed to read second response");
    assert!(String::from_utf8_lossy(&buffer[..n]).contains("HTTP/1.1 200 OK"));

    drop(second_connection);
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server did not shutdown in time")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
}

#[tokio::test]
async fn test_upgraded_connection_holds_connection_permit() {
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    for timed in [false, true] {
        for connect in [false, true] {
            let (accepted_tx, mut accepted_rx) = mpsc::channel(2);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("failed to bind")
                .tap_io(move |_| accepted_tx.try_send(()).unwrap());
            let addr = listener.local_addr().unwrap();
            let (upgraded_tx, mut upgraded_rx) = mpsc::channel(1);
            let service = service_fn(move |mut request: http::Request<hyper::body::Incoming>| {
                let upgraded_tx = upgraded_tx.clone();
                async move {
                    let connect = request.method() == http::Method::CONNECT;
                    let response = if connect || request.uri().path() == "/upgrade" {
                        tokio::spawn(async move {
                            let upgraded = hyper::upgrade::on(&mut request).await.expect("upgrade failed");
                            upgraded_tx.send(upgraded).await.unwrap();
                        });
                        http::Response::builder()
                            .status(if connect { 200 } else { 101 })
                            .header("Connection", "upgrade")
                            .header("Upgrade", "test")
                            .body(to_boxed(""))
                            .unwrap()
                    } else {
                        http::Response::new(to_boxed("OK"))
                    };
                    Ok::<_, Infallible>(response)
                }
            });
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server_handle = tokio::spawn(async move {
                let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
                    .max_connections(1)
                    .header_read_timeout(timed.then(|| Duration::from_millis(100)));
                server
                    .with_graceful_shutdown(async {
                        shutdown_rx.await.ok();
                    })
                    .await
            });

            let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
            let request = if connect {
                b"CONNECT localhost:443 HTTP/1.1\r\nHost: localhost:443\r\n\r\n".as_slice()
            } else {
                b"GET /upgrade HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: test\r\n\r\n".as_slice()
            };
            first.write_all(request).await.unwrap();
            let mut buffer = [0; 4096];
            let n = tokio::time::timeout(Duration::from_secs(2), first.read(&mut buffer))
                .await
                .expect("upgrade response timed out")
                .unwrap();
            let status = if connect { "200 OK" } else { "101 Switching Protocols" };
            assert!(String::from_utf8_lossy(&buffer[..n]).contains(status));
            let upgraded = tokio::time::timeout(Duration::from_secs(2), upgraded_rx.recv())
                .await
                .expect("server upgrade timed out")
                .unwrap();
            accepted_rx.recv().await.unwrap();

            type Io = TokioIo<aws_smithy_http_server::serve::ConnLimiterIo<tokio::net::TcpStream>>;
            let parts = hyper_util::server::conn::auto::upgrade::downcast::<Io>(upgraded)
                .expect("automatic HTTP/1 IO downcast failed");
            assert!(parts.read_buf.is_empty());
            let mut upgraded = parts.io.into_inner();
            first.write_all(b"x").await.unwrap();
            let mut byte = [0];
            tokio::time::timeout(Duration::from_secs(2), upgraded.read_exact(&mut byte))
                .await
                .expect("upgraded connection should remain usable")
                .unwrap();
            assert_eq!(byte, [b'x']);

            let mut second = tokio::net::TcpStream::connect(addr).await.unwrap();
            second
                .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(200), accepted_rx.recv())
                    .await
                    .is_err(),
                "an upgraded socket must retain its connection permit"
            );

            drop(upgraded);
            drop(first);
            let n = tokio::time::timeout(Duration::from_secs(2), second.read(&mut buffer))
                .await
                .expect("closing the upgraded socket should release the slot")
                .unwrap();
            assert!(String::from_utf8_lossy(&buffer[..n]).contains("200 OK"));
            accepted_rx.recv().await.unwrap();

            drop(second);
            shutdown_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), server_handle)
                .await
                .expect("server shutdown timed out")
                .expect("server task panicked")
                .expect("server should shutdown cleanly");
        }
    }
}
#[tokio::test]
async fn test_header_read_timeout_releases_stalled_connection_slots() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    // Test both accept loops and a custom Hyper builder before any complete
    // request head reaches the service.
    for prefix in [
        b"".as_slice(),
        b"P",
        b"PRI * HTTP/2.0\r\n",
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
        b"GET /test HTTP/1.1\r\nHost: localhost\r\n",
    ] {
        for graceful in [false, true] {
            let (accepted_tx, mut accepted_rx) = mpsc::channel(2);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("failed to bind")
                .tap_io(move |_| accepted_tx.try_send(()).unwrap());
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
                .max_connections(1)
                .header_read_timeout(Some(Duration::from_millis(100)));
            let server = if graceful {
                server.configure_hyper(|builder| builder)
            } else {
                server
            };
            let server_handle = tokio::spawn(async move {
                if graceful {
                    server
                        .with_graceful_shutdown(async {
                            shutdown_rx.await.ok();
                        })
                        .await
                } else {
                    server.await
                }
            });

            let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
            first.write_all(prefix).await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), accepted_rx.recv())
                .await
                .expect("first connection was not accepted")
                .unwrap();
            let mut second = tokio::net::TcpStream::connect(addr).await.unwrap();
            second
                .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();

            let mut first_bytes = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), first.read_to_end(&mut first_bytes))
                .await
                .expect("header-read timeout should close the first connection")
                .unwrap_or_else(|error| {
                    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
                    0
                });
            // HTTP/2 can send connection-level SETTINGS before receiving a head.
            if prefix != b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" {
                assert!(first_bytes.is_empty(), "unexpected response: {first_bytes:?}");
            }
            let mut buffer = [0; 4096];
            let n = tokio::time::timeout(Duration::from_secs(2), second.read(&mut buffer))
                .await
                .expect("header-read timeout should release the connection slot")
                .unwrap();
            assert!(String::from_utf8_lossy(&buffer[..n]).contains("200 OK"));

            drop(first);
            drop(second);
            if graceful {
                shutdown_tx.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(2), server_handle)
                    .await
                    .expect("server shutdown timed out")
                    .expect("server task panicked")
                    .expect("server should shutdown cleanly");
            } else {
                server_handle.abort();
                assert!(server_handle.await.unwrap_err().is_cancelled());
            }
        }
    }
}

#[tokio::test]
async fn test_header_read_timeout_does_not_limit_established_http1_connection() {
    use http_body_util::BodyExt;
    use hyper_util::rt::TokioIo;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let service = service_fn(|request| async {
        // Timely heads must allow processing that outlasts the header deadline.
        tokio::time::sleep(Duration::from_millis(150)).await;
        ok_service(request).await
    });
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .header_read_timeout(Some(Duration::from_millis(100)))
            .await
    });
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let connection_handle = tokio::spawn(connection);
    for _ in 0..2 {
        let request = http::Request::builder()
            .uri("/test")
            .header("Host", "localhost")
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(2), sender.send_request(request))
            .await
            .expect("an established connection must outlive the first-header deadline")
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "OK");
    }
    drop(sender);
    tokio::time::timeout(Duration::from_secs(2), connection_handle)
        .await
        .expect("client connection should close")
        .unwrap()
        .unwrap();
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server shutdown timed out")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
}

#[tokio::test]
async fn test_graceful_shutdown_interrupts_connection_limit_wait() {
    let accepted_connections = Arc::new(AtomicUsize::new(0));
    let accepted_connections_clone = accepted_connections.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |_| {
            accepted_connections_clone.fetch_add(1, Ordering::SeqCst);
        });
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .max_connections(1)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let connection = tokio::net::TcpStream::connect(addr)
        .await
        .expect("failed to open connection");
    for _ in 0..20 {
        if accepted_connections.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(accepted_connections.load(Ordering::SeqCst), 1);

    shutdown_tx.send(()).unwrap();
    drop(connection);

    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("shutdown should interrupt the wait for a connection permit")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
}

#[tokio::test]
async fn test_disable_connection_limit_accepts_multiple_connections() {
    let accepted_connections = Arc::new(AtomicUsize::new(0));
    let accepted_connections_clone = accepted_connections.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |_| {
            accepted_connections_clone.fetch_add(1, Ordering::SeqCst);
        });
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .max_connections(1)
            .disable_connection_limit()
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let first_connection = tokio::net::TcpStream::connect(addr).await.unwrap();
    let second_connection = tokio::net::TcpStream::connect(addr).await.unwrap();
    for _ in 0..20 {
        if accepted_connections.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(accepted_connections.load(Ordering::SeqCst), 2);

    drop(first_connection);
    drop(second_connection);
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server did not shutdown in time")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
}

#[tokio::test]
async fn test_max_connections_zero_disables_the_limit() {
    let accepted_connections = Arc::new(AtomicUsize::new(0));
    let accepted_connections_clone = accepted_connections.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind")
        .tap_io(move |_| {
            accepted_connections_clone.fetch_add(1, Ordering::SeqCst);
        });
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .max_connections(0)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let first_connection = tokio::net::TcpStream::connect(addr).await.unwrap();
    let second_connection = tokio::net::TcpStream::connect(addr).await.unwrap();
    for _ in 0..20 {
        if accepted_connections.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(accepted_connections.load(Ordering::SeqCst), 2);

    drop(first_connection);
    drop(second_connection);
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server did not shutdown in time")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
}

/// A limit above tokio's `Semaphore::MAX_PERMITS` must be clamped, not panic the server task.
#[tokio::test]
async fn test_max_connections_usize_max_is_clamped_not_a_panic() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .max_connections(usize::MAX)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("failed to connect");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("failed to write request");
    let mut buffer = vec![0u8; 4096];
    let n = stream.read(&mut buffer).await.expect("failed to read response");
    let response_text = String::from_utf8_lossy(&buffer[..n]);
    assert!(response_text.contains("200 OK"), "{response_text}");

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server did not shutdown in time")
        .expect("server task panicked")
        .expect("server should shutdown cleanly");
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

#[tokio::test]
async fn test_header_read_timeout_excludes_connection_service_creation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let factory = service_fn(
        |_incoming: aws_smithy_http_server::serve::IncomingStream<'_, tokio::net::TcpListener>| async {
            tokio::time::sleep(Duration::from_millis(350)).await;
            Ok::<_, Infallible>(service_fn(ok_service))
        },
    );
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, factory)
            .header_read_timeout(Some(Duration::from_millis(100)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{response:?}");
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn test_disabled_header_read_timeout_overrides_custom_builder() {
    use hyper_util::rt::TokioTimer;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    for timeout in [None, Some(Duration::ZERO)] {
        for graceful in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)));
            let server = server.configure_hyper(|mut builder| {
                builder
                    .http1()
                    .timer(TokioTimer::new())
                    .header_read_timeout(Duration::from_millis(10));
                builder
            });
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server = tokio::spawn(async move {
                if graceful {
                    server
                        .with_graceful_shutdown(async {
                            shutdown_rx.await.ok();
                        })
                        .header_read_timeout(timeout)
                        .await
                } else {
                    server.header_read_timeout(timeout).await
                }
            });

            let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
            let mut buffer = [0; 4096];
            // Neither a silent connection nor an incomplete HTTP/1 head may expire.
            assert!(
                tokio::time::timeout(Duration::from_millis(100), client.read(&mut buffer))
                    .await
                    .is_err(),
                "silent connection expired: {timeout:?}, graceful={graceful}"
            );
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n")
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(100), client.read(&mut buffer))
                    .await
                    .is_err(),
                "incomplete headers expired: {timeout:?}, graceful={graceful}"
            );
            client.write_all(b"\r\n").await.unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{response:?}");
            if graceful {
                shutdown_tx.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(2), server)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            } else {
                server.abort();
                assert!(server.await.unwrap_err().is_cancelled());
            }
        }
    }
}

#[tokio::test]
async fn test_header_read_timeout_does_not_limit_established_http2_connection() {
    use http_body_util::BodyExt;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service_fn(ok_service)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .header_read_timeout(Some(Duration::from_millis(100)))
            .await
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
        .await
        .unwrap();
    let connection = tokio::spawn(connection);
    for index in 0..2 {
        if index == 1 {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        let request = http::Request::builder()
            .uri(format!("http://{addr}/test"))
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(2), sender.send_request(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "OK");
    }
    drop(sender);
    tokio::time::timeout(Duration::from_secs(2), connection)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn test_header_read_timeout_applies_to_later_http1_heads() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let requests = called.clone();
    let service = service_fn(move |_request: http::Request<hyper::body::Incoming>| {
        requests.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, Infallible>(http::Response::new(to_boxed("OK"))) }
    });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
            .header_read_timeout(Some(Duration::from_millis(100)))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /first HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buffer = [0; 4096];
    let n = tokio::time::timeout(Duration::from_secs(2), client.read(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    assert!(buffer[..n].starts_with(b"HTTP/1.1 200 OK"));
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .unwrap();
    let mut remainder = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut remainder))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        called.load(Ordering::SeqCst),
        1,
        "the incomplete second head must not reach the service"
    );
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Protocol restrictions must reject the other protocol even when header timing
/// is disabled, before any request reaches the application.
#[tokio::test]
async fn test_single_protocol_modes_reject_the_other_protocol() {
    use http_body_util::BodyExt;
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn http1_response(addr: SocketAddr) -> Vec<u8> {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .expect("HTTP/1 connection did not close")
            .unwrap_or_else(|error| {
                assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
                0
            });
        response
    }

    async fn http2_status(addr: SocketAddr) -> Option<http::StatusCode> {
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let handshake = tokio::time::timeout(
            Duration::from_secs(2),
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream)),
        )
        .await
        .expect("HTTP/2 handshake stalled");
        let Ok((mut sender, connection)) = handshake else {
            return None;
        };
        let connection = tokio::spawn(connection);
        let request = http::Request::builder()
            .uri(format!("http://{addr}/test"))
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), sender.send_request(request))
            .await
            .expect("HTTP/2 request stalled");
        let status = if let Ok(response) = result {
            let status = response.status();
            response.into_body().collect().await.unwrap();
            Some(status)
        } else {
            None
        };
        drop(sender);
        // A rejected protocol may produce a connection error; the task must finish.
        let _ = tokio::time::timeout(Duration::from_secs(2), connection)
            .await
            .expect("HTTP/2 connection did not close")
            .expect("HTTP/2 client task panicked");
        status
    }

    for http1_only in [true, false] {
        for graceful in [false, true] {
            for timed in [false, true] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let called = Arc::new(AtomicUsize::new(0));
                let requests = called.clone();
                let service = service_fn(move |request: http::Request<hyper::body::Incoming>| {
                    assert_eq!(
                        request.version(),
                        if http1_only {
                            http::Version::HTTP_11
                        } else {
                            http::Version::HTTP_2
                        }
                    );
                    requests.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, Infallible>(http::Response::new(to_boxed("OK"))) }
                });
                let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
                    .max_connections(1)
                    .header_read_timeout(timed.then(|| Duration::from_millis(100)));
                let server = server.configure_hyper(move |mut builder| {
                    if http1_only {
                        builder.http1().title_case_headers(true);
                        builder.http1_only()
                    } else {
                        builder.http2_only()
                    }
                });
                let (shutdown_tx, shutdown_rx) = oneshot::channel();
                let server = tokio::spawn(async move {
                    if graceful {
                        server
                            .with_graceful_shutdown(async {
                                shutdown_rx.await.ok();
                            })
                            .await
                    } else {
                        server.await
                    }
                });

                if http1_only {
                    assert_eq!(http2_status(addr).await, None, "HTTP/1-only must reject HTTP/2");
                } else {
                    let response = http1_response(addr).await;
                    assert!(!response.starts_with(b"HTTP/1.1 200"), "HTTP/2-only accepted HTTP/1");
                }
                assert_eq!(
                    called.load(Ordering::SeqCst),
                    0,
                    "the disallowed protocol reached the service"
                );

                // Rejection must release the only connection slot. The allowed
                // protocol and its configured settings must still work afterward.
                if http1_only {
                    let response = http1_response(addr).await;
                    assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{response:?}");
                    assert!(String::from_utf8_lossy(&response).contains("Content-Length:"));
                } else {
                    assert_eq!(http2_status(addr).await, Some(http::StatusCode::OK));
                }
                assert_eq!(called.load(Ordering::SeqCst), 1);

                if graceful {
                    shutdown_tx.send(()).unwrap();
                    tokio::time::timeout(Duration::from_secs(2), server)
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                } else {
                    server.abort();
                    assert!(server.await.unwrap_err().is_cancelled());
                }
            }
        }
    }
}

#[tokio::test]
async fn test_graceful_shutdown_drains_inflight_request_for_both_serving_apis() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::Notify;

    for http1_only in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let started = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let started_handler = started.clone();
        let finish_handler = finish.clone();
        let service = service_fn(move |_request: http::Request<hyper::body::Incoming>| {
            let started = started_handler.clone();
            let finish = finish_handler.clone();
            async move {
                started.notify_one();
                finish.notified().await;
                Ok::<_, Infallible>(http::Response::new(to_boxed("OK")))
            }
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let mut server = tokio::spawn(async move {
            aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
                .configure_hyper(|builder| if http1_only { builder.http1_only() } else { builder })
                .header_read_timeout(Some(Duration::from_millis(100)))
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        shutdown_tx.send(()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut server)
                .await
                .is_err(),
            "shutdown must wait for the in-flight request beyond the header deadline"
        );
        finish.notify_one();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{response:?}");
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

/// Exercise the public protocol configuration APIs with the same paused-clock
/// scenario as the deadline wrapper's partial-header unit test.
#[tokio::test(start_paused = true)]
async fn test_partial_bytes_do_not_restart_header_deadline_for_configured_protocols() {
    use futures_util::FutureExt;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

    // Signal on Hyper's first read so time advances only after its connection
    // and header deadline have actually started.
    struct TestIo {
        stream: DuplexStream,
        started: Option<oneshot::Sender<()>>,
    }

    impl AsyncRead for TestIo {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if let Some(started) = self.started.take() {
                started.send(()).unwrap();
            }
            Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for TestIo {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.stream).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }

    struct TestListener(Option<TestIo>);

    impl Listener for TestListener {
        type Io = TestIo;
        type Addr = ();

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            match self.0.take() {
                Some(io) => (io, ()),
                None => std::future::pending().await,
            }
        }

        fn local_addr(&self) -> std::io::Result<Self::Addr> {
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Protocol {
        Auto,
        AutoHttp1Only,
        Http2Only,
    }

    for protocol in [Protocol::Auto, Protocol::AutoHttp1Only, Protocol::Http2Only] {
        let (server_io, mut client) = tokio::io::duplex(4096);
        let (started_tx, started_rx) = oneshot::channel();
        let listener = TestListener(Some(TestIo {
            stream: server_io,
            started: Some(started_tx),
        }));
        let called = Arc::new(AtomicBool::new(false));
        let service_called = Arc::clone(&called);
        let service = service_fn(move |request| {
            service_called.store(true, Ordering::SeqCst);
            ok_service(request)
        });
        let server = aws_smithy_http_server::serve::serve(listener, IntoMakeService::new(service))
            .header_read_timeout(Some(Duration::from_secs(5)));
        let server = match protocol {
            Protocol::Auto => server,
            Protocol::AutoHttp1Only => server.configure_hyper(|builder| builder.http1_only()),
            Protocol::Http2Only => server.configure_hyper(|builder| builder.http2_only()),
        };
        let server_task = tokio::spawn(async move { server.await });
        started_rx.await.unwrap();

        tokio::time::advance(Duration::from_secs(4)).await;
        let partial_head = match protocol {
            Protocol::Http2Only => b"PRI * HTTP/2.0\r\n".as_slice(),
            _ => b"GET / HTTP/1.1\r\n".as_slice(),
        };
        client.write_all(partial_head).await.unwrap();
        tokio::task::yield_now().await;
        let mut response = Vec::new();
        assert!(
            client.read_to_end(&mut response).now_or_never().is_none(),
            "connection closed before the deadline: {protocol:?}"
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        // Bound the wait: awaiting EOF without a timeout would let the
        // paused clock automatically advance to a wrongly restarted timer.
        tokio::time::timeout(Duration::from_millis(1), client.read_to_end(&mut response))
            .await
            .unwrap_or_else(|_| panic!("partial bytes restarted the deadline: {protocol:?}"))
            .unwrap();
        assert!(!called.load(Ordering::SeqCst), "incomplete headers reached the service");

        server_task.abort();
        assert!(server_task.await.unwrap_err().is_cancelled());
    }
}

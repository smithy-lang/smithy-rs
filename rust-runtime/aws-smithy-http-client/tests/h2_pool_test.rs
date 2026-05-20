/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! H2 connection pool behavior tests.
//!
//! Tests the v2 pool's HTTP/2 path: multiplexing, GOAWAY handling,
//! connection poisoning, and stream limits.
//!
//! Uses plain TCP with a fake ALPN signal (`Connected::new().negotiated_h2()`)
//! so no TLS infrastructure is needed. The pool's Negotiate layer trusts
//! the `Connected` metadata to route to the H2 path.

#![cfg(all(
    feature = "wire-mock",
    feature = "default-client",
    feature = "test-util",
    aws_sdk_unstable
))]

use aws_smithy_http_client::pool::Builder;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorSettings, SharedHttpClient,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use bytes::Bytes;
use h2::server::SendResponse;
use h2::RecvStream;
use http_body_util::BodyExt;
use hyper_util::client::legacy::connect::Connected;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tower::Service;

// ---------------------------------------------------------------------------
// H2MockServer — plain TCP server speaking H2 via the h2 crate directly
// ---------------------------------------------------------------------------

/// Handler function type for H2 requests.
type H2Handler =
    Arc<dyn Fn(http_1x::Request<RecvStream>, SendResponse<Bytes>) + Send + Sync + 'static>;

struct H2MockServer {
    addr: SocketAddr,
    /// Total H2 connections accepted (each connection can multiplex many streams).
    connections: Arc<AtomicUsize>,
    /// Total streams (requests) handled across all connections.
    streams: Arc<AtomicUsize>,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl H2MockServer {
    /// Start an H2 server that responds 200 with the given body to every request.
    async fn start(body: &'static str) -> Self {
        Self::start_with_handler(Arc::new(move |_req, mut respond| {
            let response = http_1x::Response::builder().status(200).body(()).unwrap();
            let mut send_stream = respond.send_response(response, false).unwrap();
            send_stream.send_data(Bytes::from(body), true).unwrap();
        }))
        .await
    }

    /// Start with a custom handler for each stream.
    async fn start_with_handler(handler: H2Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(AtomicUsize::new(0));
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let conns = connections.clone();
        let strms = streams.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        let (stream, _) = match accept {
                            Ok(v) => v,
                            Err(_) => break,
                        };
                        conns.fetch_add(1, Ordering::SeqCst);
                        let handler = handler.clone();
                        let strms = strms.clone();
                        tokio::spawn(async move {
                            let mut conn = h2::server::Builder::new()
                                .handshake(stream)
                                .await
                                .unwrap();
                            while let Some(result) = conn.accept().await {
                                let (req, respond) = result.unwrap();
                                strms.fetch_add(1, Ordering::SeqCst);
                                handler(req, respond);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });

        Self {
            addr,
            connections,
            streams,
            _shutdown: shutdown_tx,
        }
    }

    /// Start a server that sends GOAWAY after `n` streams on each connection.
    async fn start_goaway_after(n: usize, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(AtomicUsize::new(0));
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let conns = connections.clone();
        let strms = streams.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        let (stream, _) = match accept {
                            Ok(v) => v,
                            Err(_) => break,
                        };
                        conns.fetch_add(1, Ordering::SeqCst);
                        let strms = strms.clone();
                        let per_conn_count = Arc::new(AtomicUsize::new(0));
                        tokio::spawn(async move {
                            let mut conn = h2::server::Builder::new()
                                .handshake(stream)
                                .await
                                .unwrap();
                            while let Some(result) = conn.accept().await {
                                let (req, mut respond) = result.unwrap();
                                strms.fetch_add(1, Ordering::SeqCst);
                                let count = per_conn_count.fetch_add(1, Ordering::SeqCst) + 1;

                                // Respond normally
                                let response = http_1x::Response::builder()
                                    .status(200)
                                    .body(())
                                    .unwrap();
                                let mut send_stream = respond.send_response(response, false).unwrap();
                                send_stream.send_data(Bytes::from(body), true).unwrap();
                                drop(req);

                                // After n streams, send GOAWAY
                                if count >= n {
                                    conn.graceful_shutdown();
                                }
                            }
                        });
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });

        Self {
            addr,
            connections,
            streams,
            _shutdown: shutdown_tx,
        }
    }

    fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn stream_count(&self) -> usize {
        self.streams.load(Ordering::SeqCst)
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.addr.port())
    }
}

// ---------------------------------------------------------------------------
// H2Connector — connects via TCP, signals negotiated_h2()
// ---------------------------------------------------------------------------

/// IO wrapper that signals H2 negotiation to the pool's Negotiate layer.
struct H2Io {
    inner: TcpStream,
}

impl hyper_util::client::legacy::connect::Connection for H2Io {
    fn connected(&self) -> Connected {
        Connected::new().negotiated_h2()
    }
}

impl AsyncRead for H2Io {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for H2Io {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// hyper::rt::Read and Write are needed by the pool
impl hyper::rt::Read for H2Io {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        let n = unsafe {
            let mut tbuf = ReadBuf::uninit(buf.as_mut());
            match Pin::new(&mut self.get_mut().inner).poll_read(cx, &mut tbuf) {
                Poll::Ready(Ok(())) => tbuf.filled().len(),
                other => return other,
            }
        };
        unsafe { buf.advance(n) };
        Poll::Ready(Ok(()))
    }
}

impl hyper::rt::Write for H2Io {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Connector that establishes TCP connections and signals H2 negotiation.
#[derive(Clone)]
struct H2Connector {
    addr: SocketAddr,
}

impl Service<http_1x::Uri> for H2Connector {
    type Response = H2Io;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http_1x::Uri) -> Self::Future {
        let addr = self.addr;
        Box::pin(async move {
            let stream = TcpStream::connect(addr).await?;
            Ok(H2Io { inner: stream })
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_h2_client(server: &H2MockServer) -> SharedHttpClient {
    Builder::new().build_http_with_tcp_connector(H2Connector { addr: server.addr })
}

fn runtime_components() -> aws_smithy_runtime_api::client::runtime_components::RuntimeComponents {
    RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(aws_smithy_async::time::SystemTimeSource::new()))
        .build()
        .expect("valid runtime components")
}

async fn send_request(
    client: &SharedHttpClient,
    url: &str,
) -> Result<(u16, Vec<u8>), aws_smithy_runtime_api::client::result::ConnectorError> {
    let settings = HttpConnectorSettings::builder().build();
    let components = runtime_components();
    let connector = client.http_connector(&settings, &components);
    let resp = connector
        .call(HttpRequest::get(url).expect("valid request"))
        .await?;
    let status = resp.status().as_u16();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    Ok((status, body))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Multiple concurrent requests multiplex over a single H2 connection.
#[tokio::test]
async fn h2_multiplexing_shares_one_connection() {
    let server = H2MockServer::start("ok").await;
    let client = build_h2_client(&server);
    let url = server.url();

    // Warm the connection with one request first so the Singleton is populated
    let (status, _) = send_request(&client, &url).await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(server.connection_count(), 1);

    // Now send 4 concurrent requests — they should all multiplex on the existing connection
    let futs: Vec<_> = (0..4).map(|_| send_request(&client, &url)).collect();
    let results = futures_util::future::join_all(futs).await;

    for (i, r) in results.iter().enumerate() {
        let (status, _) = r
            .as_ref()
            .unwrap_or_else(|e| panic!("request {i} failed: {e}"));
        assert_eq!(*status, 200);
    }

    // All 5 requests (1 warm + 4 concurrent) should have used a single connection
    assert_eq!(
        server.connection_count(),
        1,
        "H2 should multiplex on one connection"
    );
    assert_eq!(server.stream_count(), 5, "should have 5 streams");
}

/// After GOAWAY, the pool establishes a new connection for subsequent requests.
#[tokio::test]
async fn h2_goaway_triggers_new_connection() {
    // Server sends GOAWAY after 2 streams per connection
    let server = H2MockServer::start_goaway_after(2, "ok").await;
    let client = build_h2_client(&server);
    let url = server.url();

    // First 2 requests on connection 1
    let (s1, _) = send_request(&client, &url).await.unwrap();
    let (s2, _) = send_request(&client, &url).await.unwrap();
    assert_eq!(s1, 200);
    assert_eq!(s2, 200);

    // Give the pool time to observe the GOAWAY
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Next request should go on a new connection
    let (s3, _) = send_request(&client, &url).await.unwrap();
    assert_eq!(s3, 200);

    assert!(
        server.connection_count() >= 2,
        "should have opened a second connection after GOAWAY, got {}",
        server.connection_count()
    );
}

/// Sequential requests reuse the same H2 connection (no unnecessary reconnects).
#[tokio::test]
async fn h2_sequential_requests_reuse_connection() {
    let server = H2MockServer::start("hello").await;
    let client = build_h2_client(&server);
    let url = server.url();

    for i in 0..5 {
        let (status, body) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200, "request {i}");
        assert_eq!(body, b"hello", "request {i}");
    }

    assert_eq!(
        server.connection_count(),
        1,
        "should reuse one H2 connection"
    );
    assert_eq!(server.stream_count(), 5);
}

/// Poisoning an H2 connection forces the pool to establish a new one.
#[tokio::test]
async fn h2_poisoned_connection_not_reused() {
    use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;

    let server = H2MockServer::start("ok").await;
    let client = build_h2_client(&server);
    let url = server.url();

    // First request: establish H2 connection, capture metadata.
    let settings = HttpConnectorSettings::builder().build();
    let components = runtime_components();
    let connector = client.http_connector(&settings, &components);

    let capture = CaptureSmithyConnection::new();
    let mut request = HttpRequest::get(&url).expect("valid request");
    request.add_extension(capture.clone());

    let resp = connector
        .call(request)
        .await
        .expect("request should succeed");
    let _body = resp.into_body().collect().await.expect("body");
    assert_eq!(server.connection_count(), 1);

    let metadata = capture.get().expect("adapter should populate metadata");
    metadata.poison();

    // Give the pool a moment to observe the poison
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Next request should open a NEW H2 connection
    let (status, _) = send_request(&client, &url).await.unwrap();
    assert_eq!(status, 200);
    assert!(
        server.connection_count() >= 2,
        "poisoned H2 connection should not be reused, got {} connections",
        server.connection_count()
    );
}

// ===========================================================================
// TLS + ALPN — real certificate negotiation
// ===========================================================================
//
// These tests use a TLS server with self-signed certs advertising h2 via ALPN.
// They verify the v2 pool correctly routes to H2 when ALPN negotiates it
// through real TLS.

#[cfg(feature = "rustls-aws-lc")]
mod tls_h2 {
    use aws_smithy_http_client::pool::Builder;
    use aws_smithy_http_client::tls;
    use aws_smithy_http_client::tls::{TlsContext, TrustStore};
    use aws_smithy_runtime_api::client::http::{
        HttpClient, HttpConnector, HttpConnectorSettings, SharedHttpClient,
    };
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use http_body_util::BodyExt;
    use hyper_util::rt::TokioExecutor;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    /// TLS test server that tracks connection count and serves H2 via ALPN.
    struct TlsH2Server {
        addr: SocketAddr,
        connections: Arc<AtomicUsize>,
        _shutdown: tokio::sync::oneshot::Sender<()>,
    }

    impl TlsH2Server {
        async fn start() -> Self {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let connections = Arc::new(AtomicUsize::new(0));
            let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

            // Load certs
            let cert_pem = std::fs::read("tests/server.pem").unwrap();
            let key_pem = std::fs::read("tests/server.rsa").unwrap();
            let certs: Vec<_> = rustls_pemfile::certs(&mut &cert_pem[..])
                .collect::<Result<_, _>>()
                .unwrap();
            let key = rustls_pemfile::private_key(&mut &key_pem[..])
                .unwrap()
                .unwrap();

            let mut server_config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .unwrap();
            // Only advertise h2 — force H2 negotiation
            server_config.alpn_protocols = vec![b"h2".to_vec()];
            let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

            let conns = connections.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        accept = listener.accept() => {
                            let (tcp, _) = match accept {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            conns.fetch_add(1, Ordering::SeqCst);
                            let tls_acceptor = tls_acceptor.clone();
                            tokio::spawn(async move {
                                let tls_stream = match tls_acceptor.accept(tcp).await {
                                    Ok(s) => s,
                                    Err(e) => {
                                        eprintln!("TLS accept failed: {e}");
                                        return;
                                    }
                                };
                                let service = hyper::service::service_fn(|_req| async {
                                    Ok::<_, hyper::Error>(
                                        http_1x::Response::builder()
                                            .status(200)
                                            .body(http_body_util::Full::new(
                                                bytes::Bytes::from("h2-ok"),
                                            ))
                                            .unwrap(),
                                    )
                                });
                                let io = hyper_util::rt::TokioIo::new(tls_stream);
                                // Use http2 only server since we only advertise h2
                                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                                    .serve_connection(io, service)
                                    .await;
                            });
                        }
                        _ = &mut shutdown_rx => break,
                    }
                }
            });

            Self {
                addr,
                connections,
                _shutdown: shutdown_tx,
            }
        }

        fn connection_count(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }

        fn url(&self) -> String {
            format!("https://localhost:{}/", self.addr.port())
        }
    }

    fn tls_context() -> TlsContext {
        let pem = std::fs::read("tests/server.pem").unwrap();
        let trust_store = TrustStore::empty().with_pem_certificate(pem);
        TlsContext::builder()
            .with_trust_store(trust_store)
            .build()
            .unwrap()
    }

    fn runtime_components() -> aws_smithy_runtime_api::client::runtime_components::RuntimeComponents
    {
        RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(aws_smithy_async::time::SystemTimeSource::new()))
            .build()
            .unwrap()
    }

    async fn send_request(
        client: &SharedHttpClient,
        url: &str,
    ) -> Result<(u16, Vec<u8>), aws_smithy_runtime_api::client::result::ConnectorError> {
        let settings = HttpConnectorSettings::builder().build();
        let components = runtime_components();
        let connector = client.http_connector(&settings, &components);
        let resp = connector
            .call(HttpRequest::get(url).expect("valid request"))
            .await?;
        let status = resp.status().as_u16();
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        Ok((status, body))
    }

    /// Rustls + ALPN h2: v2 pool routes to H2, multiple requests multiplex.
    #[tokio::test]
    async fn rustls_alpn_h2_multiplexing() {
        let server = TlsH2Server::start().await;
        let client = Builder::new()
            .tls_provider(tls::Provider::Rustls(
                tls::rustls_provider::CryptoMode::AwsLc,
            ))
            .tls_context(tls_context())
            .build_https();

        let url = server.url();

        // First request establishes connection
        let (status, body) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"h2-ok");

        // Second request should multiplex on same connection
        let (status, _) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);

        // Third request too
        let (status, _) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);

        assert_eq!(
            server.connection_count(),
            1,
            "rustls H2: all requests should multiplex on one connection"
        );
    }

    /// s2n-tls + ALPN h2: v2 pool should route to H2 (currently broken — opens
    /// new connection per request because Connected never signals negotiated_h2).
    #[cfg(feature = "s2n-tls")]
    #[tokio::test]
    async fn s2n_tls_alpn_h2_multiplexing() {
        let server = TlsH2Server::start().await;
        let client = Builder::new()
            .tls_provider(tls::Provider::S2nTls)
            .tls_context(tls_context())
            .build_https();

        let url = server.url();

        // First request
        let (status, body) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"h2-ok");

        // Second request — if H2 works, should multiplex on same connection
        let (status, _) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);

        // Third request
        let (status, _) = send_request(&client, &url).await.unwrap();
        assert_eq!(status, 200);

        // This assertion will FAIL until s2n-tls signals negotiated_h2()
        assert_eq!(
            server.connection_count(),
            1,
            "s2n-tls H2: all requests should multiplex on one connection"
        );
    }
}

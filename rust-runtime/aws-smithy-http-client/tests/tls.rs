/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(any(feature = "__rustls", feature = "s2n-tls",))]

mod common;

use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http_client::tls;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnector, HttpConnectorSettings};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_types::byte_stream::ByteStream;
use common::tls as test_tls;
use http_1x::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{debug, error};

struct TestServer {
    _handle: JoinHandle<()>,
    listen_addr: SocketAddr,
    conn_count: Arc<()>,
}

impl TestServer {
    /// Return the number of active connections to this server
    fn conn_count(&self) -> usize {
        // 1 reference for the struct MockProxyServer, 1 reference for the
        // socket task.
        Arc::strong_count(&self.conn_count)
            .checked_sub(2)
            .expect("de-count 2 refs")
    }
}

async fn server() -> Result<TestServer, BoxError> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    debug!("Starting to serve on https://{}", addr);

    let tls_acceptor = test_tls::server_tls_acceptor(&[b"h2", b"http/1.1", b"http/1.0"])?;
    let service = service_fn(echo);

    let conn_count = Arc::new(());
    let server_conn_count = conn_count.clone();

    let server = async move {
        loop {
            let (tcp_stream, remote_addr) = listener.accept().await.unwrap();
            debug!("accepted connection from: {}", remote_addr);

            let tls_acceptor = tls_acceptor.clone();
            let connection_conn_count = server_conn_count.clone();
            tokio::spawn(async move {
                let _connection_conn_count = connection_conn_count;
                let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                    Ok(tls_stream) => tls_stream,
                    Err(err) => {
                        error!("failed to perform tls handshake: {err:#}");
                        return;
                    }
                };
                if let Err(err) = Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls_stream), service)
                    .await
                {
                    error!("failed to serve connection: {err:#}");
                }
            });
        }
    };

    let server_task = tokio::spawn(server);

    Ok(TestServer {
        _handle: server_task,
        listen_addr: addr,
        conn_count,
    })
}

// Custom echo service, handling two different routes and a
// catch-all 404 responder.
async fn echo(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let mut response = Response::new(Full::default());
    match (req.method(), req.uri().path()) {
        // default route.
        (&Method::GET, "/") => {
            *response.body_mut() = Full::from("Hello TLS!");
        }
        // echo service route.
        (&Method::POST, "/echo") => {
            *response.body_mut() = Full::from(req.into_body().collect().await?.to_bytes());
        }
        // Catch-all 404.
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
        }
    };
    Ok(response)
}

#[cfg(feature = "rustls-aws-lc")]
#[should_panic(expected = "InvalidCertificate(UnknownIssuer)")]
#[tokio::test]
async fn test_rustls_aws_lc_native_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "rustls-aws-lc")]
#[tokio::test]
async fn test_rustls_aws_lc_custom_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .tls_context(test_tls::server_tls_context())
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "rustls-aws-lc")]
#[tokio::test(start_paused = false)]
// can't have paused clock due to <https://github.com/hyperium/hyper/issues/3950>
async fn test_rustls_aws_lc_custom_ca_with_timeout() {
    const TIMEOUT: Duration = Duration::from_secs(10);
    let client = aws_smithy_http_client::Builder::new()
        .pool_idle_timeout(TIMEOUT)
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .tls_context(test_tls::server_tls_context())
        .build_https();

    run_tls_test_with_idle_timeout(&client, Some(TIMEOUT))
        .await
        .unwrap()
}

#[cfg(feature = "rustls-aws-lc-fips")]
#[should_panic(expected = "InvalidCertificate(UnknownIssuer)")]
#[tokio::test]
async fn test_rustls_aws_lc_fips_native_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLcFips,
        ))
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "rustls-aws-lc-fips")]
#[tokio::test]
async fn test_rustls_aws_lc_fips_custom_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLcFips,
        ))
        .tls_context(test_tls::server_tls_context())
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "rustls-ring")]
#[should_panic(expected = "InvalidCertificate(UnknownIssuer)")]
#[tokio::test]
async fn test_rustls_ring_native_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "rustls-ring")]
#[tokio::test]
async fn test_rustls_ring_custom_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .tls_context(test_tls::server_tls_context())
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(all(aws_sdk_unstable, feature = "rustls-ring"))]
#[should_panic(expected = "InvalidCertificate(UnknownIssuer)")]
#[tokio::test]
async fn test_rustls_custom_provider_native_ca() {
    let provider = rustls::crypto::ring::default_provider();
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Custom(provider),
        ))
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(all(aws_sdk_unstable, feature = "rustls-ring"))]
#[tokio::test]
async fn test_rustls_custom_provider_custom_ca() {
    let ring_provider = rustls::crypto::ring::default_provider();
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Custom(ring_provider),
        ))
        .tls_context(test_tls::server_tls_context())
        .build_https();
    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "s2n-tls")]
#[should_panic(expected = "Certificate is untrusted")]
#[tokio::test]
async fn test_s2n_native_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::S2nTls)
        .build_https();

    run_tls_test(&client).await.unwrap()
}

#[cfg(feature = "s2n-tls")]
#[tokio::test]
async fn test_s2n_tls_custom_ca() {
    let client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::S2nTls)
        .tls_context(test_tls::server_tls_context())
        .build_https();
    run_tls_test(&client).await.unwrap()
}

async fn run_tls_test(client: &dyn HttpClient) -> Result<(), BoxError> {
    run_tls_test_with_idle_timeout(client, None).await
}

async fn run_tls_test_with_idle_timeout(
    client: &dyn HttpClient,
    pool_timeout: Option<Duration>,
) -> Result<(), BoxError> {
    let server = server().await?;
    let start = tokio::time::Instant::now();
    assert_eq!(server.conn_count(), 0); // calibrate conn_count
    let endpoint = format!("https://localhost:{}/", server.listen_addr.port());

    let connector_settings = HttpConnectorSettings::builder().build();
    let runtime_components = RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(SystemTimeSource::new()))
        .build()
        .unwrap();
    let connector = client.http_connector(&connector_settings, &runtime_components);
    let mut response = connector.call(HttpRequest::get(endpoint).unwrap()).await?;

    let sdk_body = response.take_body();
    let body_stream = ByteStream::new(sdk_body);
    let resp_bytes = body_stream.collect().await?.into_bytes();
    assert_eq!(b"Hello TLS!", &resp_bytes[..]);

    if let Some(pool_timeout) = pool_timeout {
        assert_eq!(server.conn_count(), 1);
        tokio::time::sleep_until(start + pool_timeout - Duration::from_secs(1)).await;
        assert_eq!(server.conn_count(), 1);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(server.conn_count(), 0);
    }
    Ok(())
}

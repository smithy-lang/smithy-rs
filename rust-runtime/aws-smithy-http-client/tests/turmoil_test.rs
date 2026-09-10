/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! End-to-end tests for `test_util::turmoil_client`.
//!
//! These drive the *real* smithy HTTP client (connection pool, connect/read
//! timeouts, error classification) over the [`turmoil`] discrete-event network
//! simulator. Each turmoil host runs on a paused tokio runtime, so the
//! connect-timeout assertion below fires against simulated time and is
//! deterministic.

#![cfg(feature = "__turmoil")]

// The `turmoil-06` / `turmoil-07` features select which major version of the
// `turmoil` crate is linked; alias the enabled one so the test body stays
// version-agnostic. These features are intended to be mutually exclusive, but
// some tooling enables every feature at once, so `turmoil-07` takes precedence
// when both are enabled (matching `src/test_util/turmoil.rs`).
#[cfg(all(feature = "turmoil-06", not(feature = "turmoil-07")))]
use turmoil_0_6 as turmoil;
#[cfg(feature = "turmoil-07")]
use turmoil_0_7 as turmoil;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_http_client::test_util::turmoil_client;
use aws_smithy_runtime_api::client::dns::{DnsFuture, ResolveDns, SharedDnsResolver};
use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnector, HttpConnectorSettings};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;

const PORT: u16 = 8891;

/// Minimal fixed-address [`ResolveDns`] that points the client at a single IP
/// regardless of the requested hostname. In these tests the IP is the one
/// turmoil assigned to the `"server"` host (via [`turmoil::lookup`]).
#[derive(Debug, Clone)]
struct StaticResolver(IpAddr);

impl ResolveDns for StaticResolver {
    fn resolve_dns<'a>(&'a self, _name: &'a str) -> DnsFuture<'a> {
        let ip = self.0;
        DnsFuture::new(async move { Ok(vec![ip]) })
    }
}

/// Runtime components carrying a real Tokio sleep impl so connect/read timeouts
/// fire against turmoil's (paused) simulated clock.
fn runtime_components() -> aws_smithy_runtime_api::client::runtime_components::RuntimeComponents {
    RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(SystemTimeSource::new()))
        .with_sleep_impl(Some(TokioSleep::new()))
        .build()
        .unwrap()
}

/// A request driven through `turmoil_client` traverses the simulated network to
/// a turmoil-hosted HTTP/1.1 server and gets a 200 back.
#[test]
fn round_trip_returns_200() {
    let mut sim = turmoil::Builder::new().build();

    sim.host("server", || async move {
        let listener = turmoil::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
        loop {
            let (mut stream, _) = listener.accept().await?;
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                // Read the request head; we don't need the body for this test.
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let response =
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
                let _ = stream.write_all(response).await;
                let _ = stream.flush().await;
            });
        }
    });

    sim.client("client", async move {
        // Resolve the address turmoil assigned to the server host.
        let server_ip = turmoil::lookup("server");
        let client = turmoil_client(SharedDnsResolver::new(StaticResolver(server_ip)), PORT);
        let settings = HttpConnectorSettings::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(5))
            .build();
        let connector = client.http_connector(&settings, &runtime_components());
        let response = connector
            .call(HttpRequest::get("http://server").unwrap())
            .await
            .expect("request should succeed");
        assert_eq!(response.status().as_u16(), 200);
        Ok(())
    });

    sim.run().unwrap();
}

/// When the network to the server is severed, the client surfaces a connect
/// timeout (proving the real timeout/error-classification stack runs over the
/// simulated transport).
#[test]
fn connect_timeout_is_classified() {
    let mut sim = turmoil::Builder::new().build();

    // The server binds and loops on accept, but the link is held below, so the
    // TCP connect never reaches it. The failure comes purely from the network
    // hold, which is what makes the client's connect timeout fire.
    sim.host("server", || async move {
        let listener = turmoil::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
        loop {
            let _ = listener.accept().await;
        }
    });

    sim.client("client", async move {
        let server_ip = turmoil::lookup("server");
        // Sever the link so the TCP connect handshake can never complete.
        turmoil::hold("client", "server");
        let client = turmoil_client(SharedDnsResolver::new(StaticResolver(server_ip)), PORT);
        let settings = HttpConnectorSettings::builder()
            .connect_timeout(Duration::from_millis(500))
            .build();
        let connector = client.http_connector(&settings, &runtime_components());
        let err = connector
            .call(HttpRequest::get("http://server").unwrap())
            .await
            .expect_err("connect should time out");
        assert!(err.is_timeout(), "expected a connect timeout, got: {err:?}");
        Ok(())
    });

    sim.run().unwrap();
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};

use aws_smithy_http_client::{Builder, tls};
use tokio::time::timeout;

use pokemon_service_client::{Client, Config};
use pokemon_service_common::ChildDrop;
use pokemon_service_tls::{DEFAULT_DOMAIN, DEFAULT_TEST_CERT};

pub struct ServerHandle {
    pub child: ChildDrop,
    pub port: u16,
}

pub async fn run_server() -> ServerHandle {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("pokemon-service-tls"))
        .args(["--port", "0"]) // Use port 0 for random available port
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Wait for the server to signal it's ready by reading stderr
    let stderr = child.stderr.take().unwrap();
    let ready_signal = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                if let Some(port_str) = line.strip_prefix("SERVER_READY:") {
                    if let Ok(port) = port_str.parse::<u16>() {
                        return Some(port);
                    }
                }
            }
        }
        None
    });

    // Wait for the ready signal with a timeout
    let port = match timeout(Duration::from_secs(5), ready_signal).await {
        Ok(Ok(Some(port))) => port,
        _ => {
            panic!("Server did not become ready within 5 seconds");
        }
    };

    ServerHandle {
        child: ChildDrop(child),
        port,
    }
}

// Returns a client that only talks through https and http2 connections.
// It is useful in testing whether our server can talk to http2.
pub fn client_http2_only(port: u16) -> Client {
    // Create custom cert store and add our test certificate to prevent unknown cert issues.
    let cert_pem = std::fs::read(DEFAULT_TEST_CERT).expect("could not open certificate");

    let trust_store = tls::TrustStore::empty()
        .with_native_roots(false)
        .with_pem_certificate(cert_pem);

    let tls_context = tls::TlsContext::builder()
        .with_trust_store(trust_store)
        .build()
        .expect("failed to build TLS context");

    let http_client = Builder::new()
        .tls_provider(tls::Provider::Rustls(tls::rustls_provider::CryptoMode::AwsLc))
        .tls_context(tls_context)
        .build_https();

    let config = Config::builder()
        .http_client(http_client)
        .endpoint_url(format!("https://{DEFAULT_DOMAIN}:{port}"))
        .build();
    Client::from_conf(config)
}

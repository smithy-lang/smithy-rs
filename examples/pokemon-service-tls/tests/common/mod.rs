/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{fs::File, io::BufReader, process::Command, time::Duration};

use assert_cmd::prelude::*;
use aws_smithy_client::hyper_ext::Adapter;
use tokio::time::sleep;

use pokemon_service_client::{Client, Config};
use pokemon_service_common::ChildDrop;
use pokemon_service_tls::{DEFAULT_DOMAIN, DEFAULT_PORT, DEFAULT_TEST_CERT};

pub async fn run_server() -> ChildDrop {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();
    let child = Command::cargo_bin(crate_name).unwrap().spawn().unwrap();

    sleep(Duration::from_millis(500)).await;

    ChildDrop(child)
}

// Returns a client that only talks through https and http2 connections.
// It is useful in testing whether our server can talk to http2.
pub fn client_http2_only() -> Client {
    // Create custom cert store and add our test certificate to prevent unknown cert issues.
    let mut reader =
        BufReader::new(File::open(DEFAULT_TEST_CERT).expect("could not open certificate"));
    let certs = rustls_pemfile::certs(&mut reader).expect("could not parse certificate");
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.add_parsable_certificates(&certs);

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(
            tokio_rustls::rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
        .https_only()
        .enable_http2()
        .build();

    let config = Config::builder()
        .http_connector(Adapter::builder().build(connector))
        .endpoint_url(format!("https://{DEFAULT_DOMAIN}:{DEFAULT_PORT}"))
        .build();
    Client::from_conf(config)
}

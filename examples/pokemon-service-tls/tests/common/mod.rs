/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{fs::File, io::BufReader, process::Command, str::FromStr, time::Duration};

use assert_cmd::prelude::*;
use aws_smithy_client::{
    erase::{DynConnector, DynMiddleware},
    hyper_ext::Adapter,
};
use hyper::http::uri::{Authority, Scheme};
use tokio::time::sleep;

use pokemon_service_client::{Builder, Client, Config};
use pokemon_service_common::{rewrite_base_url, ChildDrop};
use pokemon_service_tls::{DEFAULT_DOMAIN, DEFAULT_PORT, DEFAULT_TEST_CERT};

pub async fn run_server() -> ChildDrop {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();
    let child = Command::cargo_bin(crate_name).unwrap().spawn().unwrap();

    sleep(Duration::from_millis(500)).await;

    ChildDrop(child)
}

// Returns a client that only talks through https and http2 connections.
// It is useful in testing whether our server can talk to http2.
pub fn client_http2_only() -> Client<DynConnector, DynMiddleware<DynConnector>> {
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

    let authority = Authority::from_str(&format!("{DEFAULT_DOMAIN}:{DEFAULT_PORT}"))
        .expect("could not parse authority");
    let raw_client = Builder::new()
        .connector(DynConnector::new(Adapter::builder().build(connector)))
        .middleware_fn(rewrite_base_url(Scheme::HTTPS, authority))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}

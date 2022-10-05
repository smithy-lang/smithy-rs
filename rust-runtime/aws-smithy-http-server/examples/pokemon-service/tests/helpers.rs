/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fs::File;
use std::io::BufReader;
use std::process::{Child, Command};
use std::time::Duration;

use assert_cmd::prelude::*;
use aws_smithy_client::{erase::DynConnector, hyper_ext::Adapter};
use aws_smithy_http::operation::Request;
use pokemon_service_client::{Builder, Client, Config};
use tokio::time;

const TEST_KEY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.key");
const TEST_CERT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.crt");

enum PokemonServiceVariant {
    Http,
    Https,
}

impl PokemonServiceVariant {
    async fn run_process(&self) -> Child {
        let process = match self {
            PokemonServiceVariant::Http => Command::cargo_bin("pokemon-service").unwrap().spawn().unwrap(),
            PokemonServiceVariant::Https => Command::cargo_bin("pokemon-service-tls")
                .unwrap()
                .args(["--tls-cert-path", TEST_CERT, "--tls-key-path", TEST_KEY])
                .spawn()
                .unwrap(),
        };

        // Give PokémonService some time to start up.
        time::sleep(Duration::from_millis(500)).await;

        process
    }

    fn base_url(&self) -> String {
        match self {
            PokemonServiceVariant::Http => "http://localhost:13734".to_string(),
            PokemonServiceVariant::Https => "https://localhost:13734".to_string(),
        }
    }
}

pub(crate) struct PokemonService {
    child_process: Child,
}

impl PokemonService {
    #[allow(dead_code)]
    pub(crate) async fn run() -> Self {
        Self {
            child_process: PokemonServiceVariant::Http.run_process().await,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn run_https() -> Self {
        Self {
            child_process: PokemonServiceVariant::Https.run_process().await,
        }
    }
}

impl Drop for PokemonService {
    fn drop(&mut self) {
        self.child_process
            .kill()
            .expect("failed to kill Pokémon Service program")
    }
}

#[allow(dead_code)]
pub fn client() -> Client<
    aws_smithy_client::erase::DynConnector,
    aws_smithy_client::erase::DynMiddleware<aws_smithy_client::erase::DynConnector>,
> {
    let base_url = PokemonServiceVariant::Http.base_url();
    let raw_client = Builder::new()
        .rustls_connector(Default::default())
        .middleware_fn(rewrite_base_url(base_url))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}

// Returns a client that only talks through https and http2 connections.
// It is useful in testing whether our server can talk to http2.
#[allow(dead_code)]
pub fn client_http2_only() -> Client<
    aws_smithy_client::erase::DynConnector,
    aws_smithy_client::erase::DynMiddleware<aws_smithy_client::erase::DynConnector>,
> {
    // Create custom cert store and add our test certificate to prevent unknown cert issues.
    let mut reader = BufReader::new(File::open(TEST_CERT).expect("could not open certificate"));
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

    let base_url = PokemonServiceVariant::Https.base_url();
    let raw_client = Builder::new()
        .connector(DynConnector::new(Adapter::builder().build(connector)))
        .middleware_fn(rewrite_base_url(base_url))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}

fn rewrite_base_url(base_url: String) -> impl Fn(Request) -> Request + Clone {
    move |mut req| {
        let http_req = req.http_mut();
        let uri = format!("{base_url}{}", http_req.uri().path());
        *http_req.uri_mut() = uri.parse().unwrap();
        req
    }
}

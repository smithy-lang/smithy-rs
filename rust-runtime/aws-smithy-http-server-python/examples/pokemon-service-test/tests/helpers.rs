/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fs::File;
use std::io::BufReader;
use std::process::Command;
use std::time::Duration;

use aws_smithy_client::{erase::DynConnector, hyper_ext::Adapter};
use aws_smithy_http::operation::Request;
use command_group::{CommandGroup, GroupChild};
use pokemon_service_client::{Builder, Client, Config};
use tokio::time;

const TEST_KEY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.key");
const TEST_CERT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testdata/localhost.crt");

pub type PokemonClient = Client<
    aws_smithy_client::erase::DynConnector,
    aws_smithy_client::erase::DynMiddleware<aws_smithy_client::erase::DynConnector>,
>;

enum PokemonServiceVariant {
    Http,
    Http2,
}

impl PokemonServiceVariant {
    async fn run_process(&self) -> GroupChild {
        let mut args = vec!["../pokemon_service.py".to_string()];

        match self {
            PokemonServiceVariant::Http => {}
            PokemonServiceVariant::Http2 => {
                args.push("--enable-tls".to_string());
                args.push(format!("--tls-key-path={TEST_KEY}"));
                args.push(format!("--tls-cert-path={TEST_CERT}"));
            }
        }

        let process = Command::new("python3")
            .args(args)
            .group_spawn()
            .expect("failed to spawn the Pokémon Service program");

        // The Python interpreter takes a little to startup.
        time::sleep(Duration::from_secs(2)).await;

        process
    }

    fn base_url(&self) -> &'static str {
        match self {
            PokemonServiceVariant::Http => "http://localhost:13734",
            PokemonServiceVariant::Http2 => "https://localhost:13734",
        }
    }
}

pub(crate) struct PokemonService {
    // We need to ensure all processes forked by the Python interpreter
    // are on the same process group, otherwise only the main process
    // will be killed during drop, leaving the test worker alive.
    child_process: GroupChild,
}

impl PokemonService {
    #[allow(dead_code)]
    pub(crate) async fn run() -> Self {
        Self {
            child_process: PokemonServiceVariant::Http.run_process().await,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn run_http2() -> Self {
        Self {
            child_process: PokemonServiceVariant::Http2.run_process().await,
        }
    }
}

impl Drop for PokemonService {
    fn drop(&mut self) {
        self.child_process
            .kill()
            .expect("failed to kill Pokémon Service program");
        self.child_process.wait().ok();
    }
}

#[allow(dead_code)]
pub fn client() -> PokemonClient {
    let base_url = PokemonServiceVariant::Http.base_url();
    let raw_client = Builder::new()
        .rustls_connector(Default::default())
        .middleware_fn(rewrite_base_url(base_url))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}

#[allow(dead_code)]
pub fn http2_client() -> PokemonClient {
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

    let base_url = PokemonServiceVariant::Http2.base_url();
    let raw_client = Builder::new()
        .connector(DynConnector::new(Adapter::builder().build(connector)))
        .middleware_fn(rewrite_base_url(base_url))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}

fn rewrite_base_url(base_url: &'static str) -> impl Fn(Request) -> Request + Clone {
    move |mut req| {
        let http_req = req.http_mut();
        let uri = format!("{base_url}{}", http_req.uri().path());
        *http_req.uri_mut() = uri.parse().unwrap();
        req
    }
}

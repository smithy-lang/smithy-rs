/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon-service-tls`.
// It uses `tls-listener`, `tokio-rustls` (and `rustls-pemfile` to parse PEM files)
// to serve TLS connections. It also enables h2 ALPN protocol,
// without this clients by default don't upgrade to http2.
//
// You can use `mkcert` (https://github.com/FiloSottile/mkcert) to create certificates for testing:
// `$ mkcert localhost`
// it should create `./localhost.pem` and `./localhost-key.pem`,
// then you can run TLS server via:
// `$ cargo run --bin pokemon-service-tls -- --tls-cert-path ./localhost.pem --tls-key-path ./localhost-key.pem`
// and test it:
// ```bash
// $ curl -k -D- -H "Accept: application/json" https://localhost:13734/pokemon-species/pikachu
// HTTP/2 200
// # ...
// ```
// note that by default created certificates will be unknown and you should use `-k|--insecure`
// flag while making requests with cURL or you can run `mkcert -install` to trust certificates created by `mkcert`.

use std::{fs::File, future, io::BufReader, net::SocketAddr, sync::Arc};

use clap::Parser;
use futures_util::stream::StreamExt;
use tokio_rustls::{
    rustls::{Certificate, PrivateKey, ServerConfig},
    TlsAcceptor,
};

use pokemon_service_common::{
    capture_pokemon, check_health, get_pokemon_species, get_server_statistics, get_storage,
    setup_tracing, stream_pokemon_radio, State,
};
use pokemon_service_server_sdk_http0x::{
    input, output,
    server::{request::connect_info::ConnectInfo, routing::Connected, AddExtensionLayer},
    PokemonService, PokemonServiceConfig,
};
use pokemon_service_tls::{DEFAULT_ADDRESS, DEFAULT_PORT, DEFAULT_TEST_CERT, DEFAULT_TEST_KEY};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = DEFAULT_ADDRESS)]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Hyper server TLS certificate path. Must be a PEM file.
    #[clap(long, default_value = DEFAULT_TEST_CERT)]
    tls_cert_path: String,
    /// Hyper server TLS private key path. Must be a PEM file.
    #[clap(long, default_value = DEFAULT_TEST_KEY)]
    tls_key_path: String,
}

/// Information derived from the TLS connection.
#[derive(Debug, Clone)]
pub struct TlsConnectInfo {
    /// The remote peer address of this connection.
    pub socket_addr: SocketAddr,

    /// The set of TLS certificates presented by the peer in this connection.
    pub certs: Option<Arc<Vec<Certificate>>>,
}

impl Connected<&tokio_rustls::server::TlsStream<hyper::server::conn::AddrStream>>
    for TlsConnectInfo
{
    fn connect_info(
        target: &tokio_rustls::server::TlsStream<hyper::server::conn::AddrStream>,
    ) -> Self {
        let (addr_stream, session) = target.get_ref();
        let socket_addr = addr_stream.remote_addr();

        let certs = session
            .peer_certificates()
            .map(|certs| Arc::new(certs.to_vec()));

        TlsConnectInfo { socket_addr, certs }
    }
}

/// Empty operation used to showcase how we can get access to information derived from the TLS
/// connection in.
pub async fn do_nothing_with_tls_connect_info(
    _input: input::DoNothingInput,
    ConnectInfo(tls_connect_info): ConnectInfo<TlsConnectInfo>,
) -> output::DoNothingOutput {
    // Logging these might pose a security concern! You probably don't want to do this in
    // production.
    tracing::debug!(?tls_connect_info.certs, "peer TLS certificates");

    output::DoNothingOutput {}
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();
    setup_tracing();

    let config = PokemonServiceConfig::builder()
        // Set up shared state and middlewares.
        .layer(AddExtensionLayer::new(Arc::new(State::default())))
        .build();
    let app = PokemonService::builder(config)
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing_with_tls_connect_info)
        .check_health(check_health)
        .stream_pokemon_radio(stream_pokemon_radio)
        .build()
        .expect("failed to build an instance of PokemonService");

    let addr: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");

    let acceptor = acceptor(&args.tls_cert_path, &args.tls_key_path);
    let listener = tls_listener::TlsListener::new(
        acceptor,
        hyper::server::conn::AddrIncoming::bind(&addr).expect("could not bind"),
    )
    .connections()
    .filter(|conn| {
        if let Err(err) = conn {
            eprintln!("connection error: {err:?}");
            future::ready(false)
        } else {
            future::ready(true)
        }
    });
    // Using `into_make_service_with_connect_info`, rather than `into_make_service`, to adjoin the `TlsConnectInfo`
    // connection info.
    let make_app = app.into_make_service_with_connect_info::<TlsConnectInfo>();
    let server =
        hyper::Server::builder(hyper::server::accept::from_stream(listener)).serve(make_app);
    if let Err(err) = server.await {
        eprintln!("server error: {err}");
    }
}

// Returns a `TlsAcceptor` that can be used to create `TlsListener`
// which then can be used with Hyper.
pub fn acceptor(cert_path: &str, key_path: &str) -> TlsAcceptor {
    let certs = load_certs(cert_path);
    let key = load_key(key_path);
    let mut server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("could not create server config");

    // If we don't state we are accepting "h2", clients by default don't negotiate way up to http2.
    server_config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    TlsAcceptor::from(Arc::new(server_config))
}

fn load_certs(path: &str) -> Vec<Certificate> {
    let mut reader = BufReader::new(File::open(path).expect("could not open certificate"));
    rustls_pemfile::certs(&mut reader)
        .expect("could not parse certificate")
        .into_iter()
        .map(Certificate)
        .collect()
}

fn load_key(path: &str) -> PrivateKey {
    let mut reader = BufReader::new(File::open(path).expect("could not open private key"));
    loop {
        match rustls_pemfile::read_one(&mut reader).expect("could not parse private key") {
            Some(rustls_pemfile::Item::RSAKey(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::ECKey(key)) => return PrivateKey(key),
            None => break,
            _ => {}
        }
    }
    panic!("invalid private key")
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon_service`.
use std::future;
use std::{net::SocketAddr, sync::Arc};

use aws_smithy_http_server::{AddExtensionLayer, Router};
use clap::Parser;
use futures_util::stream::StreamExt;
use pokemon_service::{
    capture_pokemon, empty_operation, get_pokemon_species, get_server_statistics, get_storage, health_check_operation,
    setup_tracing, State,
};
use pokemon_service_server_sdk::operation_registry::OperationRegistryBuilder;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

mod tls;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = "127.0.0.1")]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value = "13734")]
    port: u16,
    /// Hyper server whether to use TLS.
    #[clap(long, action)]
    use_tls: bool,
    /// Hyper server TLS certificate path. Must be a PEM file.
    #[clap(long, default_value = "")]
    tls_cert_path: String,
    /// Hyper server TLS private key path. Must be a PEM file.
    #[clap(long, default_value = "")]
    tls_key_path: String,
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();
    setup_tracing();
    let app: Router = OperationRegistryBuilder::default()
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon_operation(capture_pokemon)
        .empty_operation(empty_operation)
        .health_check_operation(health_check_operation)
        .build()
        .expect("Unable to build operation registry")
        // Convert it into a router that will route requests to the matching operation
        // implementation.
        .into();

    // Setup shared state and middlewares.
    let shared_state = Arc::new(State::default());
    let app = app.layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(AddExtensionLayer::new(shared_state)),
    );

    let addr: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");

    if args.use_tls {
        run_tls_server(app, args, &addr).await;
    } else {
        run_server(app, &addr).await;
    }
}

async fn run_server(app: Router, addr: &SocketAddr) {
    let server = hyper::Server::bind(addr).serve(app.into_make_service());
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

async fn run_tls_server(app: Router, args: Args, addr: &SocketAddr) {
    let acceptor = tls::acceptor(&args.tls_cert_path, &args.tls_key_path);
    let listener = tls_listener::TlsListener::new(
        acceptor,
        hyper::server::conn::AddrIncoming::bind(addr).expect("could not bind"),
    )
    .filter(|conn| {
        if let Err(err) = conn {
            eprintln!("connection error: {:?}", err);
            future::ready(false)
        } else {
            future::ready(true)
        }
    });
    let server = hyper::Server::builder(hyper::server::accept::from_stream(listener)).serve(app.into_make_service());
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

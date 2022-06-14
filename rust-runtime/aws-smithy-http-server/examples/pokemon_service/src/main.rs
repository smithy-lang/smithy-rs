/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon_service`.
use std::{net::SocketAddr, sync::Arc};

use aws_smithy_http_server::{AddExtensionLayer, Router};
use clap::Parser;
use pokemon_service::{empty_operation, get_pokemon_species, get_server_statistics, setup_tracing, State};
use pokemon_service_sdk::operation_registry::OperationRegistryBuilder;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = "127.0.0.1")]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value = "13734")]
    port: u16,
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
        .get_server_statistics(get_server_statistics)
        .empty_operation(empty_operation)
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

    // Start the [`hyper::Server`].
    let bind: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind).serve(app.into_make_service());

    // Run forever-ish...
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

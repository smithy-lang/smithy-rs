/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon-service`.
use std::{net::SocketAddr, sync::Arc};

use aws_smithy_http_server::{operation::OperationShapeExt, AddExtensionLayer, Router};
use clap::Parser;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use pokemon_service::{
    capture_pokemon, empty_operation, get_pokemon_species, get_server_statistics, get_storage, health_check_operation,
    setup_tracing, State,
};
use pokemon_service_server_sdk::operation_registry::OperationRegistryBuilder;
use pokemon_service_server_sdk::operation_shape::GetPokemonSpecies;
use pokemon_service_server_sdk::service::PokemonService;

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

    // Old builder
    let _app: Router = OperationRegistryBuilder::default()
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

    // New builder
    let get_pokemon_species = GetPokemonSpecies::from_handler(get_pokemon_species);
    let app = PokemonService::builder()
        .get_pokemon_species_operation(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon_operation(capture_pokemon)
        .empty_operation(empty_operation)
        .health_check_operation(health_check_operation)
        .build();

    // Unchecked build does not type check whether or not an operation is set but will panic at runtime.
    let _ = PokemonService::unchecked_builder().build::<hyper::Body>();

    // Setup shared state and middlewares.
    let shared_state = Arc::new(State::default());
    let app = app.layer(
        &ServiceBuilder::new()
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

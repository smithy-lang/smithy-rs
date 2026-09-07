/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::Arc;

use pokemon_service_common::{
    capture_pokemon, check_health, do_nothing, get_pokemon_species, get_server_statistics,
    setup_tracing, stream_pokemon_radio, State,
};
use pokemon_service_lambda::get_storage_lambda;
use pokemon_service_server_sdk::{
    server::{routing::LambdaHandler, AddExtensionLayer},
    PokemonService, PokemonServiceConfig,
};

#[tokio::main]
pub async fn main() {
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
        .get_storage(get_storage_lambda)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        .stream_pokemon_radio(stream_pokemon_radio)
        .build()
        .expect("failed to build an instance of PokemonService");

    let handler = LambdaHandler::new(app);
    let lambda = lambda_http::run(handler);

    if let Err(err) = lambda.await {
        eprintln!("lambda error: {err}");
    }
}

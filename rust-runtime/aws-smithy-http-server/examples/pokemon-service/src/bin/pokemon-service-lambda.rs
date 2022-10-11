/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon-service-lambda`.
use std::sync::Arc;

use aws_smithy_http_server::{routing::LambdaHandler, AddExtensionLayer};
use pokemon_service::{
    capture_pokemon, check_health, do_nothing, get_pokemon_species, get_server_statistics, get_storage,
    plugin::PrintExt, setup_tracing, State,
};
use pokemon_service_server_sdk::service::PokemonService;

#[tokio::main]
pub async fn main() {
    setup_tracing();

    let app = PokemonService::builder()
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        // Apply the `PrintPlugin` defined in `plugin.rs`
        .print()
        .build()
        // Setup shared state and middlewares.
        .layer(&AddExtensionLayer::new(Arc::new(State::default())));

    let handler = LambdaHandler::new(app);
    let lambda = lambda_http::run(handler);

    if let Err(err) = lambda.await {
        eprintln!("lambda error: {}", err);
    }
}

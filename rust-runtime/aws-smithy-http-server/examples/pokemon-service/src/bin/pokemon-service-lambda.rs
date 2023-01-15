/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This program is exported as a binary named `pokemon-service-lambda`.
use std::sync::Arc;

use aws_smithy_http_server::{
    plugin::PluginPipeline, request::lambda::Context, routing::LambdaHandler, AddExtensionLayer, Extension,
};
use pokemon_service::{
    capture_pokemon, check_health, do_nothing, get_pokemon_species, get_server_statistics, plugin::PrintExt,
    setup_tracing, State,
};
use pokemon_service_server_sdk::{error, input, output, PokemonService};

/// Retrieves the user's storage and records the .
pub async fn get_storage_lambda(
    input: input::GetStorageInput,
    _state: Extension<Arc<State>>,
    context: Context,
) -> Result<output::GetStorageOutput, error::GetStorageError> {
    tracing::debug!(request_id = %context.request_id, "attempting to authenticate storage user");

    // We currently only support Ash and he has nothing stored
    if !(input.user == "ash" && input.passcode == "pikachu123") {
        tracing::debug!("authentication failed");
        return Err(error::GetStorageError::StorageAccessNotAuthorized(
            error::StorageAccessNotAuthorized {},
        ));
    }
    Ok(output::GetStorageOutput { collection: vec![] })
}

#[tokio::main]
pub async fn main() {
    setup_tracing();
    // Apply the `PrintPlugin` defined in `plugin.rs`
    let plugins = PluginPipeline::new().print();
    let app = PokemonService::builder_with_plugins(plugins)
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage_lambda)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        .build()
        .expect("failed to build an instance of PokemonService")
        // Set up shared state and middlewares.
        .layer(&AddExtensionLayer::new(Arc::new(State::default())));

    let handler = LambdaHandler::new(app);
    let lambda = lambda_http::run(handler);

    if let Err(err) = lambda.await {
        eprintln!("lambda error: {}", err);
    }
}

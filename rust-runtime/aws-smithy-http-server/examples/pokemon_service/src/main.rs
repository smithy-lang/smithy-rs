/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This program is exported as a binary named `pokemon_service`.
use std::sync::Arc;

use aws_smithy_http_server::{AddExtensionLayer, Router};
use pokemon_service::{empty_operation, get_pokemon_species, get_server_statistics, setup_tracing, State};
use pokemon_service_sdk::operation_registry::OperationRegistryBuilder;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

#[tokio::main]
pub async fn main() {
    setup_tracing();
    let app: Router<_> = OperationRegistryBuilder::default()
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
    let server = hyper::Server::bind(&"0.0.0.0:13734".parse().unwrap()).serve(app.into_make_service());

    // Run forever-ish...
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

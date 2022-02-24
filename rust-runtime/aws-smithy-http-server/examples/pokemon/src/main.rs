/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This program is exported as a binary named pokemon-service
use std::{env, sync::Arc};

use aws_smithy_http_server::{AddExtensionLayer, Router};
use pokemon_service::{get_pokemon_species, get_server_statistics, setup_tracing, State};
use pokemon_service_sdk::operation_registry::OperationRegistryBuilder;
use tower::ServiceBuilder;
use tower_http::{
    trace::{
        DefaultMakeSpan, DefaultOnBodyChunk, DefaultOnEos, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse,
        TraceLayer,
    },
    LatencyUnit,
};
use tracing::Level;

#[tokio::main]
pub async fn main() {
    setup_tracing();
    let app: Router = OperationRegistryBuilder::default()
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_server_statistics(get_server_statistics)
        .build()
        .expect("Unable to build operation registry")
        // Convert it into a router that will route requests to the matching operation
        // implementation.
        .into();

    let log_level = env::var("RUST_LOG")
        .map(|x| if x.contains("debug") { Level::DEBUG } else { Level::INFO })
        .unwrap_or(Level::INFO);
    let shared_state = Arc::new(State::new());
    let app = app.layer(
        ServiceBuilder::new()
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().level(log_level).include_headers(true))
                    .on_request(DefaultOnRequest::new().level(log_level))
                    .on_response(
                        DefaultOnResponse::new()
                            .level(log_level)
                            .latency_unit(LatencyUnit::Millis),
                    )
                    .on_body_chunk(DefaultOnBodyChunk::new())
                    .on_eos(DefaultOnEos::new().level(log_level).latency_unit(LatencyUnit::Millis))
                    .on_failure(
                        DefaultOnFailure::new()
                            .level(log_level)
                            .latency_unit(LatencyUnit::Millis),
                    ),
            )
            .layer(AddExtensionLayer::new(shared_state)),
    );

    // Start the [`axum server`]. Note that also an [`hyper` server`] can be used.
    //
    // [`axum server`]: [`axum::Server`]
    // [`hyper server`]: [`hyper::Server`]
    let server = hyper::Server::bind(&"0.0.0.0:13734".parse().unwrap()).serve(app.into_make_service());

    // Run forever-ish...
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

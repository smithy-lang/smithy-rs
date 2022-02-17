/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This program is exported as a binary named smithy-rs-server-sdk-pokemon-service.
//
// See Cargo.toml for how it gets its name, and module.mk for how it ends up in build/bin.
//
// You can run the program (after running `brazil-build`) using:
//
// ```console
// $ brazil-runtime-exec smithy-rs-server-sdk-pokemon-service
// ```

use std::{env, sync::Arc};

use aws_smithy_http_server::{AddExtensionLayer, Router};
use pokemon::{get_pokemon_species, get_server_statistics, setup_tracing, State};
use pokemon_sdk::operation_registry::OperationRegistryBuilder;
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

    let log_level = match env::var("RUST_LOG") {
        Ok(log) => match log.as_str() {
            "debug" => Level::DEBUG,
            "error" => Level::ERROR,
            "warn" => Level::WARN,
            _ => Level::INFO,
        },
        Err(_) => Level::INFO,
    };
    let shared_state = Arc::new(State::default());
    let app = app.layer(
        ServiceBuilder::new()
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().level(log_level))
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

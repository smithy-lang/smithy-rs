/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a `smithy-rs` client, and call an operation with custom
/// headers in the request.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example custom-header`
///
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

/// Creates a new `smithy-rs` client that is configured to communicate with a locally running Pokémon
/// service on TCP port 13734.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// let client = create_client();
/// ```
fn create_client() -> PokemonClient {
    // The generated client has a type `Config::Builder` that can be used to build a `Config`, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .build();

    // Apply the configuration on the client, and return that.
    pokemon_service_client::Client::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured `smithy-rs` client.
    let client = create_client();

    // Call an operation `get_server_statistics` on the Pokémon service.
    let response = client
        .get_server_statistics()
        .customize()
        .mutate_request(|req| {
            // For demonstration purposes, add a header `x-ttl-seconds` to the outgoing request.
            let headers = req.headers_mut();
            headers.insert(
                hyper::header::HeaderName::from_static("x-ttl-seconds"),
                hyper::header::HeaderValue::from(30),
            );
        })
        .send()
        .await
        .expect("operation failed");

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

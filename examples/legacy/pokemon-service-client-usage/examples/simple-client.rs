/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a `smithy-rs` Client and call an
/// [operation](https://smithy.io/2.0/spec/idl.html?highlight=operation#operation-shape).
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example simple-client`.
///
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

/// Creates a new `smithy-rs` client that is configured to communicate with a locally running Pokémon
/// service on TCP port 13734.
///
/// # Examples
///
/// Basic usage:
/// ```
/// let client = create_client();
/// ```
fn create_client() -> PokemonClient {
    // The generated client contains a type `config::Builder` for constructing a `Config` instance.
    // This enables configuration of endpoint resolvers, timeouts, retries, etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .build();

    // Instantiate a client by applying the configuration.
    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured `smithy-rs` client.
    let client = create_client();

    // Call an operation `get_server_statistics` on the Pokémon service.
    let response = client
        .get_server_statistics()
        .send()
        .await
        .expect("operation failed");

    // Print the response received from the service.
    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

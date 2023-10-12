/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a Smithy Client and call an
/// [operation](https://smithy.io/2.0/spec/idl.html?highlight=operation#operation-shape).
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example dyn-client`.
///
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::setup_tracing_subscriber;
use tracing::info;

static BASE_URL: &str = "http://localhost:13734";

/// Creates a new Smithy client that is configured to communicate with a locally running Pokemon
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
        .endpoint_url(BASE_URL)
        .build();

    // Instantiate a client by applying the configuration.
    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured Smithy client.
    let client = create_client();

    // Call an operation `get_server_statistics` on Pokemon service.
    let response = client
        .get_server_statistics()
        .send()
        .await
        .expect("Pokemon service does not seem to be running on localhost:13734");

    // Print the response received from the service.
    info!(%BASE_URL, ?response, "Response received");
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a `smithy-rs` Client and set connection
/// and operation related timeouts on the client.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example timeout-config`
///
use std::time::Duration;

use pokemon_service_client::{config::timeout::TimeoutConfig, Client as PokemonClient};
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

/// Creates a new `smithy-rs` client that is configured to communicate with a locally running Pokémon service on TCP port 13734.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// let client = create_client();
/// ```
fn create_client() -> PokemonClient {
    // Different type of timeouts can be set on the client. These are:
    // operation_attempt_timeout - If retries are enabled, this represents the timeout
    //    for each individual operation attempt.
    // operation_timeout - Overall timeout for the operation to complete.
    // connect timeout - The amount of time allowed for a connection to be established.
    let timeout_config = TimeoutConfig::builder()
        .operation_attempt_timeout(Duration::from_secs(1))
        .operation_timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_millis(500))
        .build();

    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .timeout_config(timeout_config)
        .build();

    // Apply the configuration on the client, and return that.
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
        .expect("Pokemon service does not seem to be running on localhost:13734");

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to customize retry settings on a Smithy client.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example retry-customize`.
///
use aws_smithy_types::retry::RetryConfig;
use pokemon_service_client_usage::setup_tracing_subscriber;
use std::time::Duration;
use tracing::info;

use pokemon_service_client::Client as PokemonClient;

static BASE_URL: &str = "http://localhost:13734";

/// Creates a new Smithy client that is configured to communicate with a locally running Pokemon service on TCP port 13734.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// let client = create_client();
/// ```
fn create_client() -> PokemonClient {
    // By default the Smithy client uses RetryConfig::standard() strategy, with 3 retries, and
    // an initial exponential back off of 1 second. To turn it off use RetryConfig::disabled().
    let retry_config = RetryConfig::standard()
        .with_initial_backoff(Duration::from_secs(3))
        .with_max_attempts(5);

    // The generated client has a type config::Builder that can be used to build a Config, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .retry_config(retry_config)
        .sleep_impl(::aws_smithy_async::rt::sleep::SharedAsyncSleep::new(
            aws_smithy_async::rt::sleep::default_async_sleep()
                .expect("sleep implementation could not be created"),
        ))
        .build();

    // Apply the configuration on the Client, and return that.
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

    info!(%BASE_URL, ?response, "Response received");
}

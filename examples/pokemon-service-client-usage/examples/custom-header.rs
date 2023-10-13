/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a Smithy Client, and call an operation with custom
/// headers in the request.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example custom-header`
///
use tracing::info;

use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};

static BASE_URL: &str = "http://localhost:13734";

/// Creates a new Smithy client that is configured to communicate with a locally running Pokemon
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
    // The generated client has a type config::Builder that can be used to build a Config, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .build();

    // Apply the configuration on the Client, and return that.
    pokemon_service_client::Client::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured Smithy client.
    let client = create_client();

    // Call an operation `get_server_statistics` on Pokemon service.
    let request = client.get_server_statistics().customize();

    // Mutate the request, then insert header name / value pair to mutated header collection.
    let response = request
        .mutate_request(|req| {
            let headers = req.headers_mut();
            headers.insert(
                hyper::header::HeaderName::from_static("x-ttl-seconds"),
                hyper::header::HeaderValue::from(30),
            );
        })
        .send()
        .await
        .custom_expect_and_log("get_server_statistics failed");

    info!(%BASE_URL, ?response, "Response received");
}

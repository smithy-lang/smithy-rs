/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how a custom `ResolveEndpoint` can be implemented for resolving
/// endpoint of a request. Additionally, it shows how a header can be added using the endpoint
/// builder.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example endpoint-resolver`.
///
use pokemon_service_client::config::endpoint::{Endpoint, EndpointFuture, Params, ResolveEndpoint};
use pokemon_service_client::primitives::{DateTime, DateTimeFormat};
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::setup_tracing_subscriber;

use std::time::SystemTime;

// This struct, provided as an example, constructs the URL that should be set on each request during initialization.
// It also implements the `ResolveEndpoint` trait, enabling it to be assigned as the endpoint_resolver in the `Config`.
#[derive(Debug)]
struct RegionalEndpoint {
    url_to_use: String,
}

impl RegionalEndpoint {
    fn new(regional_url: &str, port: u16) -> Self {
        let url_to_use = format!("{}:{}", regional_url, port);
        RegionalEndpoint { url_to_use }
    }
}

impl ResolveEndpoint for RegionalEndpoint {
    fn resolve_endpoint<'a>(&'a self, _params: &'a Params) -> EndpointFuture<'a> {
        // Construct an endpoint using the Endpoint::Builder. Set the URL and,
        // optionally, any headers to be sent with the request. For this example,
        // we'll set the 'x-amz-date' header to the current date for all outgoing requests.
        // `DateTime` can be used for formatting an RFC 3339 date time.
        let now = SystemTime::now();
        let date_time = DateTime::from(now);

        let endpoint = Endpoint::builder()
            .url(self.url_to_use.clone())
            .header(
                "x-amz-date",
                date_time
                    .fmt(DateTimeFormat::DateTimeWithOffset)
                    .expect("Could not create a date in UTC format"),
            )
            .build();
        tracing::info!(?endpoint, "Resolving endpoint");
        EndpointFuture::ready(Ok(endpoint))
    }
}

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
    const DEFAULT_PORT: u16 = 13734;

    // Use the environment variable `REGIONAL_URL` for the URL.
    let resolver = RegionalEndpoint::new(
        std::env::var("REGIONAL_URL")
            .as_deref()
            .unwrap_or("http://localhost"),
        DEFAULT_PORT,
    );

    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(resolver)
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
        .expect("operation failed");

    tracing::info!(?response, "Response received");
}

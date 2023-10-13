/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how a custom ResolveEndpoint can be implemented for resolving
/// endpoint of a request. Additionally, it shows how a header can be added using the endpoint
/// builder.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example endpoint-resolver`.
///
use aws_smithy_http::endpoint::{ResolveEndpoint, ResolveEndpointError};
use aws_smithy_types::{date_time::Format, endpoint::Endpoint};
use pokemon_service_client::config::endpoint::Params;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};
use std::time::SystemTime;
use tracing::info;

// This struct, provided as an example, constructs the URL that should be set on each request during initialization.
// It also implements the ResolveEndpoint trait, enabling it to be assigned as the endpoint_resolver in the Config.
struct RegionalEndpoint {
    url_to_use: String,
}

impl RegionalEndpoint {
    fn new(port: u16, region: &str) -> Self {
        let url_to_use = if region.starts_with("http://") | region.starts_with("https://") {
            format!("{}:{}", region, port)
        } else {
            format!("http://{}:{}", region, port)
        };
        RegionalEndpoint { url_to_use }
    }
}

impl ResolveEndpoint<Params> for RegionalEndpoint {
    fn resolve_endpoint(
        &self,
        _params: &Params,
    ) -> std::result::Result<Endpoint, ResolveEndpointError> {
        // Construct an endpoint using the Endpoint::Builder. Set the URL and,
        // optionally, any headers to be sent with the request. For this example,
        // we'll set the 'x-amz-date' header to the current date for all outgoing requests.
        // `aws_smithy_types::DateTime` can be used for formatting an ISO 8601 date time.
        let now = SystemTime::now();
        let date_time = aws_smithy_types::DateTime::from(now);

        let endpoint = Endpoint::builder()
            .url(self.url_to_use.clone())
            .header(
                "x-amz-date",
                date_time
                    .fmt(Format::DateTimeWithOffset)
                    .expect("Could create a date in UTC format"),
            )
            .build();

        info!(?endpoint, "Resolving endpoint");
        Ok(endpoint)
    }
}

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
    // Use the environment variable AWS_PORT_POKEMON_SERVICE for the service listening port
    // or 13734 as default.
    let port = std::env::var("AWS_PORT_POKEMON_SERVICE")
        .unwrap_or("13734".to_string())
        .parse::<u16>()
        .expect("Could not convert port to a u16");
    let resolver = RegionalEndpoint::new(
        port,
        std::env::var("AWS_DEFAULT_REGION")
            .as_deref()
            .unwrap_or("localhost"),
    );

    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(resolver)
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
        .custom_expect_and_log("get_server_statistics failed");

    info!(?response, "Response received");
}

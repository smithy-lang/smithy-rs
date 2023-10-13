/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to use an Interceptor to set the endpoint on a HTTP request.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example endpoint-using-middleware`.
///
use aws_smithy_types::config_bag::ConfigBag;
use pokemon_service_client::{
    config::{interceptors::BeforeTransmitInterceptorContextMut, Interceptor, RuntimeComponents},
    error::BoxError,
};
use pokemon_service_client_usage::setup_tracing_subscriber;
use tracing::info;

use pokemon_service_client::Client as PokemonClient;

// URL where example Pokemon service is running.
static BASE_URL: &str = "http://localhost:13734";

#[derive(Debug, Default)]
pub struct EndpointInterceptor;

impl EndpointInterceptor {
    /// Creates a new `EndpontInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for EndpointInterceptor {
    fn name(&self) -> &'static str {
        "EndpointInterceptor"
    }

    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // This interceptor checks the environment variable URL_TO_USE
        // to determine if it should override the request's set URL.
        if let Ok(service_url) = std::env::var("URL_TO_USE") {
            // Get a mutable reference to the Request.
            let http_req = context.request_mut();
            // Create a new URI to set on the request. Include the exact path and query string
            // that have been passed as part of the operation that is being called.
            let path_and_query = http_req
                .uri()
                .path_and_query()
                // Convert the PathAndQuery type to a string.
                .map(|p_and_q| p_and_q.as_str())
                // By default an empty path should added to the new URI.
                .unwrap_or("");

            // Create a new URI and set it on the request.
            let uri = format!("{}:{}", service_url, path_and_query).parse()?;
            info!(?uri, "URI changed");

            *http_req.uri_mut() = uri;
        }

        Ok(())
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
    let config = pokemon_service_client::Config::builder()
        .interceptor(EndpointInterceptor::default())
        .endpoint_url(BASE_URL)
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

    info!(?response, "Response received");
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how an interceptor can be written to trace what is being
/// serialized / deserialized on the wire.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example use-config-bag`.
///
use http::StatusCode;
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};
use std::str;
use tracing::{debug, error, info};

use pokemon_service_client::{config::Interceptor, Client as PokemonClient};

static BASE_URL: &str = "http://localhost:13734";

/// An example interceptor that logs the request and response as they're sent and received.
#[derive(Debug, Default)]
pub struct WireFormatInterceptor;

impl WireFormatInterceptor {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for WireFormatInterceptor {
    fn name(&self) -> &'static str {
        "WireFormatInterceptor"
    }

    // Called after the operation input has been serialized but before it's dispatched over the wire.
    fn read_after_serialization(
        &self,
        context: &pokemon_service_client::config::interceptors::BeforeTransmitInterceptorContextRef<
            '_,
        >,
        _runtime_components: &pokemon_service_client::config::RuntimeComponents,
        _cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        // Get the request type from the context.
        let request = context.request();
        // Print the request to the debug tracing log.
        debug!(?request);

        Ok(())
    }

    // Called after the operation's response has been received but before it's deserialized into the
    // operation's output type.
    fn read_before_deserialization(
        &self,
        context: &pokemon_service_client::config::interceptors::BeforeDeserializationInterceptorContextRef<'_>,
        _runtime_components: &pokemon_service_client::config::RuntimeComponents,
        _cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        // Get the response type from the context.
        let response = context.response();
        // Print the response to the debug tracing log.
        if response.status() == StatusCode::OK {
            info!(?response, "Response received:");
        } else {
            error!(?response);
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
    // The generated client has a type config::Builder that can be used to build a Config, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .interceptor(WireFormatInterceptor {})
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
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .custom_expect_and_log("get_storage failed");

    info!(%BASE_URL, ?response, "Response received");
}

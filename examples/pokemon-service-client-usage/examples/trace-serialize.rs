/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use aws_smithy_runtime::client::http::connection_poisoning::CaptureSmithyConnection;
/// This example demonstrates how an interceptor can be written to trace what is being
/// serialized / deserialized on the wire.
///
/// Please beware that this may log sensitive information! This example is meant for pedagogical
/// purposes and may be useful in debugging scenarios. Please don't use this as-is in production.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example trace-serialize`.
///
use http::StatusCode;
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};
use std::str;

use pokemon_service_client::{
    config::{
        interceptors::{
            BeforeDeserializationInterceptorContextRef, BeforeTransmitInterceptorContextRef,
        },
        ConfigBag, Intercept, RuntimeComponents,
    },
    error::BoxError,
    Client as PokemonClient,
};

/// An example interceptor that logs the request and response as they're sent and received.
#[derive(Debug, Default)]
pub struct WireFormatInterceptor;

impl Intercept for WireFormatInterceptor {
    fn name(&self) -> &'static str {
        "WireFormatInterceptor"
    }

    // Called after the operation input has been serialized but before it's dispatched over the wire.
    fn read_after_serialization(
        &self,
        context: &BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Get the request type from the context.
        let request = context.request();
        // Print the request to the debug tracing log.
        tracing::debug!(?request);

        Ok(())
    }

    // Called after the operation's response has been received but before it's deserialized into the
    // operation's output type.
    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Get the response type from the context.
        let response = context.response();
        // Print the response.
        if response.status().as_u16() == StatusCode::OK.as_u16() {
            tracing::info!(?response, "Response received:");
        } else {
            tracing::error!(?response);
        }

        // Print the connection information
        let captured_connection = cfg.load::<CaptureSmithyConnection>().cloned();
        if let Some(captured_connection) = captured_connection.and_then(|conn| conn.get()) {
            tracing::info!(
                remote_addr = ?captured_connection.remote_addr(),
                local_addr = ?captured_connection.local_addr(),
                "Captured connection info"
            );
        } else {
            tracing::warn!("Connection info is missing!");
        }

        Ok(())
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
    // The generated client has a type `Config::Builder` that can be used to build a `Config`, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .interceptor(WireFormatInterceptor {})
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

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

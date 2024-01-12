/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// In this example, a custom header `x-amzn-client-ttl-seconds` is set for all outgoing requests.
/// It serves as a demonstration of how an operation name can be retrieved and utilized within
/// the interceptor.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example custom-header-using-interceptor`.
///
use std::{collections::HashMap, time::Duration};

use aws_smithy_runtime_api::client::orchestrator::Metadata;
use pokemon_service_client::config::{ConfigBag, Intercept};
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client::{
    config::{interceptors::BeforeTransmitInterceptorContextMut, RuntimeComponents},
    error::BoxError,
};
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

// The `TtlHeaderInterceptor` keeps a map of operation specific value to send
// in the header for each Request.
#[derive(Debug)]
pub struct TtlHeaderInterceptor {
    /// Default time-to-live for an operation.
    default_ttl: hyper::http::HeaderValue,
    /// Operation specific time-to-live.
    operation_ttl: HashMap<&'static str, hyper::http::HeaderValue>,
}

// Helper function to format duration as fractional seconds.
fn format_ttl_value(ttl: Duration) -> String {
    format!("{:.2}", ttl.as_secs_f64())
}

impl TtlHeaderInterceptor {
    fn new(default_ttl: Duration) -> Self {
        let duration_str = format_ttl_value(default_ttl);
        let default_ttl_value = hyper::http::HeaderValue::from_str(duration_str.as_str())
            .expect("could not create a header value for the default ttl");

        Self {
            default_ttl: default_ttl_value,
            operation_ttl: Default::default(),
        }
    }

    /// Adds an operation name specific timeout value that needs to be set in the header.
    fn add_operation_ttl(&mut self, operation_name: &'static str, ttl: Duration) {
        let duration_str = format_ttl_value(ttl);

        self.operation_ttl.insert(
            operation_name,
            hyper::http::HeaderValue::from_str(duration_str.as_str())
                .expect("cannot create header value for the given ttl duration"),
        );
    }
}

/// Appends the header `x-amzn-client-ttl-seconds` using either the default time-to-live value
/// or an operation-specific value if it was set earlier using `add_operation_ttl`.
//impl aws_smithy_runtime_api::client::interceptors::Interceptor for TtlHeaderInterceptor {
impl Intercept for TtlHeaderInterceptor {
    fn name(&self) -> &'static str {
        "TtlHeaderInterceptor"
    }

    /// Before the request is signed, add the header to the outgoing request.
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Metadata in the ConfigBag has the operation name.
        let metadata = cfg.load::<Metadata>().expect("metadata should exist");
        let operation_name = metadata.name();

        // Get operation specific or default HeaderValue to set for the header key.
        let ttl = self
            .operation_ttl
            .get(operation_name)
            .unwrap_or(&self.default_ttl);

        context
            .request_mut()
            .headers_mut()
            .insert("x-amzn-client-ttl-seconds", ttl.clone());

        tracing::info!("{operation_name} header set to {ttl:?}");

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
    // By default set the value of all operations to 6 seconds.
    const DEFAULT_TTL: Duration = Duration::from_secs(6);

    // Set up the interceptor to add an operation specific value of 3.5 seconds to be added
    // for GetStorage operation.
    let mut ttl_headers_interceptor = TtlHeaderInterceptor::new(DEFAULT_TTL);
    ttl_headers_interceptor.add_operation_ttl("GetStorage", Duration::from_millis(3500));

    // The generated client has a type `Config::Builder` that can be used to build a `Config`, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .interceptor(ttl_headers_interceptor)
        .build();

    pokemon_service_client::Client::from_conf(config)
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

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response for get_server_statistics()");

    // Call the operation `get_storage` on the Pokémon service. The `TtlHeaderInterceptor`
    // interceptor will add a specific header name / value pair for this operation.
    let response = client
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .expect("operation failed");

    // Print the response received from the service.
    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

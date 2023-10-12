/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how different interceptor can use a property bag to pass
/// state from one interceptor to the next.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example use-config-bag`.
///
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use pokemon_service_client_usage::setup_tracing_subscriber;
use std::time::Instant;
use tracing::info;

use pokemon_service_client::{config::Interceptor, Client as PokemonClient};

static BASE_URL: &str = "http://localhost:13734";

#[derive(Debug)]
struct RequestTimestamp(Instant);

impl Storable for RequestTimestamp {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug, Default)]
pub struct SetTimeInterceptor;

impl SetTimeInterceptor {
    /// Creates a new `SetTimeInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

/// Note: This is merely an example demonstrating how state can
/// be shared between two different interceptors. In a practical
/// scenario, there wouldn't be a need to write two interceptors
/// merely to display the duration from the start of the lifecycle
/// to the receipt of the response. This task can be accomplished
/// within a single interceptor by overriding both
/// read_before_execution and read_before_deserialization.
impl Interceptor for SetTimeInterceptor {
    fn name(&self) -> &'static str {
        "SetTimeInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &pokemon_service_client::config::interceptors::BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), pokemon_service_client::error::BoxError> {
        cfg.interceptor_state()
            .store_put(RequestTimestamp(Instant::now()));
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct GetTimeInterceptor;

impl GetTimeInterceptor {
    /// Creates a new `SetTimeInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for GetTimeInterceptor {
    fn name(&self) -> &'static str {
        "GetTimeInterceptor"
    }

    fn read_before_deserialization(
        &self,
        _context: &pokemon_service_client::config::interceptors::BeforeDeserializationInterceptorContextRef<'_>,
        _runtime_components: &pokemon_service_client::config::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), pokemon_service_client::error::BoxError> {
        let stop_watch = cfg
            .load::<RequestTimestamp>()
            .expect("StopWatch not found in the ConfigBag");

        let time_taken = stop_watch.0.elapsed();
        info!(time_taken = %time_taken.as_micros(), "Microseconds:");

        Ok(())
    }

    fn read_after_execution(
        &self,
        _context: &pokemon_service_client::config::interceptors::FinalizerInterceptorContextRef<'_>,
        _runtime_components: &pokemon_service_client::config::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), pokemon_service_client::error::BoxError> {
        let timestamp = cfg
            .load::<RequestTimestamp>()
            .expect("RequestTimeStamp not found in the ConfigBag");

        let time_taken = timestamp.0.elapsed();
        info!(time_taken = %time_taken.as_micros(), "Microseconds:");

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
        .interceptor(SetTimeInterceptor)
        .interceptor(GetTimeInterceptor)
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

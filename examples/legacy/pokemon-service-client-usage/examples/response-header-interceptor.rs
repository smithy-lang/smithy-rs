/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use aws_smithy_runtime_api::client::orchestrator::Metadata;
/// This example demonstrates how response headers can be examined before they are deserialized
/// into the output type.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example response-header-interceptor`.
///
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use pokemon_service_client::{
    config::{
        interceptors::{
            BeforeDeserializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
        },
        ConfigBag, Intercept, RuntimeComponents,
    },
    error::BoxError,
    Client as PokemonClient,
};
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct RequestId {
    client_id: String,
    server_id: Option<String>,
}

impl Storable for RequestId {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug, thiserror::Error)]
enum RequestIdError {
    /// Client side
    #[error("Client side request ID has not been set")]
    ClientRequestIdMissing(),
}

#[derive(Debug, Default)]
pub struct ResponseHeaderLoggingInterceptor;

impl ResponseHeaderLoggingInterceptor {
    /// Creates a new `ResponseHeaderLoggingInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Intercept for ResponseHeaderLoggingInterceptor {
    fn name(&self) -> &'static str {
        "ResponseHeaderLoggingInterceptor"
    }

    /// Before the request is signed, add the header to the outgoing request.
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let client_id = Uuid::new_v4().to_string();

        let request_id = hyper::header::HeaderValue::from_str(&client_id)
            .expect("failed to construct a header value from UUID");
        context
            .request_mut()
            .headers_mut()
            .insert("x-amzn-requestid", request_id);

        cfg.interceptor_state().store_put(RequestId {
            client_id,
            server_id: None,
        });

        Ok(())
    }

    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // `Metadata` in the `ConfigBag` has the operation name in it.
        let metadata = cfg.load::<Metadata>().expect("metadata should exist");
        let operation_name = metadata.name().to_string();

        // Get the server side request ID and set it in the RequestID data type
        // that is in the ConfigBag. This way any other interceptor that requires the mapping
        // can easily find it from the bag.
        let response = context.response();
        let header_received = response
            .headers()
            .iter()
            .find(|(header_name, _)| *header_name == "x-request-id");

        if let Some((_, server_id)) = header_received {
            let request_details = cfg
                .get_mut::<RequestId>()
                .ok_or_else(|| Box::new(RequestIdError::ClientRequestIdMissing()))?;

            tracing::info!(operation = %operation_name,
                "RequestID Mapping: {} = {server_id}",
                request_details.client_id,
            );

            request_details.server_id = Some(server_id.into());
        } else {
            tracing::info!(operation = %operation_name, "Server RequestID missing in response");
        }

        Ok(())
    }
}

/// Creates a new `smithy-rs` client that is configured to communicate with a locally running Pokémon
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
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .interceptor(ResponseHeaderLoggingInterceptor)
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

    // If you need to access the `RequestIdError` raised by the interceptor,
    // you can convert `SdkError::DispatchFailure` to a `ConnectorError`
    // and then use `downcast_ref` on its source to get a `RequestIdError`.

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

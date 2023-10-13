/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// In this example, a custom header x-amzn-requestid is set for all outgoing requests
/// by writing an interceptor.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example custom-header-using-interceptor`.
///
use aws_smithy_types::config_bag::ConfigBag;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client::{
    config::{interceptors::BeforeTransmitInterceptorContextMut, Interceptor, RuntimeComponents},
    error::BoxError,
};
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};
use tracing::info;
use uuid::Uuid;

// URL where example Pokemon service is running.
static BASE_URL: &str = "http://localhost:13734";
// Header to send with each operation.
const HEADER_TO_SEND: hyper::header::HeaderName =
    hyper::header::HeaderName::from_static("x-amzn-requestid");

// The RequestIdInterceptor keeps a map of operation specific value to send
// for the header.
#[derive(Debug, Clone)]
pub struct RequestIdInterceptor;

impl Interceptor for RequestIdInterceptor {
    fn name(&self) -> &'static str {
        "RequestIdInterceptor"
    }

    /// Before the request is signed, add the header to the outgoing request.
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let request_id = hyper::header::HeaderValue::from_str(&Uuid::new_v4().to_string())
            .expect("failed to construct a header value from UUID");

        context
            .request_mut()
            .headers_mut()
            .insert(&HEADER_TO_SEND, request_id);

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
        .interceptor(RequestIdInterceptor {})
        .build();

    pokemon_service_client::Client::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured Smithy client.
    let client = create_client();

    // Call the operation `get_storage` on Pokemon service.
    let response = client
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .custom_expect_and_log("Operation could not be called");

    info!(%BASE_URL, ?response, "Response for get_storage()");
}

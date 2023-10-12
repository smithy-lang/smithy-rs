/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how response headers can be examined before they are deserialized
/// into the output type.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example custom-middleware-mapresponse`.
///
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use tracing::info;

// Define a type alias that makes it easy to pass around the Client type.
use pokemon_service_client::{
    config::{
        interceptors::{
            BeforeDeserializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
        },
        Interceptor, RuntimeComponents,
    },
    Client as PokemonClient,
};
use uuid::Uuid;

static BASE_URL: &str = "http://localhost:13734";
// Header to send for client side request ID. Refer to example 'requestid-interceptor'
// to see how an interceptor can add headers to outgoing requests.
const REQUEST_ID_HEADER: hyper::header::HeaderName =
    hyper::header::HeaderName::from_static("x-amzn-requestid");

#[derive(Debug, Clone)]
struct RequestId {
    client_id: String,
    server_id: Option<String>,
}

impl Storable for RequestId {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug, Default)]
pub struct ResponseHeaderLoggingInterceptor;

impl ResponseHeaderLoggingInterceptor {
    /// Creates a new `ResponseHeaderLoggingInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for ResponseHeaderLoggingInterceptor {
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
            .insert(&REQUEST_ID_HEADER, request_id);

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
        // Metadata in the ConfigBag has the operation name in it.
        let metadata = cfg
            .load::<aws_smithy_http::operation::Metadata>()
            .expect("metadata should exist");
        let operation_name = metadata.name().to_string();

        // Get the server side request ID and set it in the RequestID data type
        // that is in the ConfigBag. This way any other interceptor that requires the mapping
        // can easily find it from the bag.
        let response = context.response();
        let header_received = response
            .headers()
            .iter()
            .find(|(header_name, _)| *header_name == "REQUEST_ID");

        if let Some((_, server_id)) = header_received {
            let server_id = server_id
                .to_str()
                .expect("HeaderValue for server side request ID could not be converted to string")
                .to_string();

            let request_details = cfg
                .get_mut::<RequestId>()
                .expect("RequestId data type not found in the ConfigBag");

            info!(operation = %operation_name,
                "RequestID Mapping: {} = {server_id}",
                request_details.client_id,
            );

            request_details.server_id = Some(server_id);
        } else {
            info!(operation = %operation_name, "Server RequestID missing in response");
        }

        Ok(())
    }
}

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
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .interceptor(ResponseHeaderLoggingInterceptor)
        .build();

    // Apply the configuration on the Client, and return that.
    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    tracing_setup();

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

fn tracing_setup() {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }
    let _ = color_eyre::install();

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}

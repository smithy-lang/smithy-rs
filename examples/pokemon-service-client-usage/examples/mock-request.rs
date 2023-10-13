/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to create a Smithy Client using a connector
/// that can be used to return mock responses. This can be useful for testing
/// purposes.
///
/// The example can be run using `cargo run --example mock-request`.
///
//use aws_smithy_http::body::SdkBody;
use aws_smithy_http::body::SdkBody;
use aws_smithy_runtime_api::{
    client::{
        http::{
            HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
            SharedHttpConnector,
        },
        orchestrator::HttpRequest,
    },
    shared::FromUnshared,
};
use pokemon_service_client::{config::RuntimeComponents, Client as PokemonClient};
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};
use tracing::info;

static BASE_URL: &str = "http://localhost:13734";

/// The MockConnector won't send the request on the wire and will send a static Response
/// when a request comes in. It can be extended to return different responses for different
/// requests.
#[derive(Debug, Clone)]
struct MockConnector {}

impl HttpConnector for MockConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        info!(?request, "Got request in MockConnector");

        let res = http::Response::builder()
            .status(200)
            .body(SdkBody::from(
                r#" {
                   "calls_count" : 100
                 }"#,
            ))
            .expect("Cannot construct a response");

        HttpConnectorFuture::new(async move { Ok(res) })
    }
}

/// HttpClient must be implemented for a type to be acceptable as a Connector
/// by the Config::builder.
impl HttpClient for MockConnector {
    fn http_connector(
        &self,
        _: &HttpConnectorSettings,
        _: &RuntimeComponents,
    ) -> SharedHttpConnector {
        // Any type that implements HttpConnector trait can be converted
        // into a SharedHttpConnector by using `FromUnshared::from_unshared`
        FromUnshared::from_unshared(self.clone())
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
    // The generated client has a type config::Builder that can be used to build a Config, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .http_client(MockConnector {})
        .build();

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

    // Print the response received from the service.
    info!(%BASE_URL, ?response, "Response received");
}

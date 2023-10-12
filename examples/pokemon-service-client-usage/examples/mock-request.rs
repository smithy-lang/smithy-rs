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
use aws_smithy_http::{body::SdkBody, result::ConnectorError};
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::setup_tracing_subscriber;
use std::{future::Future, pin::Pin};
use tower::Service;
use tracing::info;

static BASE_URL: &str = "http://localhost:13734";

#[derive(Debug, Clone)]
struct MockConnector {}

/// Implement a tower Service that takes a Request as input and returns
/// a Response as output.
impl Service<http::Request<SdkBody>> for MockConnector {
    type Response = HttpResponse;
    type Error = ConnectorError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        info!(?req, "Request received in MockConnector:");

        let r = http::Response::builder()
            .header("content-type", "application/json")
            .status(http::StatusCode::OK)
            .body::<SdkBody>("".into())
            .map_err(|e| ConnectorError::user(Box::new(e)));

        Box::pin(async move { r })
    }
}

/// Creates a new Smithy client that is configured to communicate with a locally running Pokemon
/// service on TCP port 13734.
///
/// For convenience, this example type-erases the concrete HTTP transport backend used using
/// dynamic dispatch. This comes at a slight runtime performance cost. See
/// [`DynConnector`](https://docs.rs/aws-smithy-client/latest/aws_smithy_client/erase/struct.DynConnector.html) for details.
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
        .http_connector(MockConnector {})
        .build();

    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured Smithy client.
    let client = create_client();
    // Call an operation `get_server_statistics` on Pokemon service.
    let response = client.get_server_statistics().send().await;
    // Print the response received from the service.
    info!(%BASE_URL, ?response, "Response received");
}

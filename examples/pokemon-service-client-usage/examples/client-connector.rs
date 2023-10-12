/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how set connector settings. For example, how to set
/// trusted root certificates to use for https communication.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example dyn-client`.
///
use std::time::Duration;

use aws_smithy_client::{http_connector::ConnectorSettings, hyper_ext, SdkError};
// ConfigBuilderExt
use hyper_rustls::ConfigBuilderExt;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, ResultExt};
use tracing::{error, info};

static BASE_URL: &str = "http://localhost:13734";

/// Creates a new Smithy client that is configured to communicate with a locally running Pokemon
/// service on TCP port 13734.
///
/// # Examples
///
/// Basic usage:
/// ```
/// let client = create_client();
/// ```
fn create_client() -> PokemonClient {
    let tls_config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        // with_native_roots(): Load platform trusted root certificates.
        // with_webpki_roots(): Load Mozilla’s set of trusted roots.
        .with_native_roots()
        // To use client side certificates, you can use
        // .with_client_auth_cert(client_cert, client_key) instead of .with_no_client_auth();
        .with_no_client_auth();

    let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        // This can be changed to .https_only() to ensure that the client always uses https
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let smithy_connector = hyper_ext::Adapter::builder()
        // Optionally set things like timeouts as well
        .connector_settings(
            ConnectorSettings::builder()
                .connect_timeout(Duration::from_secs(5))
                .build(),
        )
        .build(https_connector);

    // Pass the smithy connector to the Client::ConfigBuilder
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(BASE_URL)
        .http_connector(smithy_connector)
        .build();

    // Instantiate a client by applying the configuration.
    pokemon_service_client::Client::from_conf(config)
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

    info!(?response, "Response from service")
}

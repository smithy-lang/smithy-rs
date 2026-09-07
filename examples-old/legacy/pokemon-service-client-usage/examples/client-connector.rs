/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to set connector settings. For example, how to set
/// trusted root certificates to use for HTTPs communication.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example client-connector`.
///
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use hyper_rustls::ConfigBuilderExt;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

/// Creates a new `smithy-rs` client that is configured to communicate with a locally running Pokémon
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
        // `with_native_roots()`: Load platform trusted root certificates.
        // `with_webpki_roots()`: Load Mozilla’s set of trusted roots.
        .with_native_roots()
        // To use client side certificates, you can use
        // `.with_client_auth_cert(client_cert, client_key)` instead of `.with_no_client_auth()`
        .with_no_client_auth();

    let tls_connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        // This can be changed to `.https_only()` to ensure that the client always uses HTTPs
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    // Create a hyper-based HTTP client that uses this TLS connector.
    let http_client = HyperClientBuilder::new().build(tls_connector);

    // Pass the smithy connector to the Client::ConfigBuilder
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .http_client(http_client)
        .build();

    // Instantiate a client by applying the configuration.
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

    tracing::info!(?response, "Response from service")
}

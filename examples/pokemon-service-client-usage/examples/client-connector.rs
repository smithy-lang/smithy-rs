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
use aws_smithy_http_client::{tls, Builder};
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
    // Create a TLS context that loads platform trusted root certificates.
    // The TrustStore::default() enables native roots by default.
    let tls_context = tls::TlsContext::builder()
        .with_trust_store(tls::TrustStore::default())
        .build()
        .expect("failed to build TLS context");

    // Create an HTTP client using rustls with AWS-LC crypto provider.
    // To use client side certificates, you would need to customize the TLS config further.
    let http_client = Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .tls_context(tls_context)
        .build_https();

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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how a custom RetryClassifier can be written to decide
/// which error conditions should be retried.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example retry-classifier`.
///
use http::StatusCode;
use pokemon_service_client::{
    config::{
        interceptors::InterceptorContext,
        retry::{ClassifyRetry, RetryAction, RetryConfig},
    },
    operation::get_server_statistics::GetServerStatisticsError,
};
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};
use std::time::Duration;

use pokemon_service_client::Client as PokemonClient;

#[derive(Debug)]
struct SampleRetryClassifier;

// By default, the generated client uses the `aws_http::retry::AwsResponseRetryClassifier`
// to determine whether an error should be retried. To use a custom retry classifier,
// implement the `ClassifyRetry` trait and pass it to the retry_classifier method
// of the `Config::builder`.
impl ClassifyRetry for SampleRetryClassifier {
    fn name(&self) -> &'static str {
        "SampleRetryClassifier"
    }

    // For this example, the classifier should retry in case the error is GetServerStatisticsError
    // and the status code is 503.
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
        // Get the output or error that has been deserialized from the response.
        let output_or_error = ctx.output_or_error();

        let error = match output_or_error {
            Some(Ok(_)) | None => return RetryAction::NoActionIndicated,
            Some(Err(err)) => err,
        };

        // Retry in case the error returned is GetServerStatisticsError and StatusCode is 503.
        if let Some(_err) = error
            .as_operation_error()
            .and_then(|err| err.downcast_ref::<GetServerStatisticsError>())
        {
            if let Some(response) = ctx.response() {
                if response.status() == StatusCode::SERVICE_UNAVAILABLE.into() {
                    return RetryAction::server_error();
                }
            }
        }

        // Let other classifiers run and decide if the request should be retried.
        // Returning RetryAction::RetryForbidden will forbid any retries.
        RetryAction::NoActionIndicated
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
    // By default the Smithy client uses RetryConfig::standard() strategy, with 3 retries, and
    // an initial exponential back off of 1 second. To turn it off use RetryConfig::disabled().
    let retry_config = RetryConfig::standard()
        .with_initial_backoff(Duration::from_secs(3))
        .with_max_attempts(5);

    // The generated client has a type `Config::Builder` that can be used to build a `Config`, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .retry_config(retry_config)
        // Add the retry classifier.
        .retry_classifier(SampleRetryClassifier {})
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

    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");
}

use aws_smithy_http::retry::ClassifyRetry;
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to customize retry settings on a Smithy client.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example retry-customize`.
///
use aws_smithy_types::retry::{RetryConfig, RetryKind};
use pokemon_service_client_usage::setup_tracing_subscriber;
use std::time::Duration;
use tracing::info;

use pokemon_service_client::Client as PokemonClient;

static BASE_URL: &str = "http://localhost:13734";

use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use aws_smithy_types::retry::ErrorKind;
use std::error::Error as StdError;
use std::marker::PhantomData;

const RETRYABLE_ERROR_CODES: &[&str] = &["500"];

// When classifying at an operation's error type, classifiers require a generic parameter.
// When classifying the HTTP response alone, no generic is needed.
#[derive(Debug, Default, Clone)]
pub struct ErrorCodeClassifier<E> {
    _inner: PhantomData<E>,
}

impl<E> ErrorCodeClassifier<E> {
    pub fn new() -> Self {
        Self {
            _inner: PhantomData,
        }
    }
}

impl<T, E> ClassifyRetry<T, E> for ErrorCodeClassifier<E>
where
    // Adding a trait bound for ProvideErrorMetadata allows us to inspect the error code.
    E: StdError + ProvideErrorMetadata + Send + Sync + 'static,
    E: Clone,
{
    fn classify_retry(&self, response: Result<&T, &E>) -> RetryKind {
        todo!()
    }
    
    // fn classify_retry(&self, ctx: &InterceptorContext) -> RetryKind {
    //     // Check for a result
    //     let output_or_error = ctx.output_or_error();
    //     // Check for an error
    //     let error = match output_or_error {
    //         Some(Ok(_)) | None => return RetryAction::NoActionIndicated,
    //         Some(Err(err)) => err,
    //     };

    //     // Downcast the generic error and extract the code
    //     let error_code = OrchestratorError::as_operation_error(error)
    //         .and_then(|err| err.downcast_ref::<E>())
    //         .and_then(|err| err.code());

    //     // If this error's code is in our list, return an action that tells the RetryStrategy to retry this request.
    //     if let Some(error_code) = error_code {
    //         if RETRYABLE_ERROR_CODES.contains(&error_code) {
    //             return RetryAction::transient_error();
    //         }
    //     }

    //     // Otherwise, return that no action is indicated i.e. that this classifier doesn't require a retry.
    //     // Another classifier may still classify this response as retryable.
    //     RetryAction::NoActionIndicated
    // }
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
    // By default the Smithy client uses RetryConfig::standard() strategy, with 3 retries, and
    // an initial exponential back off of 1 second. To turn it off use RetryConfig::disabled().
    let retry_config = RetryConfig::standard()
        .with_initial_backoff(Duration::from_secs(3))
        .with_max_attempts(5);

    // The generated client has a type config::Builder that can be used to build a Config, which
    // allows configuring endpoint-resolver, timeouts, retries etc.
    let config = pokemon_service_client::Config::builder()
        .endpoint_resolver(BASE_URL)
        .retry_config(retry_config)
        .sleep_impl(::aws_smithy_async::rt::sleep::SharedAsyncSleep::new(
            aws_smithy_async::rt::sleep::default_async_sleep()
                .expect("sleep implementation could not be created"),
        ))
        .build();

    // Apply the configuration on the Client, and return that.
    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // let sdk_config = aws_config::load_from_env().await;
    // let client = aws_sdk_s3::Client::new(&sdk_config);
    // let some_object = client
    //     .get_object()
    //     .bucket("bucket")
    //     .key("key")
    //     .customize()
    //     .config_override(aws_sdk_s3::config::Config::builder().retry_classifier(ExampleErrorCodeClassifier::<GetObjectError>::new()))
    //     .send()
    //     .await
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

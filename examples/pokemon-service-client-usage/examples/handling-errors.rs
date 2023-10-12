/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// Copyright © 2023, Amazon, LLC.
///
/// This example demonstrates how to handle Service generated errors.
///
/// The example assumes that the Pokemon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example handling-errors`.
///
use aws_smithy_client::SdkError;
use pokemon_service_client::operation::get_storage::GetStorageError;
use pokemon_service_client_usage::setup_tracing_subscriber;
use tracing::{error, info};

use pokemon_service_client::Client as PokemonClient;

static BASE_URL: &str = "http://localhost:13734";

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
        .build();

    // Apply the configuration on the Client, and return that.
    PokemonClient::from_conf(config)
}

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Create a configured Smithy client.
    let client = create_client();

    // The following example sends an incorrect passcode to the operation `get_storage`,
    // which will return
    // [StorageAccessNotAuthorized](https://github.com/awslabs/smithy-rs/blob/main/codegen-core/common-test-models/pokemon.smithy#L48)
    let response_result = client
        .get_storage()
        .user("ash")
        // Give a wrong password to generate a service error.
        .passcode("pkachu123")
        .send()
        .await;

    // All errors are consolidated into an SdkError<>. It includes the following variants:
    // SdkError::ServiceError: This represents an error raised by the service.
    // SdkError::Timeout: Indicates that the request couldn't be completed due to a timeout.
    // SdkError::DispatchError: Denotes an error encountered while sending the request.
    // SdkError::ConstructionFailure: Signifies that the request couldn't be constructed.
    // SdkError::ResponseError: Implies that the response from the service couldn't be converted into the expected response data type.
    match response_result {
        Ok(response) => {
            info!(?response, "Response from service")
        }
        Err(SdkError::ServiceError(se)) => {
            // When an error response is received from the service, it is modeled
            // as a SdkError::ServiceError.
            match se.err() {
                // Not authorized to access Pokémon storage.
                GetStorageError::StorageAccessNotAuthorized(_) => {
                    error!("You do not have access to this resource.");
                }
                GetStorageError::ResourceNotFoundError(rnfe) => {
                    let message = rnfe.message();
                    error!(error = %message,
                        "Given Pikachu does not exist on the server."
                    )
                }
                GetStorageError::ValidationError(ve) => {
                    error!(error = %ve, "A required field has not been set.");
                }
                // An unexpected error occurred (e.g., invalid JSON returned by the service or an unknown error code).
                GetStorageError::Unhandled(uh) => {
                    error!(error = %uh, "An unhandled error has occurred.")
                }
                // The SdkError is marked as #[non_exhaustive]. Therefore, a catch-all pattern is required to handle
                // potential future variants introduced in SdkError.
                _ => {
                    error!(error = %se.err(), "Some other error has occurred on the server")
                }
            }
        }
        Err(SdkError::TimeoutError(_)) => {
            error!("The request timed out and could not be completed");
        }
        Err(SdkError::ResponseError(re)) => {
            // Raw response received from the service can be retrieved using
            // the raw() method.
            error!(
                "An unparsable response was received. Raw response: {:?}",
                re.raw()
            );
        }
        Err(sdk_error) => {
            // To retrieve the `source()` of an error within the following match statements,
            // we work with the parent `SdkError` type, as individual variants don't directly provide it.
            // Converting the parent error to its source transfers ownership of the variable.
            match sdk_error {
                SdkError::DispatchFailure(ref failure) => {
                    if failure.is_io() {
                        error!("An I/O error occurred");
                    } else if failure.is_timeout() {
                        error!("Request timed out");
                    } else if failure.is_user() {
                        error!("An invalid HTTP request has been provided");
                    } else {
                        error!("Some other dispatch error occurred.");
                    };

                    if let Ok(source) = sdk_error.into_source() {
                        error!(%source, "Error source");
                    }
                }
                SdkError::ConstructionFailure(_) => {
                    if let Ok(source) = sdk_error.into_source() {
                        error!(%source, "Request could not be constructed.");
                    } else {
                        error!("Request could not be constructed for unknown reasons");
                    }
                }
                _ => {
                    error!("An unknown error has occurred");
                }
            }
        }
    }
}

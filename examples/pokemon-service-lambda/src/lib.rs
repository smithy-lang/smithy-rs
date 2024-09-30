/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::Arc;

use pokemon_service_common::State;
use pokemon_service_server_sdk::{
    error::{GetStorageError, StorageAccessNotAuthorized},
    input::GetStorageInput,
    output::GetStorageOutput,
    server::{request::lambda::Context, Extension},
};

/// Retrieves the user's storage and logs the lambda request ID.
pub async fn get_storage_lambda(
    input: GetStorageInput,
    _state: Extension<Arc<State>>,
    context: Context,
) -> Result<GetStorageOutput, GetStorageError> {
    tracing::debug!(request_id = %context.request_id, "attempting to authenticate storage user");

    // We currently only support Ash and he has nothing stored
    if !(input.user == "ash" && input.passcode == "pikachu123") {
        tracing::debug!("authentication failed");
        return Err(GetStorageError::StorageAccessNotAuthorized(
            StorageAccessNotAuthorized {},
        ));
    }
    Ok(GetStorageOutput { collection: vec![] })
}

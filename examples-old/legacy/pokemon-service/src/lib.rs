/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::net::{IpAddr, SocketAddr};

use pokemon_service_server_sdk_http0x::{
    error::{GetStorageError, StorageAccessNotAuthorized},
    input::{DoNothingInput, GetStorageInput},
    output::{DoNothingOutput, GetStorageOutput},
    server::request::{connect_info::ConnectInfo, request_id::ServerRequestId},
};

// Defaults shared between `main.rs` and `/tests`.
pub const DEFAULT_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 13734;

/// Logs the request IDs to `DoNothing` operation.
pub async fn do_nothing_but_log_request_ids(
    _input: DoNothingInput,
    request_id: ServerRequestId,
) -> DoNothingOutput {
    tracing::debug!(%request_id, "do nothing");
    DoNothingOutput {}
}

/// Retrieves the user's storage. No authentication required for locals.
pub async fn get_storage_with_local_approved(
    input: GetStorageInput,
    connect_info: ConnectInfo<SocketAddr>,
) -> Result<GetStorageOutput, GetStorageError> {
    tracing::debug!("attempting to authenticate storage user");

    if !(input.user == "ash" && input.passcode == "pikachu123") {
        tracing::debug!("authentication failed");
        return Err(GetStorageError::StorageAccessNotAuthorized(
            StorageAccessNotAuthorized {},
        ));
    }

    // We support trainers in our local gym
    let local = connect_info.0.ip() == "127.0.0.1".parse::<IpAddr>().unwrap();
    if local {
        tracing::info!("welcome back");
        return Ok(GetStorageOutput {
            collection: vec![
                String::from("bulbasaur"),
                String::from("charmander"),
                String::from("squirtle"),
                String::from("pikachu"),
            ],
        });
    }

    Ok(GetStorageOutput {
        collection: vec![String::from("pikachu")],
    })
}

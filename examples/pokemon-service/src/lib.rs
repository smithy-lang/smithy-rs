/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::net::IpAddr;
use std::net::SocketAddr;

use aws_smithy_http_server_metrics::operation::Metrics;
use pokemon_service_common::metrics::PokemonOperationMetrics;
use pokemon_service_server_sdk::error::GetStorageError;
use pokemon_service_server_sdk::error::StorageAccessNotAuthorized;
use pokemon_service_server_sdk::input::DoNothingInput;
use pokemon_service_server_sdk::input::GetStorageInput;
use pokemon_service_server_sdk::output::DoNothingOutput;
use pokemon_service_server_sdk::output::GetStorageOutput;
use pokemon_service_server_sdk::server::request::connect_info::ConnectInfo;
use pokemon_service_server_sdk::server::request::request_id::ServerRequestId;

pub mod middleware_data;

// Defaults shared between `main.rs` and `/tests`.
pub const DEFAULT_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 13734;

/// Logs the request IDs to `DoNothing` operation.
pub async fn do_nothing_but_log_request_ids(
    _input: DoNothingInput,
    request_id: ServerRequestId,
    middleware: middleware_data::MiddlewareExt,
) -> DoNothingOutput {
    let data = middleware
        .data()
        .expect("MiddlewareDataPlugin is registered for every operation");
    tracing::info!(
        %request_id,
        middleware_request_id = %data.request_id,
        operation = ?data.operation,
        elapsed = ?data.started_at.elapsed(),
        "do nothing"
    );
    DoNothingOutput {}
}

/// Retrieves the user's storage. No authentication required for locals.
pub async fn get_storage_with_local_approved(
    input: GetStorageInput,
    connect_info: ConnectInfo<SocketAddr>,
    mut metrics: Metrics<PokemonOperationMetrics>,
    middleware: middleware_data::MiddlewareExt,
) -> Result<GetStorageOutput, GetStorageError> {
    middleware.with(|data| {
        tracing::info!(
            middleware_request_id = %data.request_id,
            operation = ?data.operation,
            elapsed = ?data.started_at.elapsed(),
            "attempting to authenticate storage user"
        );
    });

    let authenticated = input.user == "ash" && input.passcode == "pikachu123";
    metrics.get_storage_metrics.user = Some(input.user.clone());
    metrics.get_storage_metrics.authenticated = Some(authenticated);

    if !authenticated {
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

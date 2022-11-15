/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use pokemon_service::{
    capture_pokemon, check_health, do_nothing, get_pokemon_species, get_server_statistics, setup_tracing,
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = "127.0.0.1")]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value = "13734")]
    port: u16,
}

/// Retrieves the user's storage. No authentication required for locals.
pub async fn get_storage_with_local_approved(
    input: pokemon_service_server_sdk::input::GetStorageInput,
    connect_info: aws_smithy_http_server::Extension<aws_smithy_http_server::routing::ConnectInfo<std::net::SocketAddr>>,
) -> Result<pokemon_service_server_sdk::output::GetStorageOutput, pokemon_service_server_sdk::error::GetStorageError> {
    tracing::debug!("attempting to authenticate storage user");
    let local = connect_info.0 .0.ip() == "127.0.0.1".parse::<std::net::IpAddr>().unwrap();

    // We currently support Ash: he has nothing stored
    if input.user == "ash" && input.passcode == "pikachu123" {
        return Ok(pokemon_service_server_sdk::output::GetStorageOutput { collection: vec![] });
    }
    // We support trainers in our gym
    if local {
        tracing::info!("welcome back");
        return Ok(pokemon_service_server_sdk::output::GetStorageOutput {
            collection: vec![
                String::from("bulbasaur"),
                String::from("charmander"),
                String::from("squirtle"),
            ],
        });
    }
    tracing::debug!("authentication failed");
    Err(pokemon_service_server_sdk::error::GetStorageError::NotAuthorized(
        pokemon_service_server_sdk::error::NotAuthorized {},
    ))
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    setup_tracing();
    let app = pokemon_service_server_sdk::service::PokemonService::builder_without_plugins()
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage_with_local_approved)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        .build()
        .expect("failed to build an instance of PokemonService");

    // Start the [`hyper::Server`].
    let bind: std::net::SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind).serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>());

    // Run forever-ish...
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

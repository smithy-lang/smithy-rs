/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Files here are for running integration tests.
// These tests only have access to your crate's public API.
// See: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

use std::time::Duration;

use crate::helpers::{client, PokemonService};
use tokio::time;

mod helpers;

#[tokio::test]
async fn simple_integration_test() {
    let _program = PokemonService::run();
    // Give PokemonSérvice some time to start up.
    time::sleep(Duration::from_millis(50)).await;

    let service_statistics_out = client().get_server_statistics().send().await.unwrap();
    assert_eq!(0, service_statistics_out.calls_count.unwrap());

    let pokemon_species_output = client().get_pokemon_species().name("pikachu").send().await.unwrap();
    assert_eq!("pikachu", pokemon_species_output.name().unwrap());

    let service_statistics_out = client().get_server_statistics().send().await.unwrap();
    assert_eq!(1, service_statistics_out.calls_count.unwrap());

    let pokemon_species_error = client()
        .get_pokemon_species()
        .name("some_pokémon")
        .send()
        .await
        .unwrap_err();
    assert_eq!(
        r#"ResourceNotFoundError [ResourceNotFoundException]: Requested Pokémon not available"#,
        pokemon_species_error.to_string()
    );

    let service_statistics_out = client().get_server_statistics().send().await.unwrap();
    assert_eq!(2, service_statistics_out.calls_count.unwrap());
}

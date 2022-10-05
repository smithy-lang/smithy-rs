/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Files here are for running integration tests.
// These tests only have access to your crate's public API.
// See: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

use crate::helpers::{client, client_http2_only, PokemonService};

use async_stream::stream;
use pokemon_service_client::{
    error::{
        AttemptCapturingPokemonEventError, AttemptCapturingPokemonEventErrorKind, GetStorageError, GetStorageErrorKind,
        MasterBallUnsuccessful, NotAuthorized,
    },
    model::{AttemptCapturingPokemonEvent, CapturingEvent, CapturingPayload},
    types::SdkError,
};
use rand::Rng;
use serial_test::serial;

mod helpers;

fn get_pokeball() -> String {
    let random = rand::thread_rng().gen_range(0..100);
    let pokeball = if random < 5 {
        "Master Ball"
    } else if random < 30 {
        "Great Ball"
    } else if random < 80 {
        "Fast Ball"
    } else {
        "Smithy Ball"
    };
    pokeball.to_string()
}

fn get_pokemon_to_capture() -> String {
    let pokemons = vec!["Charizard", "Pikachu", "Regieleki"];
    pokemons[rand::thread_rng().gen_range(0..pokemons.len())].to_string()
}

#[tokio::test]
#[serial]
async fn test_check_health() {
    let _program = PokemonService::run().await;

    let _check_health = client().check_health().send().await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_check_health_http2() {
    // Make sure our server can serve http2
    let _program = PokemonService::run_https().await;
    let _check_health = client_http2_only().check_health().send().await.unwrap();
}

#[tokio::test]
#[serial]
async fn simple_integration_test() {
    let _program = PokemonService::run().await;

    let service_statistics_out = client().get_server_statistics().send().await.unwrap();
    assert_eq!(0, service_statistics_out.calls_count.unwrap());

    let pokemon_species_output = client().get_pokemon_species().name("pikachu").send().await.unwrap();
    assert_eq!("pikachu", pokemon_species_output.name().unwrap());

    let service_statistics_out = client().get_server_statistics().send().await.unwrap();
    assert_eq!(1, service_statistics_out.calls_count.unwrap());

    let storage_err = client().get_storage().user("ash").passcode("pikachu321").send().await;
    if let Err(SdkError::ServiceError {
        err:
            GetStorageError {
                kind: GetStorageErrorKind::NotAuthorized(NotAuthorized { .. }),
                ..
            },
        ..
    }) = storage_err
    {
    } else {
        assert!(false, "expected NotAuthorized error")
    }

    let storage_out = client()
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .unwrap();
    assert_eq!(Some(vec![]), storage_out.collection);

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

#[tokio::test]
#[serial]
async fn event_stream_test() {
    let _program = PokemonService::run().await;

    let mut team = vec![];
    let input_stream = stream! {
        // Always Pikachu
        yield Ok(AttemptCapturingPokemonEvent::Event(
            CapturingEvent::builder()
            .payload(CapturingPayload::builder()
                .name("Pikachu")
                .pokeball("Master Ball")
                .build())
            .build()
        ));
        yield Ok(AttemptCapturingPokemonEvent::Event(
            CapturingEvent::builder()
            .payload(CapturingPayload::builder()
                .name("Regieleki")
                .pokeball("Fast Ball")
                .build())
            .build()
        ));
        yield Err(AttemptCapturingPokemonEventError::new(
            AttemptCapturingPokemonEventErrorKind::MasterBallUnsuccessful(MasterBallUnsuccessful::builder().build()),
            Default::default()
        ));
        // The next event should not happen
        yield Ok(AttemptCapturingPokemonEvent::Event(
            CapturingEvent::builder()
            .payload(CapturingPayload::builder()
                .name("Charizard")
                .pokeball("Great Ball")
                .build())
            .build()
        ));
    };

    // Throw many!
    let mut output = client()
        .capture_pokemon()
        .region("Kanto")
        .events(input_stream.into())
        .send()
        .await
        .unwrap();
    loop {
        match output.events.recv().await {
            Ok(Some(capture)) => {
                let pokemon = capture.as_event().unwrap().name.as_ref().unwrap().clone();
                let pokedex = capture.as_event().unwrap().pokedex_update.as_ref().unwrap().clone();
                let shiny = if *capture.as_event().unwrap().shiny.as_ref().unwrap() {
                    ""
                } else {
                    "not "
                };
                let expected_pokedex: Vec<u8> = (0..255).collect();
                println!("captured {} ({}shiny)", pokemon, shiny);
                if expected_pokedex == pokedex.into_inner() {
                    println!("pokedex updated")
                }
                team.push(pokemon);
            }
            Err(e) => {
                println!("error from the server: {:?}", e);
                break;
            }
            Ok(None) => break,
        }
    }

    while team.len() < 6 {
        let pokeball = get_pokeball();
        let pokemon = get_pokemon_to_capture();
        let input_stream = stream! {
            yield Ok(AttemptCapturingPokemonEvent::Event(
                CapturingEvent::builder()
                .payload(CapturingPayload::builder()
                    .name(pokemon)
                    .pokeball(pokeball)
                    .build())
                .build()
            ))
        };
        let mut output = client()
            .capture_pokemon()
            .region("Kanto")
            .events(input_stream.into())
            .send()
            .await
            .unwrap();
        match output.events.recv().await {
            Ok(Some(capture)) => {
                let pokemon = capture.as_event().unwrap().name.as_ref().unwrap().clone();
                let pokedex = capture.as_event().unwrap().pokedex_update.as_ref().unwrap().clone();
                let shiny = if *capture.as_event().unwrap().shiny.as_ref().unwrap() {
                    ""
                } else {
                    "not "
                };
                let expected_pokedex: Vec<u8> = (0..255).collect();
                println!("captured {} ({}shiny)", pokemon, shiny);
                if expected_pokedex == pokedex.into_inner() {
                    println!("pokedex updated")
                }
                team.push(pokemon);
            }
            Err(e) => {
                println!("error from the server: {:?}", e);
                break;
            }
            Ok(None) => {}
        }
    }
    println!("Team: {:?}", team);
}

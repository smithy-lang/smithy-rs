/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Files here are for running integration tests.
// These tests only have access to your crate's public API.
// See: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

use async_stream::stream;
use aws_smithy_types::error::display::DisplayErrorContext;
use rand::Rng;
use serial_test::serial;

use crate::helpers::{client, http2_client, PokemonClient, PokemonService};
use pokemon_service_client::{
    types::error::{AttemptCapturingPokemonEventError, MasterBallUnsuccessful},
    types::{AttemptCapturingPokemonEvent, CapturingEvent, CapturingPayload},
};

mod helpers;

#[tokio::test]
#[serial]
async fn simple_integration_test() {
    let _program = PokemonService::run().await;
    simple_integration_test_with_client(client()).await;
}

#[tokio::test]
#[serial]
async fn simple_integration_test_http2() {
    let _program = PokemonService::run_http2().await;
    simple_integration_test_with_client(http2_client()).await;
}

async fn simple_integration_test_with_client(client: PokemonClient) {
    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(0, service_statistics_out.calls_count.unwrap());

    let pokemon_species_output = client
        .get_pokemon_species()
        .name("pikachu")
        .send()
        .await
        .unwrap();
    assert_eq!("pikachu", pokemon_species_output.name().unwrap());

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(1, service_statistics_out.calls_count.unwrap());

    let pokemon_species_error = client
        .get_pokemon_species()
        .name("some_pokémon")
        .send()
        .await
        .unwrap_err();
    let message = DisplayErrorContext(pokemon_species_error).to_string();
    let expected =
        r#"ResourceNotFoundError [ResourceNotFoundException]: Requested Pokémon not available"#;
    assert!(
        message.contains(expected),
        "expected '{message}' to contain '{expected}'"
    );

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(2, service_statistics_out.calls_count.unwrap());

    let _health_check = client.check_health().send().await.unwrap();
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
        yield Err(AttemptCapturingPokemonEventError::MasterBallUnsuccessful(MasterBallUnsuccessful::builder().build()));
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
                let pokedex = capture
                    .as_event()
                    .unwrap()
                    .pokedex_update
                    .as_ref()
                    .unwrap()
                    .clone();
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
                let pokedex = capture
                    .as_event()
                    .unwrap()
                    .pokedex_update
                    .as_ref()
                    .unwrap()
                    .clone();
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

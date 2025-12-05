/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod common;

use async_stream::stream;
use pokemon_service_client::types::{
    error::{AttemptCapturingPokemonEventError, MasterBallUnsuccessful},
    AttemptCapturingPokemonEvent, CapturingEvent, CapturingPayload,
};
use rand::Rng;
use serial_test::serial;
use std::time::Duration;
use tokio::time::sleep;

fn get_pokemon_to_capture() -> String {
    let pokemons = vec!["Charizard", "Pikachu", "Regieleki"];
    pokemons[rand::thread_rng().gen_range(0..pokemons.len())].to_string()
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

#[tokio::test]
#[serial]
async fn event_stream_empty_stream() {
    let _child = common::run_server().await;
    let client = common::client();
    let stream = stream! {
        sleep(Duration::from_secs(10000)).await;
        // this line never hit
        yield Ok(AttemptCapturingPokemonEvent::Event(
            CapturingEvent::builder()
            .payload(CapturingPayload::builder()
                .name("Pikachu")
                .pokeball("Master Ball")
                .build())
            .build()
        ));
    };
    let _output = tokio::time::timeout(
        Duration::from_secs(1),
        client
            .capture_pokemon()
            .region("Kanto")
            .events(stream.into())
            .send(),
    )
    .await
    .expect("timed out")
    .expect("error from service");
}

#[tokio::test]
#[serial]
async fn event_stream_test() {
    let _child = common::run_server().await;
    let client = common::client();

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
    let mut output = common::client()
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
        let mut output = client
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

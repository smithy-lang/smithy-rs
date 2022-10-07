/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pokémon Service
//!
//! This crate implements the Pokémon Service.
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{atomic::AtomicUsize, Arc},
};

use async_stream::stream;
use aws_smithy_http_server::Extension;
use pokemon_service_server_sdk::{error, input, model, model::CapturingPayload, output, types::Blob};
use rand::Rng;
use tracing_subscriber::{prelude::*, EnvFilter};

#[doc(hidden)]
pub mod plugin;

const PIKACHU_ENGLISH_FLAVOR_TEXT: &str =
    "When several of these Pokémon gather, their electricity could build and cause lightning storms.";
const PIKACHU_SPANISH_FLAVOR_TEXT: &str =
    "Cuando varios de estos Pokémon se juntan, su energía puede causar fuertes tormentas.";
const PIKACHU_ITALIAN_FLAVOR_TEXT: &str =
    "Quando vari Pokémon di questo tipo si radunano, la loro energia può causare forti tempeste.";
const PIKACHU_JAPANESE_FLAVOR_TEXT: &str =
    "ほっぺたの りょうがわに ちいさい でんきぶくろを もつ。ピンチのときに ほうでんする。";

/// Setup `tracing::subscriber` to read the log level from RUST_LOG environment variable.
pub fn setup_tracing() {
    let format = tracing_subscriber::fmt::layer().pretty();
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    tracing_subscriber::registry().with(format).with(filter).init();
}

/// Structure holding the translations for a Pokémon description.
#[derive(Debug)]
struct PokemonTranslations {
    en: String,
    es: String,
    it: String,
    jp: String,
}

/// PokémonService shared state.
///
/// Some applications may want to manage state between handlers. Imagine having a database connection pool
/// that can be shared between different handlers and operation implementations.
/// State management can be expressed in a struct where the attributes hold the shared entities.
///
/// **NOTE: It is up to the implementation of the state structure to handle concurrency by protecting**
/// **its attributes using synchronization mechanisms.**
///
/// The framework stores the `Arc<T>` inside an [`http::Extensions`] and conveniently passes it to
/// the operation's implementation, making it able to handle operations with two different async signatures:
/// * `FnOnce(InputType) -> Future<OutputType>`
/// * `FnOnce(InputType, Extension<Arc<T>>) -> Future<OutputType>`
///
/// Wrapping the service with a [`tower::Layer`] will allow to have operations' signatures with and without shared state:
///
/// ```compile_fail
/// use std::sync::Arc;
/// use aws_smithy_http_server::{AddExtensionLayer, Extension, Router};
/// use tower::ServiceBuilder;
/// use tokio::sync::RwLock;
///
/// // Shared state,
/// #[derive(Debug, State)]
/// pub struct State {
///     pub count: RwLock<u64>
/// }
///
/// // Operation implementation with shared state.
/// async fn operation_with_state(input: Input, state: Extension<Arc<State>>) -> Output {
///     let mut count = state.0.write().await;
///     *count += 1;
///     Ok(Output::new())
/// }
///
/// // Operation implementation without shared state.
/// async fn operation_without_state(input: Input) -> Output {
///     Ok(Output::new())
/// }
///
/// let app: Router = OperationRegistryBuilder::default()
///     .operation_with_state(operation_with_state)
///     .operation_without_state(operation_without_state)
///     .build()
///     .unwrap()
///     .into();
/// let shared_state = Arc::new(State::default());
/// let app = app.layer(ServiceBuilder::new().layer(AddExtensionLayer::new(shared_state)));
/// let server = hyper::Server::bind(&"0.0.0.0:13734".parse().unwrap()).serve(app.into_make_service());
/// ...
/// ```
///
/// Without the middleware layer, the framework will require operations' signatures without
/// the shared state.
///
/// [`middleware`]: [`aws_smithy_http_server::AddExtensionLayer`]
#[derive(Debug)]
pub struct State {
    pokemons_translations: HashMap<String, PokemonTranslations>,
    call_count: AtomicUsize,
}

impl Default for State {
    fn default() -> Self {
        let mut pokemons_translations = HashMap::new();
        pokemons_translations.insert(
            String::from("pikachu"),
            PokemonTranslations {
                en: String::from(PIKACHU_ENGLISH_FLAVOR_TEXT),
                es: String::from(PIKACHU_SPANISH_FLAVOR_TEXT),
                it: String::from(PIKACHU_ITALIAN_FLAVOR_TEXT),
                jp: String::from(PIKACHU_JAPANESE_FLAVOR_TEXT),
            },
        );
        Self {
            pokemons_translations,
            call_count: Default::default(),
        }
    }
}

/// Retrieves information about a Pokémon species.
pub async fn get_pokemon_species(
    input: input::GetPokemonSpeciesInput,
    state: Extension<Arc<State>>,
) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
    state.0.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // We only support retrieving information about Pikachu.
    let pokemon = state.0.pokemons_translations.get(&input.name);
    match pokemon.as_ref() {
        Some(pokemon) => {
            tracing::debug!("Requested Pokémon is {}", input.name);
            let flavor_text_entries = vec![
                model::FlavorText {
                    flavor_text: pokemon.en.to_owned(),
                    language: model::Language::English,
                },
                model::FlavorText {
                    flavor_text: pokemon.es.to_owned(),
                    language: model::Language::Spanish,
                },
                model::FlavorText {
                    flavor_text: pokemon.it.to_owned(),
                    language: model::Language::Italian,
                },
                model::FlavorText {
                    flavor_text: pokemon.jp.to_owned(),
                    language: model::Language::Japanese,
                },
            ];
            let output = output::GetPokemonSpeciesOutput {
                name: String::from("pikachu"),
                flavor_text_entries,
            };
            Ok(output)
        }
        None => {
            tracing::error!("Requested Pokémon {} not available", input.name);
            Err(error::GetPokemonSpeciesError::ResourceNotFoundException(
                error::ResourceNotFoundException {
                    message: String::from("Requested Pokémon not available"),
                },
            ))
        }
    }
}

/// Retrieves the users storage.
pub async fn get_storage(
    input: input::GetStorageInput,
    _state: Extension<Arc<State>>,
) -> Result<output::GetStorageOutput, error::GetStorageError> {
    tracing::debug!("attempting to authenticate storage user");

    // We currently only support Ash and he has nothing stored
    if !(input.user == "ash" && input.passcode == "pikachu123") {
        tracing::debug!("authentication failed");
        return Err(error::GetStorageError::NotAuthorized(error::NotAuthorized {}));
    }
    Ok(output::GetStorageOutput { collection: vec![] })
}

/// Calculates and reports metrics about this server instance.
pub async fn get_server_statistics(
    _input: input::GetServerStatisticsInput,
    state: Extension<Arc<State>>,
) -> output::GetServerStatisticsOutput {
    // Read the current calls count.
    let counter = state.0.call_count.load(std::sync::atomic::Ordering::SeqCst);
    let calls_count = counter
        .try_into()
        .map_err(|e| {
            tracing::error!("Unable to convert u64 to i64: {}", e);
        })
        .unwrap_or(0);
    tracing::debug!("This instance served {} requests", counter);
    output::GetServerStatisticsOutput { calls_count }
}

/// Attempts to capture a Pokémon.
pub async fn capture_pokemon(
    mut input: input::CapturePokemonInput,
) -> Result<output::CapturePokemonOutput, error::CapturePokemonError> {
    if input.region != "Kanto" {
        return Err(error::CapturePokemonError::UnsupportedRegionError(
            error::UnsupportedRegionError::builder().build(),
        ));
    }
    let output_stream = stream! {
        loop {
            use std::time::Duration;
            match input.events.recv().await {
                Ok(maybe_event) => match maybe_event {
                    Some(event) => {
                        let capturing_event = event.as_event();
                        if let Ok(attempt) = capturing_event {
                            let payload = attempt.payload.clone().unwrap_or(CapturingPayload::builder().build());
                            let pokeball = payload.pokeball.as_ref().map(|ball| ball.as_str()).unwrap_or("");
                            if ! matches!(pokeball, "Master Ball" | "Great Ball" | "Fast Ball") {
                                yield Err(
                                    crate::error::CapturePokemonEventsError::InvalidPokeballError(
                                        crate::error::InvalidPokeballError::builder().pokeball(pokeball).build()
                                    )
                                );
                            } else {
                                let captured = match pokeball {
                                    "Master Ball" => true,
                                    "Great Ball" => rand::thread_rng().gen_range(0..100) > 33,
                                    "Fast Ball" => rand::thread_rng().gen_range(0..100) > 66,
                                    _ => unreachable!("invalid pokeball"),
                                };
                                // Only support Kanto
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                                // Will it capture the Pokémon?
                                if captured {
                                    let shiny = rand::thread_rng().gen_range(0..4096) == 0;
                                    let pokemon = payload
                                        .name
                                        .as_ref()
                                        .map(|name| name.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let pokedex: Vec<u8> = (0..255).collect();
                                    yield Ok(crate::model::CapturePokemonEvents::Event(
                                        crate::model::CaptureEvent::builder()
                                        .name(pokemon)
                                        .shiny(shiny)
                                        .pokedex_update(Blob::new(pokedex))
                                        .build(),
                                    ));
                                }
                            }
                        }
                    }
                    None => break,
                },
                Err(e) => println!("{:?}", e),
            }
        }
    };
    Ok(output::CapturePokemonOutput::builder()
        .events(output_stream.into())
        .build()
        .unwrap())
}

/// Empty operation used to benchmark the service.
pub async fn do_nothing(_input: input::DoNothingInput) -> output::DoNothingOutput {
    output::DoNothingOutput {}
}

/// Operation used to show the service is running.
pub async fn check_health(_input: input::CheckHealthInput) -> output::CheckHealthOutput {
    output::CheckHealthOutput {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_pokemon_species_pikachu_spanish_flavor_text() {
        let input = input::GetPokemonSpeciesInput {
            name: String::from("pikachu"),
        };

        let state = Arc::new(State::default());

        let actual_spanish_flavor_text = get_pokemon_species(input, Extension(state.clone()))
            .await
            .unwrap()
            .flavor_text_entries
            .into_iter()
            .find(|flavor_text| flavor_text.language == model::Language::Spanish)
            .unwrap();

        assert_eq!(PIKACHU_SPANISH_FLAVOR_TEXT, actual_spanish_flavor_text.flavor_text());

        let input = input::GetServerStatisticsInput {};
        let stats = get_server_statistics(input, Extension(state.clone())).await;
        assert_eq!(1, stats.calls_count);
    }
}

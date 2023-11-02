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
    process::Child,
    sync::{atomic::AtomicUsize, Arc},
};

use async_stream::stream;
use aws_smithy_http_server::Extension;
use aws_smithy_runtime::client::http::hyper_014::HyperConnector;
use aws_smithy_runtime_api::client::http::HttpConnector;
use http::Uri;
use pokemon_service_server_sdk::{
    error, input, model,
    model::CapturingPayload,
    output,
    types::{Blob, ByteStream, SdkBody},
};
use rand::{seq::SliceRandom, Rng};
use tracing_subscriber::{prelude::*, EnvFilter};

const PIKACHU_ENGLISH_FLAVOR_TEXT: &str =
    "When several of these Pokémon gather, their electricity could build and cause lightning storms.";
const PIKACHU_SPANISH_FLAVOR_TEXT: &str =
    "Cuando varios de estos Pokémon se juntan, su energía puede causar fuertes tormentas.";
const PIKACHU_ITALIAN_FLAVOR_TEXT: &str =
    "Quando vari Pokémon di questo tipo si radunano, la loro energia può causare forti tempeste.";
const PIKACHU_JAPANESE_FLAVOR_TEXT: &str =
    "ほっぺたの りょうがわに ちいさい でんきぶくろを もつ。ピンチのときに ほうでんする。";

/// Kills [`Child`] process when dropped.
#[derive(Debug)]
#[must_use]
pub struct ChildDrop(pub Child);

impl Drop for ChildDrop {
    fn drop(&mut self) {
        self.0.kill().expect("failed to kill process")
    }
}

/// Setup `tracing::subscriber` to read the log level from RUST_LOG environment variable.
pub fn setup_tracing() {
    let format = tracing_subscriber::fmt::layer().json();
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    tracing_subscriber::registry()
        .with(format)
        .with(filter)
        .init();
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
/// The framework stores the `Arc<T>` inside an `http::Extensions` and conveniently passes it to
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
    state
        .0
        .call_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

/// Retrieves the user's storage.
pub async fn get_storage(
    input: input::GetStorageInput,
    _state: Extension<Arc<State>>,
) -> Result<output::GetStorageOutput, error::GetStorageError> {
    tracing::debug!("attempting to authenticate storage user");

    // We currently only support Ash and he has nothing stored
    if !(input.user == "ash" && input.passcode == "pikachu123") {
        tracing::debug!("authentication failed");
        return Err(error::GetStorageError::StorageAccessNotAuthorized(
            error::StorageAccessNotAuthorized {},
        ));
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
            error::UnsupportedRegionError {
                region: input.region,
            },
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
                            let payload = attempt.payload.clone().unwrap_or_else(|| CapturingPayload::builder().build());
                            let pokeball = payload.pokeball().unwrap_or("");
                            if ! matches!(pokeball, "Master Ball" | "Great Ball" | "Fast Ball") {
                                yield Err(
                                    crate::error::CapturePokemonEventsError::InvalidPokeballError(
                                        crate::error::InvalidPokeballError {
                                            pokeball: pokeball.to_owned()
                                        }
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
                                        .name()
                                        .unwrap_or("")
                                        .to_string();
                                    let pokedex: Vec<u8> = (0..255).collect();
                                    yield Ok(crate::model::CapturePokemonEvents::Event(
                                        crate::model::CaptureEvent {
                                            name: Some(pokemon),
                                            shiny: Some(shiny),
                                            pokedex_update: Some(Blob::new(pokedex)),
                                            captured: Some(true),
                                        }
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

const RADIO_STREAMS: [&str; 2] = [
    "https://ia800107.us.archive.org/33/items/299SoundEffectCollection/102%20Palette%20Town%20Theme.mp3",
    "https://ia600408.us.archive.org/29/items/PocketMonstersGreenBetaLavenderTownMusicwwwFlvtoCom/Pocket%20Monsters%20Green%20Beta-%20Lavender%20Town%20Music-%5Bwww_flvto_com%5D.mp3",
];

/// Streams a random Pokémon song.
pub async fn stream_pokemon_radio(
    _input: input::StreamPokemonRadioInput,
) -> output::StreamPokemonRadioOutput {
    let radio_stream_url = RADIO_STREAMS
        .choose(&mut rand::thread_rng())
        .expect("`RADIO_STREAMS` is empty")
        .parse::<Uri>()
        .expect("Invalid url in `RADIO_STREAMS`");

    let connector = HyperConnector::builder().build_https();
    let result = connector
        .call(
            http::Request::builder()
                .uri(radio_stream_url)
                .body(SdkBody::empty())
                .unwrap()
                .try_into()
                .unwrap(),
        )
        .await
        .unwrap();

    output::StreamPokemonRadioOutput {
        data: ByteStream::new(result.into_body()),
    }
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

        assert_eq!(
            PIKACHU_SPANISH_FLAVOR_TEXT,
            actual_spanish_flavor_text.flavor_text()
        );

        let input = input::GetServerStatisticsInput {};
        let stats = get_server_statistics(input, Extension(state.clone())).await;
        assert_eq!(1, stats.calls_count);
    }
}


        /// get_custom_op0
        pub async fn get_custom_op0(
            _input: input::CustomOp0Input,
        ) -> Result<output::CustomOp0Output, error::CustomOp0Error> {
        Ok(output::CustomOp0Output { output: "0".into() })
        }


        /// get_custom_op1
        pub async fn get_custom_op1(
            _input: input::CustomOp1Input,
        ) -> Result<output::CustomOp1Output, error::CustomOp1Error> {
        Ok(output::CustomOp1Output { output: "1".into() })
        }


        /// get_custom_op2
        pub async fn get_custom_op2(
            _input: input::CustomOp2Input,
        ) -> Result<output::CustomOp2Output, error::CustomOp2Error> {
        Ok(output::CustomOp2Output { output: "2".into() })
        }


        /// get_custom_op3
        pub async fn get_custom_op3(
            _input: input::CustomOp3Input,
        ) -> Result<output::CustomOp3Output, error::CustomOp3Error> {
        Ok(output::CustomOp3Output { output: "3".into() })
        }


        /// get_custom_op4
        pub async fn get_custom_op4(
            _input: input::CustomOp4Input,
        ) -> Result<output::CustomOp4Output, error::CustomOp4Error> {
        Ok(output::CustomOp4Output { output: "4".into() })
        }


        /// get_custom_op5
        pub async fn get_custom_op5(
            _input: input::CustomOp5Input,
        ) -> Result<output::CustomOp5Output, error::CustomOp5Error> {
        Ok(output::CustomOp5Output { output: "5".into() })
        }


        /// get_custom_op6
        pub async fn get_custom_op6(
            _input: input::CustomOp6Input,
        ) -> Result<output::CustomOp6Output, error::CustomOp6Error> {
        Ok(output::CustomOp6Output { output: "6".into() })
        }


        /// get_custom_op7
        pub async fn get_custom_op7(
            _input: input::CustomOp7Input,
        ) -> Result<output::CustomOp7Output, error::CustomOp7Error> {
        Ok(output::CustomOp7Output { output: "7".into() })
        }


        /// get_custom_op8
        pub async fn get_custom_op8(
            _input: input::CustomOp8Input,
        ) -> Result<output::CustomOp8Output, error::CustomOp8Error> {
        Ok(output::CustomOp8Output { output: "8".into() })
        }


        /// get_custom_op9
        pub async fn get_custom_op9(
            _input: input::CustomOp9Input,
        ) -> Result<output::CustomOp9Output, error::CustomOp9Error> {
        Ok(output::CustomOp9Output { output: "9".into() })
        }


        /// get_custom_op10
        pub async fn get_custom_op10(
            _input: input::CustomOp10Input,
        ) -> Result<output::CustomOp10Output, error::CustomOp10Error> {
        Ok(output::CustomOp10Output { output: "10".into() })
        }


        /// get_custom_op11
        pub async fn get_custom_op11(
            _input: input::CustomOp11Input,
        ) -> Result<output::CustomOp11Output, error::CustomOp11Error> {
        Ok(output::CustomOp11Output { output: "11".into() })
        }


        /// get_custom_op12
        pub async fn get_custom_op12(
            _input: input::CustomOp12Input,
        ) -> Result<output::CustomOp12Output, error::CustomOp12Error> {
        Ok(output::CustomOp12Output { output: "12".into() })
        }


        /// get_custom_op13
        pub async fn get_custom_op13(
            _input: input::CustomOp13Input,
        ) -> Result<output::CustomOp13Output, error::CustomOp13Error> {
        Ok(output::CustomOp13Output { output: "13".into() })
        }


        /// get_custom_op14
        pub async fn get_custom_op14(
            _input: input::CustomOp14Input,
        ) -> Result<output::CustomOp14Output, error::CustomOp14Error> {
        Ok(output::CustomOp14Output { output: "14".into() })
        }


        /// get_custom_op15
        pub async fn get_custom_op15(
            _input: input::CustomOp15Input,
        ) -> Result<output::CustomOp15Output, error::CustomOp15Error> {
        Ok(output::CustomOp15Output { output: "15".into() })
        }


        /// get_custom_op16
        pub async fn get_custom_op16(
            _input: input::CustomOp16Input,
        ) -> Result<output::CustomOp16Output, error::CustomOp16Error> {
        Ok(output::CustomOp16Output { output: "16".into() })
        }


        /// get_custom_op17
        pub async fn get_custom_op17(
            _input: input::CustomOp17Input,
        ) -> Result<output::CustomOp17Output, error::CustomOp17Error> {
        Ok(output::CustomOp17Output { output: "17".into() })
        }


        /// get_custom_op18
        pub async fn get_custom_op18(
            _input: input::CustomOp18Input,
        ) -> Result<output::CustomOp18Output, error::CustomOp18Error> {
        Ok(output::CustomOp18Output { output: "18".into() })
        }


        /// get_custom_op19
        pub async fn get_custom_op19(
            _input: input::CustomOp19Input,
        ) -> Result<output::CustomOp19Output, error::CustomOp19Error> {
        Ok(output::CustomOp19Output { output: "19".into() })
        }


        /// get_custom_op20
        pub async fn get_custom_op20(
            _input: input::CustomOp20Input,
        ) -> Result<output::CustomOp20Output, error::CustomOp20Error> {
        Ok(output::CustomOp20Output { output: "20".into() })
        }


        /// get_custom_op21
        pub async fn get_custom_op21(
            _input: input::CustomOp21Input,
        ) -> Result<output::CustomOp21Output, error::CustomOp21Error> {
        Ok(output::CustomOp21Output { output: "21".into() })
        }


        /// get_custom_op22
        pub async fn get_custom_op22(
            _input: input::CustomOp22Input,
        ) -> Result<output::CustomOp22Output, error::CustomOp22Error> {
        Ok(output::CustomOp22Output { output: "22".into() })
        }


        /// get_custom_op23
        pub async fn get_custom_op23(
            _input: input::CustomOp23Input,
        ) -> Result<output::CustomOp23Output, error::CustomOp23Error> {
        Ok(output::CustomOp23Output { output: "23".into() })
        }


        /// get_custom_op24
        pub async fn get_custom_op24(
            _input: input::CustomOp24Input,
        ) -> Result<output::CustomOp24Output, error::CustomOp24Error> {
        Ok(output::CustomOp24Output { output: "24".into() })
        }


        /// get_custom_op25
        pub async fn get_custom_op25(
            _input: input::CustomOp25Input,
        ) -> Result<output::CustomOp25Output, error::CustomOp25Error> {
        Ok(output::CustomOp25Output { output: "25".into() })
        }


        /// get_custom_op26
        pub async fn get_custom_op26(
            _input: input::CustomOp26Input,
        ) -> Result<output::CustomOp26Output, error::CustomOp26Error> {
        Ok(output::CustomOp26Output { output: "26".into() })
        }


        /// get_custom_op27
        pub async fn get_custom_op27(
            _input: input::CustomOp27Input,
        ) -> Result<output::CustomOp27Output, error::CustomOp27Error> {
        Ok(output::CustomOp27Output { output: "27".into() })
        }


        /// get_custom_op28
        pub async fn get_custom_op28(
            _input: input::CustomOp28Input,
        ) -> Result<output::CustomOp28Output, error::CustomOp28Error> {
        Ok(output::CustomOp28Output { output: "28".into() })
        }


        /// get_custom_op29
        pub async fn get_custom_op29(
            _input: input::CustomOp29Input,
        ) -> Result<output::CustomOp29Output, error::CustomOp29Error> {
        Ok(output::CustomOp29Output { output: "29".into() })
        }


        /// get_custom_op30
        pub async fn get_custom_op30(
            _input: input::CustomOp30Input,
        ) -> Result<output::CustomOp30Output, error::CustomOp30Error> {
        Ok(output::CustomOp30Output { output: "30".into() })
        }


        /// get_custom_op31
        pub async fn get_custom_op31(
            _input: input::CustomOp31Input,
        ) -> Result<output::CustomOp31Output, error::CustomOp31Error> {
        Ok(output::CustomOp31Output { output: "31".into() })
        }


        /// get_custom_op32
        pub async fn get_custom_op32(
            _input: input::CustomOp32Input,
        ) -> Result<output::CustomOp32Output, error::CustomOp32Error> {
        Ok(output::CustomOp32Output { output: "32".into() })
        }


        /// get_custom_op33
        pub async fn get_custom_op33(
            _input: input::CustomOp33Input,
        ) -> Result<output::CustomOp33Output, error::CustomOp33Error> {
        Ok(output::CustomOp33Output { output: "33".into() })
        }


        /// get_custom_op34
        pub async fn get_custom_op34(
            _input: input::CustomOp34Input,
        ) -> Result<output::CustomOp34Output, error::CustomOp34Error> {
        Ok(output::CustomOp34Output { output: "34".into() })
        }


        /// get_custom_op35
        pub async fn get_custom_op35(
            _input: input::CustomOp35Input,
        ) -> Result<output::CustomOp35Output, error::CustomOp35Error> {
        Ok(output::CustomOp35Output { output: "35".into() })
        }


        /// get_custom_op36
        pub async fn get_custom_op36(
            _input: input::CustomOp36Input,
        ) -> Result<output::CustomOp36Output, error::CustomOp36Error> {
        Ok(output::CustomOp36Output { output: "36".into() })
        }


        /// get_custom_op37
        pub async fn get_custom_op37(
            _input: input::CustomOp37Input,
        ) -> Result<output::CustomOp37Output, error::CustomOp37Error> {
        Ok(output::CustomOp37Output { output: "37".into() })
        }


        /// get_custom_op38
        pub async fn get_custom_op38(
            _input: input::CustomOp38Input,
        ) -> Result<output::CustomOp38Output, error::CustomOp38Error> {
        Ok(output::CustomOp38Output { output: "38".into() })
        }


        /// get_custom_op39
        pub async fn get_custom_op39(
            _input: input::CustomOp39Input,
        ) -> Result<output::CustomOp39Output, error::CustomOp39Error> {
        Ok(output::CustomOp39Output { output: "39".into() })
        }


        /// get_custom_op40
        pub async fn get_custom_op40(
            _input: input::CustomOp40Input,
        ) -> Result<output::CustomOp40Output, error::CustomOp40Error> {
        Ok(output::CustomOp40Output { output: "40".into() })
        }


        /// get_custom_op41
        pub async fn get_custom_op41(
            _input: input::CustomOp41Input,
        ) -> Result<output::CustomOp41Output, error::CustomOp41Error> {
        Ok(output::CustomOp41Output { output: "41".into() })
        }


        /// get_custom_op42
        pub async fn get_custom_op42(
            _input: input::CustomOp42Input,
        ) -> Result<output::CustomOp42Output, error::CustomOp42Error> {
        Ok(output::CustomOp42Output { output: "42".into() })
        }


        /// get_custom_op43
        pub async fn get_custom_op43(
            _input: input::CustomOp43Input,
        ) -> Result<output::CustomOp43Output, error::CustomOp43Error> {
        Ok(output::CustomOp43Output { output: "43".into() })
        }


        /// get_custom_op44
        pub async fn get_custom_op44(
            _input: input::CustomOp44Input,
        ) -> Result<output::CustomOp44Output, error::CustomOp44Error> {
        Ok(output::CustomOp44Output { output: "44".into() })
        }


        /// get_custom_op45
        pub async fn get_custom_op45(
            _input: input::CustomOp45Input,
        ) -> Result<output::CustomOp45Output, error::CustomOp45Error> {
        Ok(output::CustomOp45Output { output: "45".into() })
        }


        /// get_custom_op46
        pub async fn get_custom_op46(
            _input: input::CustomOp46Input,
        ) -> Result<output::CustomOp46Output, error::CustomOp46Error> {
        Ok(output::CustomOp46Output { output: "46".into() })
        }


        /// get_custom_op47
        pub async fn get_custom_op47(
            _input: input::CustomOp47Input,
        ) -> Result<output::CustomOp47Output, error::CustomOp47Error> {
        Ok(output::CustomOp47Output { output: "47".into() })
        }


        /// get_custom_op48
        pub async fn get_custom_op48(
            _input: input::CustomOp48Input,
        ) -> Result<output::CustomOp48Output, error::CustomOp48Error> {
        Ok(output::CustomOp48Output { output: "48".into() })
        }


        /// get_custom_op49
        pub async fn get_custom_op49(
            _input: input::CustomOp49Input,
        ) -> Result<output::CustomOp49Output, error::CustomOp49Error> {
        Ok(output::CustomOp49Output { output: "49".into() })
        }


        /// get_custom_op50
        pub async fn get_custom_op50(
            _input: input::CustomOp50Input,
        ) -> Result<output::CustomOp50Output, error::CustomOp50Error> {
        Ok(output::CustomOp50Output { output: "50".into() })
        }


        /// get_custom_op51
        pub async fn get_custom_op51(
            _input: input::CustomOp51Input,
        ) -> Result<output::CustomOp51Output, error::CustomOp51Error> {
        Ok(output::CustomOp51Output { output: "51".into() })
        }


        /// get_custom_op52
        pub async fn get_custom_op52(
            _input: input::CustomOp52Input,
        ) -> Result<output::CustomOp52Output, error::CustomOp52Error> {
        Ok(output::CustomOp52Output { output: "52".into() })
        }


        /// get_custom_op53
        pub async fn get_custom_op53(
            _input: input::CustomOp53Input,
        ) -> Result<output::CustomOp53Output, error::CustomOp53Error> {
        Ok(output::CustomOp53Output { output: "53".into() })
        }


        /// get_custom_op54
        pub async fn get_custom_op54(
            _input: input::CustomOp54Input,
        ) -> Result<output::CustomOp54Output, error::CustomOp54Error> {
        Ok(output::CustomOp54Output { output: "54".into() })
        }


        /// get_custom_op55
        pub async fn get_custom_op55(
            _input: input::CustomOp55Input,
        ) -> Result<output::CustomOp55Output, error::CustomOp55Error> {
        Ok(output::CustomOp55Output { output: "55".into() })
        }


        /// get_custom_op56
        pub async fn get_custom_op56(
            _input: input::CustomOp56Input,
        ) -> Result<output::CustomOp56Output, error::CustomOp56Error> {
        Ok(output::CustomOp56Output { output: "56".into() })
        }


        /// get_custom_op57
        pub async fn get_custom_op57(
            _input: input::CustomOp57Input,
        ) -> Result<output::CustomOp57Output, error::CustomOp57Error> {
        Ok(output::CustomOp57Output { output: "57".into() })
        }


        /// get_custom_op58
        pub async fn get_custom_op58(
            _input: input::CustomOp58Input,
        ) -> Result<output::CustomOp58Output, error::CustomOp58Error> {
        Ok(output::CustomOp58Output { output: "58".into() })
        }


        /// get_custom_op59
        pub async fn get_custom_op59(
            _input: input::CustomOp59Input,
        ) -> Result<output::CustomOp59Output, error::CustomOp59Error> {
        Ok(output::CustomOp59Output { output: "59".into() })
        }


        /// get_custom_op60
        pub async fn get_custom_op60(
            _input: input::CustomOp60Input,
        ) -> Result<output::CustomOp60Output, error::CustomOp60Error> {
        Ok(output::CustomOp60Output { output: "60".into() })
        }


        /// get_custom_op61
        pub async fn get_custom_op61(
            _input: input::CustomOp61Input,
        ) -> Result<output::CustomOp61Output, error::CustomOp61Error> {
        Ok(output::CustomOp61Output { output: "61".into() })
        }


        /// get_custom_op62
        pub async fn get_custom_op62(
            _input: input::CustomOp62Input,
        ) -> Result<output::CustomOp62Output, error::CustomOp62Error> {
        Ok(output::CustomOp62Output { output: "62".into() })
        }


        /// get_custom_op63
        pub async fn get_custom_op63(
            _input: input::CustomOp63Input,
        ) -> Result<output::CustomOp63Output, error::CustomOp63Error> {
        Ok(output::CustomOp63Output { output: "63".into() })
        }


        /// get_custom_op64
        pub async fn get_custom_op64(
            _input: input::CustomOp64Input,
        ) -> Result<output::CustomOp64Output, error::CustomOp64Error> {
        Ok(output::CustomOp64Output { output: "64".into() })
        }


        /// get_custom_op65
        pub async fn get_custom_op65(
            _input: input::CustomOp65Input,
        ) -> Result<output::CustomOp65Output, error::CustomOp65Error> {
        Ok(output::CustomOp65Output { output: "65".into() })
        }


        /// get_custom_op66
        pub async fn get_custom_op66(
            _input: input::CustomOp66Input,
        ) -> Result<output::CustomOp66Output, error::CustomOp66Error> {
        Ok(output::CustomOp66Output { output: "66".into() })
        }


        /// get_custom_op67
        pub async fn get_custom_op67(
            _input: input::CustomOp67Input,
        ) -> Result<output::CustomOp67Output, error::CustomOp67Error> {
        Ok(output::CustomOp67Output { output: "67".into() })
        }


        /// get_custom_op68
        pub async fn get_custom_op68(
            _input: input::CustomOp68Input,
        ) -> Result<output::CustomOp68Output, error::CustomOp68Error> {
        Ok(output::CustomOp68Output { output: "68".into() })
        }


        /// get_custom_op69
        pub async fn get_custom_op69(
            _input: input::CustomOp69Input,
        ) -> Result<output::CustomOp69Output, error::CustomOp69Error> {
        Ok(output::CustomOp69Output { output: "69".into() })
        }


        /// get_custom_op70
        pub async fn get_custom_op70(
            _input: input::CustomOp70Input,
        ) -> Result<output::CustomOp70Output, error::CustomOp70Error> {
        Ok(output::CustomOp70Output { output: "70".into() })
        }


        /// get_custom_op71
        pub async fn get_custom_op71(
            _input: input::CustomOp71Input,
        ) -> Result<output::CustomOp71Output, error::CustomOp71Error> {
        Ok(output::CustomOp71Output { output: "71".into() })
        }


        /// get_custom_op72
        pub async fn get_custom_op72(
            _input: input::CustomOp72Input,
        ) -> Result<output::CustomOp72Output, error::CustomOp72Error> {
        Ok(output::CustomOp72Output { output: "72".into() })
        }


        /// get_custom_op73
        pub async fn get_custom_op73(
            _input: input::CustomOp73Input,
        ) -> Result<output::CustomOp73Output, error::CustomOp73Error> {
        Ok(output::CustomOp73Output { output: "73".into() })
        }


        /// get_custom_op74
        pub async fn get_custom_op74(
            _input: input::CustomOp74Input,
        ) -> Result<output::CustomOp74Output, error::CustomOp74Error> {
        Ok(output::CustomOp74Output { output: "74".into() })
        }


        /// get_custom_op75
        pub async fn get_custom_op75(
            _input: input::CustomOp75Input,
        ) -> Result<output::CustomOp75Output, error::CustomOp75Error> {
        Ok(output::CustomOp75Output { output: "75".into() })
        }


        /// get_custom_op76
        pub async fn get_custom_op76(
            _input: input::CustomOp76Input,
        ) -> Result<output::CustomOp76Output, error::CustomOp76Error> {
        Ok(output::CustomOp76Output { output: "76".into() })
        }


        /// get_custom_op77
        pub async fn get_custom_op77(
            _input: input::CustomOp77Input,
        ) -> Result<output::CustomOp77Output, error::CustomOp77Error> {
        Ok(output::CustomOp77Output { output: "77".into() })
        }


        /// get_custom_op78
        pub async fn get_custom_op78(
            _input: input::CustomOp78Input,
        ) -> Result<output::CustomOp78Output, error::CustomOp78Error> {
        Ok(output::CustomOp78Output { output: "78".into() })
        }


        /// get_custom_op79
        pub async fn get_custom_op79(
            _input: input::CustomOp79Input,
        ) -> Result<output::CustomOp79Output, error::CustomOp79Error> {
        Ok(output::CustomOp79Output { output: "79".into() })
        }


        /// get_custom_op80
        pub async fn get_custom_op80(
            _input: input::CustomOp80Input,
        ) -> Result<output::CustomOp80Output, error::CustomOp80Error> {
        Ok(output::CustomOp80Output { output: "80".into() })
        }


        /// get_custom_op81
        pub async fn get_custom_op81(
            _input: input::CustomOp81Input,
        ) -> Result<output::CustomOp81Output, error::CustomOp81Error> {
        Ok(output::CustomOp81Output { output: "81".into() })
        }


        /// get_custom_op82
        pub async fn get_custom_op82(
            _input: input::CustomOp82Input,
        ) -> Result<output::CustomOp82Output, error::CustomOp82Error> {
        Ok(output::CustomOp82Output { output: "82".into() })
        }


        /// get_custom_op83
        pub async fn get_custom_op83(
            _input: input::CustomOp83Input,
        ) -> Result<output::CustomOp83Output, error::CustomOp83Error> {
        Ok(output::CustomOp83Output { output: "83".into() })
        }


        /// get_custom_op84
        pub async fn get_custom_op84(
            _input: input::CustomOp84Input,
        ) -> Result<output::CustomOp84Output, error::CustomOp84Error> {
        Ok(output::CustomOp84Output { output: "84".into() })
        }


        /// get_custom_op85
        pub async fn get_custom_op85(
            _input: input::CustomOp85Input,
        ) -> Result<output::CustomOp85Output, error::CustomOp85Error> {
        Ok(output::CustomOp85Output { output: "85".into() })
        }


        /// get_custom_op86
        pub async fn get_custom_op86(
            _input: input::CustomOp86Input,
        ) -> Result<output::CustomOp86Output, error::CustomOp86Error> {
        Ok(output::CustomOp86Output { output: "86".into() })
        }


        /// get_custom_op87
        pub async fn get_custom_op87(
            _input: input::CustomOp87Input,
        ) -> Result<output::CustomOp87Output, error::CustomOp87Error> {
        Ok(output::CustomOp87Output { output: "87".into() })
        }


        /// get_custom_op88
        pub async fn get_custom_op88(
            _input: input::CustomOp88Input,
        ) -> Result<output::CustomOp88Output, error::CustomOp88Error> {
        Ok(output::CustomOp88Output { output: "88".into() })
        }


        /// get_custom_op89
        pub async fn get_custom_op89(
            _input: input::CustomOp89Input,
        ) -> Result<output::CustomOp89Output, error::CustomOp89Error> {
        Ok(output::CustomOp89Output { output: "89".into() })
        }


        /// get_custom_op90
        pub async fn get_custom_op90(
            _input: input::CustomOp90Input,
        ) -> Result<output::CustomOp90Output, error::CustomOp90Error> {
        Ok(output::CustomOp90Output { output: "90".into() })
        }


        /// get_custom_op91
        pub async fn get_custom_op91(
            _input: input::CustomOp91Input,
        ) -> Result<output::CustomOp91Output, error::CustomOp91Error> {
        Ok(output::CustomOp91Output { output: "91".into() })
        }


        /// get_custom_op92
        pub async fn get_custom_op92(
            _input: input::CustomOp92Input,
        ) -> Result<output::CustomOp92Output, error::CustomOp92Error> {
        Ok(output::CustomOp92Output { output: "92".into() })
        }


        /// get_custom_op93
        pub async fn get_custom_op93(
            _input: input::CustomOp93Input,
        ) -> Result<output::CustomOp93Output, error::CustomOp93Error> {
        Ok(output::CustomOp93Output { output: "93".into() })
        }


        /// get_custom_op94
        pub async fn get_custom_op94(
            _input: input::CustomOp94Input,
        ) -> Result<output::CustomOp94Output, error::CustomOp94Error> {
        Ok(output::CustomOp94Output { output: "94".into() })
        }


        /// get_custom_op95
        pub async fn get_custom_op95(
            _input: input::CustomOp95Input,
        ) -> Result<output::CustomOp95Output, error::CustomOp95Error> {
        Ok(output::CustomOp95Output { output: "95".into() })
        }


        /// get_custom_op96
        pub async fn get_custom_op96(
            _input: input::CustomOp96Input,
        ) -> Result<output::CustomOp96Output, error::CustomOp96Error> {
        Ok(output::CustomOp96Output { output: "96".into() })
        }


        /// get_custom_op97
        pub async fn get_custom_op97(
            _input: input::CustomOp97Input,
        ) -> Result<output::CustomOp97Output, error::CustomOp97Error> {
        Ok(output::CustomOp97Output { output: "97".into() })
        }


        /// get_custom_op98
        pub async fn get_custom_op98(
            _input: input::CustomOp98Input,
        ) -> Result<output::CustomOp98Output, error::CustomOp98Error> {
        Ok(output::CustomOp98Output { output: "98".into() })
        }


        /// get_custom_op99
        pub async fn get_custom_op99(
            _input: input::CustomOp99Input,
        ) -> Result<output::CustomOp99Output, error::CustomOp99Error> {
        Ok(output::CustomOp99Output { output: "99".into() })
        }


        /// get_custom_op100
        pub async fn get_custom_op100(
            _input: input::CustomOp100Input,
        ) -> Result<output::CustomOp100Output, error::CustomOp100Error> {
        Ok(output::CustomOp100Output { output: "100".into() })
        }


        /// get_custom_op101
        pub async fn get_custom_op101(
            _input: input::CustomOp101Input,
        ) -> Result<output::CustomOp101Output, error::CustomOp101Error> {
        Ok(output::CustomOp101Output { output: "101".into() })
        }


        /// get_custom_op102
        pub async fn get_custom_op102(
            _input: input::CustomOp102Input,
        ) -> Result<output::CustomOp102Output, error::CustomOp102Error> {
        Ok(output::CustomOp102Output { output: "102".into() })
        }


        /// get_custom_op103
        pub async fn get_custom_op103(
            _input: input::CustomOp103Input,
        ) -> Result<output::CustomOp103Output, error::CustomOp103Error> {
        Ok(output::CustomOp103Output { output: "103".into() })
        }


        /// get_custom_op104
        pub async fn get_custom_op104(
            _input: input::CustomOp104Input,
        ) -> Result<output::CustomOp104Output, error::CustomOp104Error> {
        Ok(output::CustomOp104Output { output: "104".into() })
        }


        /// get_custom_op105
        pub async fn get_custom_op105(
            _input: input::CustomOp105Input,
        ) -> Result<output::CustomOp105Output, error::CustomOp105Error> {
        Ok(output::CustomOp105Output { output: "105".into() })
        }


        /// get_custom_op106
        pub async fn get_custom_op106(
            _input: input::CustomOp106Input,
        ) -> Result<output::CustomOp106Output, error::CustomOp106Error> {
        Ok(output::CustomOp106Output { output: "106".into() })
        }


        /// get_custom_op107
        pub async fn get_custom_op107(
            _input: input::CustomOp107Input,
        ) -> Result<output::CustomOp107Output, error::CustomOp107Error> {
        Ok(output::CustomOp107Output { output: "107".into() })
        }


        /// get_custom_op108
        pub async fn get_custom_op108(
            _input: input::CustomOp108Input,
        ) -> Result<output::CustomOp108Output, error::CustomOp108Error> {
        Ok(output::CustomOp108Output { output: "108".into() })
        }


        /// get_custom_op109
        pub async fn get_custom_op109(
            _input: input::CustomOp109Input,
        ) -> Result<output::CustomOp109Output, error::CustomOp109Error> {
        Ok(output::CustomOp109Output { output: "109".into() })
        }


        /// get_custom_op110
        pub async fn get_custom_op110(
            _input: input::CustomOp110Input,
        ) -> Result<output::CustomOp110Output, error::CustomOp110Error> {
        Ok(output::CustomOp110Output { output: "110".into() })
        }


        /// get_custom_op111
        pub async fn get_custom_op111(
            _input: input::CustomOp111Input,
        ) -> Result<output::CustomOp111Output, error::CustomOp111Error> {
        Ok(output::CustomOp111Output { output: "111".into() })
        }


        /// get_custom_op112
        pub async fn get_custom_op112(
            _input: input::CustomOp112Input,
        ) -> Result<output::CustomOp112Output, error::CustomOp112Error> {
        Ok(output::CustomOp112Output { output: "112".into() })
        }


        /// get_custom_op113
        pub async fn get_custom_op113(
            _input: input::CustomOp113Input,
        ) -> Result<output::CustomOp113Output, error::CustomOp113Error> {
        Ok(output::CustomOp113Output { output: "113".into() })
        }


        /// get_custom_op114
        pub async fn get_custom_op114(
            _input: input::CustomOp114Input,
        ) -> Result<output::CustomOp114Output, error::CustomOp114Error> {
        Ok(output::CustomOp114Output { output: "114".into() })
        }


        /// get_custom_op115
        pub async fn get_custom_op115(
            _input: input::CustomOp115Input,
        ) -> Result<output::CustomOp115Output, error::CustomOp115Error> {
        Ok(output::CustomOp115Output { output: "115".into() })
        }


        /// get_custom_op116
        pub async fn get_custom_op116(
            _input: input::CustomOp116Input,
        ) -> Result<output::CustomOp116Output, error::CustomOp116Error> {
        Ok(output::CustomOp116Output { output: "116".into() })
        }


        /// get_custom_op117
        pub async fn get_custom_op117(
            _input: input::CustomOp117Input,
        ) -> Result<output::CustomOp117Output, error::CustomOp117Error> {
        Ok(output::CustomOp117Output { output: "117".into() })
        }


        /// get_custom_op118
        pub async fn get_custom_op118(
            _input: input::CustomOp118Input,
        ) -> Result<output::CustomOp118Output, error::CustomOp118Error> {
        Ok(output::CustomOp118Output { output: "118".into() })
        }


        /// get_custom_op119
        pub async fn get_custom_op119(
            _input: input::CustomOp119Input,
        ) -> Result<output::CustomOp119Output, error::CustomOp119Error> {
        Ok(output::CustomOp119Output { output: "119".into() })
        }


        /// get_custom_op120
        pub async fn get_custom_op120(
            _input: input::CustomOp120Input,
        ) -> Result<output::CustomOp120Output, error::CustomOp120Error> {
        Ok(output::CustomOp120Output { output: "120".into() })
        }


        /// get_custom_op121
        pub async fn get_custom_op121(
            _input: input::CustomOp121Input,
        ) -> Result<output::CustomOp121Output, error::CustomOp121Error> {
        Ok(output::CustomOp121Output { output: "121".into() })
        }


        /// get_custom_op122
        pub async fn get_custom_op122(
            _input: input::CustomOp122Input,
        ) -> Result<output::CustomOp122Output, error::CustomOp122Error> {
        Ok(output::CustomOp122Output { output: "122".into() })
        }


        /// get_custom_op123
        pub async fn get_custom_op123(
            _input: input::CustomOp123Input,
        ) -> Result<output::CustomOp123Output, error::CustomOp123Error> {
        Ok(output::CustomOp123Output { output: "123".into() })
        }


        /// get_custom_op124
        pub async fn get_custom_op124(
            _input: input::CustomOp124Input,
        ) -> Result<output::CustomOp124Output, error::CustomOp124Error> {
        Ok(output::CustomOp124Output { output: "124".into() })
        }


        /// get_custom_op125
        pub async fn get_custom_op125(
            _input: input::CustomOp125Input,
        ) -> Result<output::CustomOp125Output, error::CustomOp125Error> {
        Ok(output::CustomOp125Output { output: "125".into() })
        }


        /// get_custom_op126
        pub async fn get_custom_op126(
            _input: input::CustomOp126Input,
        ) -> Result<output::CustomOp126Output, error::CustomOp126Error> {
        Ok(output::CustomOp126Output { output: "126".into() })
        }


        /// get_custom_op127
        pub async fn get_custom_op127(
            _input: input::CustomOp127Input,
        ) -> Result<output::CustomOp127Output, error::CustomOp127Error> {
        Ok(output::CustomOp127Output { output: "127".into() })
        }


        /// get_custom_op128
        pub async fn get_custom_op128(
            _input: input::CustomOp128Input,
        ) -> Result<output::CustomOp128Output, error::CustomOp128Error> {
        Ok(output::CustomOp128Output { output: "128".into() })
        }


        /// get_custom_op129
        pub async fn get_custom_op129(
            _input: input::CustomOp129Input,
        ) -> Result<output::CustomOp129Output, error::CustomOp129Error> {
        Ok(output::CustomOp129Output { output: "129".into() })
        }


        /// get_custom_op130
        pub async fn get_custom_op130(
            _input: input::CustomOp130Input,
        ) -> Result<output::CustomOp130Output, error::CustomOp130Error> {
        Ok(output::CustomOp130Output { output: "130".into() })
        }


        /// get_custom_op131
        pub async fn get_custom_op131(
            _input: input::CustomOp131Input,
        ) -> Result<output::CustomOp131Output, error::CustomOp131Error> {
        Ok(output::CustomOp131Output { output: "131".into() })
        }


        /// get_custom_op132
        pub async fn get_custom_op132(
            _input: input::CustomOp132Input,
        ) -> Result<output::CustomOp132Output, error::CustomOp132Error> {
        Ok(output::CustomOp132Output { output: "132".into() })
        }


        /// get_custom_op133
        pub async fn get_custom_op133(
            _input: input::CustomOp133Input,
        ) -> Result<output::CustomOp133Output, error::CustomOp133Error> {
        Ok(output::CustomOp133Output { output: "133".into() })
        }


        /// get_custom_op134
        pub async fn get_custom_op134(
            _input: input::CustomOp134Input,
        ) -> Result<output::CustomOp134Output, error::CustomOp134Error> {
        Ok(output::CustomOp134Output { output: "134".into() })
        }


        /// get_custom_op135
        pub async fn get_custom_op135(
            _input: input::CustomOp135Input,
        ) -> Result<output::CustomOp135Output, error::CustomOp135Error> {
        Ok(output::CustomOp135Output { output: "135".into() })
        }


        /// get_custom_op136
        pub async fn get_custom_op136(
            _input: input::CustomOp136Input,
        ) -> Result<output::CustomOp136Output, error::CustomOp136Error> {
        Ok(output::CustomOp136Output { output: "136".into() })
        }


        /// get_custom_op137
        pub async fn get_custom_op137(
            _input: input::CustomOp137Input,
        ) -> Result<output::CustomOp137Output, error::CustomOp137Error> {
        Ok(output::CustomOp137Output { output: "137".into() })
        }


        /// get_custom_op138
        pub async fn get_custom_op138(
            _input: input::CustomOp138Input,
        ) -> Result<output::CustomOp138Output, error::CustomOp138Error> {
        Ok(output::CustomOp138Output { output: "138".into() })
        }


        /// get_custom_op139
        pub async fn get_custom_op139(
            _input: input::CustomOp139Input,
        ) -> Result<output::CustomOp139Output, error::CustomOp139Error> {
        Ok(output::CustomOp139Output { output: "139".into() })
        }


        /// get_custom_op140
        pub async fn get_custom_op140(
            _input: input::CustomOp140Input,
        ) -> Result<output::CustomOp140Output, error::CustomOp140Error> {
        Ok(output::CustomOp140Output { output: "140".into() })
        }


        /// get_custom_op141
        pub async fn get_custom_op141(
            _input: input::CustomOp141Input,
        ) -> Result<output::CustomOp141Output, error::CustomOp141Error> {
        Ok(output::CustomOp141Output { output: "141".into() })
        }


        /// get_custom_op142
        pub async fn get_custom_op142(
            _input: input::CustomOp142Input,
        ) -> Result<output::CustomOp142Output, error::CustomOp142Error> {
        Ok(output::CustomOp142Output { output: "142".into() })
        }


        /// get_custom_op143
        pub async fn get_custom_op143(
            _input: input::CustomOp143Input,
        ) -> Result<output::CustomOp143Output, error::CustomOp143Error> {
        Ok(output::CustomOp143Output { output: "143".into() })
        }


        /// get_custom_op144
        pub async fn get_custom_op144(
            _input: input::CustomOp144Input,
        ) -> Result<output::CustomOp144Output, error::CustomOp144Error> {
        Ok(output::CustomOp144Output { output: "144".into() })
        }


        /// get_custom_op145
        pub async fn get_custom_op145(
            _input: input::CustomOp145Input,
        ) -> Result<output::CustomOp145Output, error::CustomOp145Error> {
        Ok(output::CustomOp145Output { output: "145".into() })
        }


        /// get_custom_op146
        pub async fn get_custom_op146(
            _input: input::CustomOp146Input,
        ) -> Result<output::CustomOp146Output, error::CustomOp146Error> {
        Ok(output::CustomOp146Output { output: "146".into() })
        }


        /// get_custom_op147
        pub async fn get_custom_op147(
            _input: input::CustomOp147Input,
        ) -> Result<output::CustomOp147Output, error::CustomOp147Error> {
        Ok(output::CustomOp147Output { output: "147".into() })
        }


        /// get_custom_op148
        pub async fn get_custom_op148(
            _input: input::CustomOp148Input,
        ) -> Result<output::CustomOp148Output, error::CustomOp148Error> {
        Ok(output::CustomOp148Output { output: "148".into() })
        }


        /// get_custom_op149
        pub async fn get_custom_op149(
            _input: input::CustomOp149Input,
        ) -> Result<output::CustomOp149Output, error::CustomOp149Error> {
        Ok(output::CustomOp149Output { output: "149".into() })
        }


        /// get_custom_op150
        pub async fn get_custom_op150(
            _input: input::CustomOp150Input,
        ) -> Result<output::CustomOp150Output, error::CustomOp150Error> {
        Ok(output::CustomOp150Output { output: "150".into() })
        }


        /// get_custom_op151
        pub async fn get_custom_op151(
            _input: input::CustomOp151Input,
        ) -> Result<output::CustomOp151Output, error::CustomOp151Error> {
        Ok(output::CustomOp151Output { output: "151".into() })
        }


        /// get_custom_op152
        pub async fn get_custom_op152(
            _input: input::CustomOp152Input,
        ) -> Result<output::CustomOp152Output, error::CustomOp152Error> {
        Ok(output::CustomOp152Output { output: "152".into() })
        }


        /// get_custom_op153
        pub async fn get_custom_op153(
            _input: input::CustomOp153Input,
        ) -> Result<output::CustomOp153Output, error::CustomOp153Error> {
        Ok(output::CustomOp153Output { output: "153".into() })
        }


        /// get_custom_op154
        pub async fn get_custom_op154(
            _input: input::CustomOp154Input,
        ) -> Result<output::CustomOp154Output, error::CustomOp154Error> {
        Ok(output::CustomOp154Output { output: "154".into() })
        }


        /// get_custom_op155
        pub async fn get_custom_op155(
            _input: input::CustomOp155Input,
        ) -> Result<output::CustomOp155Output, error::CustomOp155Error> {
        Ok(output::CustomOp155Output { output: "155".into() })
        }


        /// get_custom_op156
        pub async fn get_custom_op156(
            _input: input::CustomOp156Input,
        ) -> Result<output::CustomOp156Output, error::CustomOp156Error> {
        Ok(output::CustomOp156Output { output: "156".into() })
        }


        /// get_custom_op157
        pub async fn get_custom_op157(
            _input: input::CustomOp157Input,
        ) -> Result<output::CustomOp157Output, error::CustomOp157Error> {
        Ok(output::CustomOp157Output { output: "157".into() })
        }


        /// get_custom_op158
        pub async fn get_custom_op158(
            _input: input::CustomOp158Input,
        ) -> Result<output::CustomOp158Output, error::CustomOp158Error> {
        Ok(output::CustomOp158Output { output: "158".into() })
        }


        /// get_custom_op159
        pub async fn get_custom_op159(
            _input: input::CustomOp159Input,
        ) -> Result<output::CustomOp159Output, error::CustomOp159Error> {
        Ok(output::CustomOp159Output { output: "159".into() })
        }


        /// get_custom_op160
        pub async fn get_custom_op160(
            _input: input::CustomOp160Input,
        ) -> Result<output::CustomOp160Output, error::CustomOp160Error> {
        Ok(output::CustomOp160Output { output: "160".into() })
        }


        /// get_custom_op161
        pub async fn get_custom_op161(
            _input: input::CustomOp161Input,
        ) -> Result<output::CustomOp161Output, error::CustomOp161Error> {
        Ok(output::CustomOp161Output { output: "161".into() })
        }


        /// get_custom_op162
        pub async fn get_custom_op162(
            _input: input::CustomOp162Input,
        ) -> Result<output::CustomOp162Output, error::CustomOp162Error> {
        Ok(output::CustomOp162Output { output: "162".into() })
        }


        /// get_custom_op163
        pub async fn get_custom_op163(
            _input: input::CustomOp163Input,
        ) -> Result<output::CustomOp163Output, error::CustomOp163Error> {
        Ok(output::CustomOp163Output { output: "163".into() })
        }


        /// get_custom_op164
        pub async fn get_custom_op164(
            _input: input::CustomOp164Input,
        ) -> Result<output::CustomOp164Output, error::CustomOp164Error> {
        Ok(output::CustomOp164Output { output: "164".into() })
        }


        /// get_custom_op165
        pub async fn get_custom_op165(
            _input: input::CustomOp165Input,
        ) -> Result<output::CustomOp165Output, error::CustomOp165Error> {
        Ok(output::CustomOp165Output { output: "165".into() })
        }


        /// get_custom_op166
        pub async fn get_custom_op166(
            _input: input::CustomOp166Input,
        ) -> Result<output::CustomOp166Output, error::CustomOp166Error> {
        Ok(output::CustomOp166Output { output: "166".into() })
        }


        /// get_custom_op167
        pub async fn get_custom_op167(
            _input: input::CustomOp167Input,
        ) -> Result<output::CustomOp167Output, error::CustomOp167Error> {
        Ok(output::CustomOp167Output { output: "167".into() })
        }


        /// get_custom_op168
        pub async fn get_custom_op168(
            _input: input::CustomOp168Input,
        ) -> Result<output::CustomOp168Output, error::CustomOp168Error> {
        Ok(output::CustomOp168Output { output: "168".into() })
        }


        /// get_custom_op169
        pub async fn get_custom_op169(
            _input: input::CustomOp169Input,
        ) -> Result<output::CustomOp169Output, error::CustomOp169Error> {
        Ok(output::CustomOp169Output { output: "169".into() })
        }


        /// get_custom_op170
        pub async fn get_custom_op170(
            _input: input::CustomOp170Input,
        ) -> Result<output::CustomOp170Output, error::CustomOp170Error> {
        Ok(output::CustomOp170Output { output: "170".into() })
        }


        /// get_custom_op171
        pub async fn get_custom_op171(
            _input: input::CustomOp171Input,
        ) -> Result<output::CustomOp171Output, error::CustomOp171Error> {
        Ok(output::CustomOp171Output { output: "171".into() })
        }


        /// get_custom_op172
        pub async fn get_custom_op172(
            _input: input::CustomOp172Input,
        ) -> Result<output::CustomOp172Output, error::CustomOp172Error> {
        Ok(output::CustomOp172Output { output: "172".into() })
        }


        /// get_custom_op173
        pub async fn get_custom_op173(
            _input: input::CustomOp173Input,
        ) -> Result<output::CustomOp173Output, error::CustomOp173Error> {
        Ok(output::CustomOp173Output { output: "173".into() })
        }


        /// get_custom_op174
        pub async fn get_custom_op174(
            _input: input::CustomOp174Input,
        ) -> Result<output::CustomOp174Output, error::CustomOp174Error> {
        Ok(output::CustomOp174Output { output: "174".into() })
        }


        /// get_custom_op175
        pub async fn get_custom_op175(
            _input: input::CustomOp175Input,
        ) -> Result<output::CustomOp175Output, error::CustomOp175Error> {
        Ok(output::CustomOp175Output { output: "175".into() })
        }


        /// get_custom_op176
        pub async fn get_custom_op176(
            _input: input::CustomOp176Input,
        ) -> Result<output::CustomOp176Output, error::CustomOp176Error> {
        Ok(output::CustomOp176Output { output: "176".into() })
        }


        /// get_custom_op177
        pub async fn get_custom_op177(
            _input: input::CustomOp177Input,
        ) -> Result<output::CustomOp177Output, error::CustomOp177Error> {
        Ok(output::CustomOp177Output { output: "177".into() })
        }


        /// get_custom_op178
        pub async fn get_custom_op178(
            _input: input::CustomOp178Input,
        ) -> Result<output::CustomOp178Output, error::CustomOp178Error> {
        Ok(output::CustomOp178Output { output: "178".into() })
        }


        /// get_custom_op179
        pub async fn get_custom_op179(
            _input: input::CustomOp179Input,
        ) -> Result<output::CustomOp179Output, error::CustomOp179Error> {
        Ok(output::CustomOp179Output { output: "179".into() })
        }


        /// get_custom_op180
        pub async fn get_custom_op180(
            _input: input::CustomOp180Input,
        ) -> Result<output::CustomOp180Output, error::CustomOp180Error> {
        Ok(output::CustomOp180Output { output: "180".into() })
        }


        /// get_custom_op181
        pub async fn get_custom_op181(
            _input: input::CustomOp181Input,
        ) -> Result<output::CustomOp181Output, error::CustomOp181Error> {
        Ok(output::CustomOp181Output { output: "181".into() })
        }


        /// get_custom_op182
        pub async fn get_custom_op182(
            _input: input::CustomOp182Input,
        ) -> Result<output::CustomOp182Output, error::CustomOp182Error> {
        Ok(output::CustomOp182Output { output: "182".into() })
        }


        /// get_custom_op183
        pub async fn get_custom_op183(
            _input: input::CustomOp183Input,
        ) -> Result<output::CustomOp183Output, error::CustomOp183Error> {
        Ok(output::CustomOp183Output { output: "183".into() })
        }


        /// get_custom_op184
        pub async fn get_custom_op184(
            _input: input::CustomOp184Input,
        ) -> Result<output::CustomOp184Output, error::CustomOp184Error> {
        Ok(output::CustomOp184Output { output: "184".into() })
        }


        /// get_custom_op185
        pub async fn get_custom_op185(
            _input: input::CustomOp185Input,
        ) -> Result<output::CustomOp185Output, error::CustomOp185Error> {
        Ok(output::CustomOp185Output { output: "185".into() })
        }


        /// get_custom_op186
        pub async fn get_custom_op186(
            _input: input::CustomOp186Input,
        ) -> Result<output::CustomOp186Output, error::CustomOp186Error> {
        Ok(output::CustomOp186Output { output: "186".into() })
        }


        /// get_custom_op187
        pub async fn get_custom_op187(
            _input: input::CustomOp187Input,
        ) -> Result<output::CustomOp187Output, error::CustomOp187Error> {
        Ok(output::CustomOp187Output { output: "187".into() })
        }


        /// get_custom_op188
        pub async fn get_custom_op188(
            _input: input::CustomOp188Input,
        ) -> Result<output::CustomOp188Output, error::CustomOp188Error> {
        Ok(output::CustomOp188Output { output: "188".into() })
        }


        /// get_custom_op189
        pub async fn get_custom_op189(
            _input: input::CustomOp189Input,
        ) -> Result<output::CustomOp189Output, error::CustomOp189Error> {
        Ok(output::CustomOp189Output { output: "189".into() })
        }


        /// get_custom_op190
        pub async fn get_custom_op190(
            _input: input::CustomOp190Input,
        ) -> Result<output::CustomOp190Output, error::CustomOp190Error> {
        Ok(output::CustomOp190Output { output: "190".into() })
        }


        /// get_custom_op191
        pub async fn get_custom_op191(
            _input: input::CustomOp191Input,
        ) -> Result<output::CustomOp191Output, error::CustomOp191Error> {
        Ok(output::CustomOp191Output { output: "191".into() })
        }


        /// get_custom_op192
        pub async fn get_custom_op192(
            _input: input::CustomOp192Input,
        ) -> Result<output::CustomOp192Output, error::CustomOp192Error> {
        Ok(output::CustomOp192Output { output: "192".into() })
        }


        /// get_custom_op193
        pub async fn get_custom_op193(
            _input: input::CustomOp193Input,
        ) -> Result<output::CustomOp193Output, error::CustomOp193Error> {
        Ok(output::CustomOp193Output { output: "193".into() })
        }


        /// get_custom_op194
        pub async fn get_custom_op194(
            _input: input::CustomOp194Input,
        ) -> Result<output::CustomOp194Output, error::CustomOp194Error> {
        Ok(output::CustomOp194Output { output: "194".into() })
        }


        /// get_custom_op195
        pub async fn get_custom_op195(
            _input: input::CustomOp195Input,
        ) -> Result<output::CustomOp195Output, error::CustomOp195Error> {
        Ok(output::CustomOp195Output { output: "195".into() })
        }


        /// get_custom_op196
        pub async fn get_custom_op196(
            _input: input::CustomOp196Input,
        ) -> Result<output::CustomOp196Output, error::CustomOp196Error> {
        Ok(output::CustomOp196Output { output: "196".into() })
        }


        /// get_custom_op197
        pub async fn get_custom_op197(
            _input: input::CustomOp197Input,
        ) -> Result<output::CustomOp197Output, error::CustomOp197Error> {
        Ok(output::CustomOp197Output { output: "197".into() })
        }


        /// get_custom_op198
        pub async fn get_custom_op198(
            _input: input::CustomOp198Input,
        ) -> Result<output::CustomOp198Output, error::CustomOp198Error> {
        Ok(output::CustomOp198Output { output: "198".into() })
        }


        /// get_custom_op199
        pub async fn get_custom_op199(
            _input: input::CustomOp199Input,
        ) -> Result<output::CustomOp199Output, error::CustomOp199Error> {
        Ok(output::CustomOp199Output { output: "199".into() })
        }



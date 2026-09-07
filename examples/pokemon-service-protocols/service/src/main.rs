use async_stream::stream;
use std::{error::Error, net::SocketAddr};
use tokio::net::TcpListener;
use tracing_subscriber::{prelude::*, EnvFilter};

#[cfg(feature = "aws-json-10")]
use pokemon_service_protocols_aws_json_10_server_sdk as server_sdk;
#[cfg(feature = "aws-json-11")]
use pokemon_service_protocols_aws_json_11_server_sdk as server_sdk;
#[cfg(feature = "rest-json1")]
use pokemon_service_protocols_rest_json1_server_sdk as server_sdk;
#[cfg(feature = "rest-xml")]
use pokemon_service_protocols_rest_xml_server_sdk as server_sdk;
#[cfg(feature = "rpcv2-cbor")]
use pokemon_service_protocols_rpcv2_cbor_server_sdk as server_sdk;

const DEFAULT_ADDRESS: &str = "127.0.0.1:13734";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_tracing();

    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_string());
    let bind: SocketAddr = address.parse()?;

    let config = server_sdk::PokemonServiceConfig::builder().build();
    let builder = server_sdk::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .do_nothing(do_nothing)
        .capture_pokemon(capture_pokemon)
        .check_health(check_health);

    #[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
    let builder = builder
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .stream_pokemon_radio(stream_pokemon_radio);

    let app = builder
        .build()
        .expect("failed to build an instance of PokemonService");

    let listener = TcpListener::bind(bind).await?;
    let actual_addr = listener.local_addr()?;
    eprintln!("SERVER_READY:{}", actual_addr.port());

    server_sdk::serve(listener, app.into_make_service()).await?;
    Ok(())
}

fn setup_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("valid default tracing filter");
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();
}

async fn get_server_statistics(
    _input: server_sdk::input::GetServerStatisticsInput,
) -> server_sdk::output::GetServerStatisticsOutput {
    server_sdk::output::GetServerStatisticsOutput { calls_count: 1 }
}

async fn do_nothing(
    _input: server_sdk::input::DoNothingInput,
) -> server_sdk::output::DoNothingOutput {
    server_sdk::output::DoNothingOutput {}
}

async fn check_health(
    _input: server_sdk::input::CheckHealthInput,
) -> server_sdk::output::CheckHealthOutput {
    server_sdk::output::CheckHealthOutput {}
}

async fn capture_pokemon(
    input: server_sdk::input::CapturePokemonInput,
) -> Result<server_sdk::output::CapturePokemonOutput, server_sdk::error::CapturePokemonError> {
    let mut events = input.events;
    let output_stream = stream! {
        while let Ok(Some(event)) = events.recv().await {
            let Ok(capture) = event.as_event() else {
                continue;
            };
            let payload = capture.payload.as_ref();
            let name = payload
                .and_then(|payload| payload.name.clone())
                .unwrap_or_else(|| "Pikachu".to_string());
            let pokedex_update = server_sdk::types::Blob::new(vec![25, 1, 4]);

            yield Ok(server_sdk::model::CapturePokemonEvents::Event(
                server_sdk::model::CaptureEvent {
                    name: Some(name),
                    captured: Some(true),
                    shiny: Some(false),
                    pokedex_update: Some(pokedex_update),
                },
            ));
        }
    };

    Ok(server_sdk::output::CapturePokemonOutput::builder()
        .events(output_stream.into())
        .build()
        .expect("capture output is valid"))
}

#[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
async fn get_pokemon_species(
    input: server_sdk::input::GetPokemonSpeciesInput,
) -> Result<server_sdk::output::GetPokemonSpeciesOutput, server_sdk::error::GetPokemonSpeciesError>
{
    if input.name != "pikachu" {
        return Err(
            server_sdk::error::GetPokemonSpeciesError::ResourceNotFoundException(
                server_sdk::error::ResourceNotFoundException {
                    message: "unknown Pokemon species".to_string(),
                },
            ),
        );
    }

    Ok(server_sdk::output::GetPokemonSpeciesOutput {
        name: "pikachu".to_string(),
        flavor_text_entries: vec![server_sdk::model::FlavorText {
            flavor_text: "An electric mouse Pokemon.".to_string(),
            language: server_sdk::model::Language::English,
        }],
    })
}

#[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
async fn get_storage(
    input: server_sdk::input::GetStorageInput,
) -> Result<server_sdk::output::GetStorageOutput, server_sdk::error::GetStorageError> {
    if input.user != "ash" || input.passcode != "pikachu123" {
        return Err(
            server_sdk::error::GetStorageError::StorageAccessNotAuthorized(
                server_sdk::error::StorageAccessNotAuthorized {},
            ),
        );
    }

    Ok(server_sdk::output::GetStorageOutput {
        collection: vec![
            "bulbasaur".to_string(),
            "charmander".to_string(),
            "squirtle".to_string(),
            "pikachu".to_string(),
        ],
    })
}

#[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
async fn stream_pokemon_radio(
    _input: server_sdk::input::StreamPokemonRadioInput,
) -> server_sdk::output::StreamPokemonRadioOutput {
    server_sdk::output::StreamPokemonRadioOutput {
        data: server_sdk::types::ByteStream::from_static(b"pokemon radio\n"),
    }
}

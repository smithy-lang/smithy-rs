use async_stream::stream;
use std::error::Error;
use tokio::time::{timeout, Duration};
use tracing_subscriber::{prelude::*, EnvFilter};

#[cfg(feature = "aws-json-10")]
use pokemon_service_protocols_aws_json_10_client_sdk as client_sdk;
#[cfg(feature = "aws-json-11")]
use pokemon_service_protocols_aws_json_11_client_sdk as client_sdk;
#[cfg(feature = "rest-json1")]
use pokemon_service_protocols_rest_json1_client_sdk as client_sdk;
#[cfg(feature = "rest-xml")]
use pokemon_service_protocols_rest_xml_client_sdk as client_sdk;
#[cfg(feature = "rpcv2-cbor")]
use pokemon_service_protocols_rpcv2_cbor_client_sdk as client_sdk;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:13734";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_tracing();

    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let config = client_sdk::Config::builder()
        .endpoint_url(endpoint.clone())
        .build();
    let client = client_sdk::Client::from_conf(config);

    run_common_operations(&client).await?;

    #[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
    run_rest_operations(&client).await?;

    tracing::info!(%endpoint, "completed Pokemon protocol CLI run");
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

async fn run_common_operations(client: &client_sdk::Client) -> Result<(), Box<dyn Error>> {
    client.check_health().send().await?;
    tracing::info!("check_health succeeded");

    client.do_nothing().send().await?;
    tracing::info!("do_nothing succeeded");

    let statistics = client.get_server_statistics().send().await?;
    tracing::info!(
        calls_count = statistics.calls_count(),
        "get_server_statistics succeeded"
    );

    capture_pokemon(client).await?;
    Ok(())
}

async fn capture_pokemon(client: &client_sdk::Client) -> Result<(), Box<dyn Error>> {
    let input_stream = stream! {
        yield Ok::<_, client_sdk::types::error::AttemptCapturingPokemonEventError>(
            client_sdk::types::AttemptCapturingPokemonEvent::Event(
                client_sdk::types::CapturingEvent::builder()
                    .payload(
                        client_sdk::types::CapturingPayload::builder()
                            .name("Pikachu")
                            .pokeball("Master Ball")
                            .build()
                    )
                    .build()
            )
        );
    };

    let request = client.capture_pokemon().events(input_stream.into());

    #[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
    let request = request.region("Kanto");

    let mut output = request.send().await?;
    let mut received = 0usize;

    while received < 1 {
        let event = timeout(Duration::from_secs(5), output.events.recv())
            .await
            .map_err(|_| "timed out waiting for capture_pokemon event")?;
        match event {
            Ok(Some(event)) => {
                let capture = event
                    .as_event()
                    .map_err(|_| "unexpected capture_pokemon event variant")?;
                tracing::info!(
                    name = capture.name().unwrap_or("unknown"),
                    captured = capture.captured().unwrap_or(false),
                    "capture_pokemon received event"
                );
                received += 1;
            }
            Ok(None) => break,
            Err(err) => return Err(format!("capture_pokemon event stream failed: {err:?}").into()),
        }
    }

    Ok(())
}

#[cfg(any(feature = "rest-json1", feature = "rest-xml"))]
async fn run_rest_operations(client: &client_sdk::Client) -> Result<(), Box<dyn Error>> {
    let species = client.get_pokemon_species().name("pikachu").send().await?;
    tracing::info!(name = species.name(), "get_pokemon_species succeeded");

    let storage = client
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await?;
    tracing::info!(count = storage.collection().len(), "get_storage succeeded");

    let radio = client.stream_pokemon_radio().send().await?;
    let bytes = radio.data.collect().await?;
    tracing::info!(
        bytes = bytes.into_bytes().len(),
        "stream_pokemon_radio succeeded"
    );

    Ok(())
}

use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use async_stream::stream;
use sdk::{
    TickService, TickServiceConfig,
    error::{FizzBuzzError, ValidationException},
    input::FizzBuzzInput,
    model::{BuzzEvent, FizzEvent, ValueStream},
    output::FizzBuzzOutput,
    server::AddExtensionLayer,
};
use tracing_subscriber::{
    EnvFilter,
    fmt::{
        self,
        format::{FmtSpan, Format},
        time::SystemTime,
    },
    prelude::*,
};

async fn fizz_buzz_handler(mut input: FizzBuzzInput) -> Result<FizzBuzzOutput, FizzBuzzError> {
    let output_stream = stream! {
        let mut total = 0;
        let mut pending_buzz: VecDeque<i64> = VecDeque::new();

        while let Ok(Some(ValueStream::Value(input_value))) = input.stream.recv().await {
            let num = input_value.value;

            // Generate events as an iterator
            let events = generate_events(num, &mut pending_buzz);

            for (label, event) in events {
                println!("{}: {}", label, num);
                total += 1;

                yield Ok(event);
            }
        }

        println!("Finished. Sent {} events.", total);
    };

    FizzBuzzOutput::builder()
        .stream(output_stream.into())
        .build()
        .map_err(|e| {
            FizzBuzzError::ValidationException(
                ValidationException::builder()
                    .message(e.to_string())
                    .build()
                    .expect("should not fail"),
            )
        })
}

fn generate_events(
    num: i64,
    pending_buzz: &mut VecDeque<i64>,
) -> Vec<(&'static str, sdk::model::FizzBuzzStream)> {
    let mut events = Vec::new();

    match (num % 3 == 0, num % 5 == 0) {
        (true, true) => {
            pending_buzz.push_back(num);
            events.push((
                "Fizz (with following Buzz)",
                sdk::model::FizzBuzzStream::Fizz(FizzEvent::builder().value(num).build()),
            ));
        }
        (true, false) => {
            events.push((
                "Fizz",
                sdk::model::FizzBuzzStream::Fizz(FizzEvent::builder().value(num).build()),
            ));
        }
        (false, true) => {
            events.push((
                "Buzz",
                sdk::model::FizzBuzzStream::Buzz(BuzzEvent::builder().value(num).build()),
            ));
        }
        (false, false) => return events,
    }

    // Add pending buzz events
    while let Some(fizzbuzz_num) = pending_buzz.pop_front() {
        events.push((
            "Buzz for fizz",
            sdk::model::FizzBuzzStream::Buzz(BuzzEvent::builder().value(fizzbuzz_num).build()),
        ));
    }

    events
}

#[derive(Debug, Default)]
pub struct State {}

pub fn setup_tracing() {
    color_eyre::install().expect("cannot install color-eyre");

    let format = Format::default()
        .with_ansi(true)
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_source_location(false) // This is key - disables source location entirely
        .with_timer(SystemTime::default());

    // Use this formatter in the fmt layer
    let fmt_layer = fmt::layer()
        .event_format(format)
        .with_span_events(FmtSpan::CLOSE)
        .compact();

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}

#[tokio::main]
async fn main() {
    setup_tracing();

    let config = TickServiceConfig::builder()
        .layer(AddExtensionLayer::new(Arc::new(State::default())))
        .build();

    let app = TickService::builder(config)
        .fizz_buzz(fizz_buzz_handler)
        .build()
        .expect("failed to create service");

    let make_app = app.into_make_service();
    let bind: SocketAddr = "0.0.0.0:8000"
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind)
        //.http2_only(true)
        .serve(make_app);

    tracing::info!("CBOR Server is listening on {bind:?}");

    // Run forever-ish...
    if let Err(err) = server.await {
        tracing::error!("server error: {err}");
    }
}

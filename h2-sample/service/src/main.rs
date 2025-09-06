use std::{net::SocketAddr, sync::Arc};

use sdk::{
    SampleService, SampleServiceConfig,
    error::{SampleOperationError, ValidationException},
    input::SampleOperationInput,
    output::SampleOperationOutput,
    server::AddExtensionLayer,
};
use tracing_subscriber::{EnvFilter, prelude::*};

async fn sample_handler(
    _input: SampleOperationInput,
) -> Result<SampleOperationOutput, SampleOperationError> {
    SampleOperationOutput::builder()
        .result("this is a sample".to_owned())
        .build()
        .map_err(|e| {
            SampleOperationError::ValidationException(ValidationException {
                message: e.to_string(),
                field_list: None,
            })
        })
}

#[derive(Debug, Default)]
pub struct State {}

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

#[tokio::main]
async fn main() {
    setup_tracing();

    let config = SampleServiceConfig::builder()
        .layer(AddExtensionLayer::new(Arc::new(State::default())))
        .build();

    let app = SampleService::builder(config)
        .sample_operation(sample_handler)
        .build()
        .expect("failed to create service");

    let make_app = app.into_make_service();
    let bind: SocketAddr = "0.0.0.0:8000"
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind).http2_only(true).serve(make_app);

    tracing::info!(%bind, "Running server");

    if let Err(err) = server.await {
        eprintln!("server error: {err}");
    }
}

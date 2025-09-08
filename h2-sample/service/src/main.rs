use std::{net::SocketAddr, sync::Arc};

use hyper::{Body, Request, Response, StatusCode};
use sdk::{
    SampleService, SampleServiceConfig,
    error::{SampleOperationError, ValidationException},
    input::SampleOperationInput,
    output::SampleOperationOutput,
    server::{AddExtensionLayer, body::BoxBody, routing::IntoMakeServiceWithConnectInfo},
};
use tower::{ServiceExt, service_fn};
use tracing_subscriber::{fmt::{self, format::{FmtSpan, Format}, time::SystemTime}, prelude::*, EnvFilter};

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
        .with(fmt_layer)
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

    // Instead of this:

    // let health_check_layer =
    //     AlbHealthCheckLayer::from_handler("/ping", |_req| async { StatusCode::OK });
    // let health_check_service = health_check_layer.layer(app);

    // For the time being, you can do this:

    let health_check_service = service_fn(move |req: Request<Body>| {
        let app = app.clone();
        async move {
            let uri_path = req.uri().path();

            // If the above doesn't work, the following will work.
            // .path_and_query()
            // .map(|pq| pq.path())
            // .unwrap_or(req.uri().path());

            if uri_path == "/ping" {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(BoxBody::default())
                    .unwrap())
            } else {
                app.oneshot(req).await
            }
        }
    });

    let make_app = IntoMakeServiceWithConnectInfo::<_, SocketAddr>::new(health_check_service);

    let bind: SocketAddr = "0.0.0.0:8000"
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind).http2_only(false).serve(make_app);

    tracing::info!(%bind, "Running server");

    if let Err(err) = server.await {
        eprintln!("server error: {err}");
    }
}

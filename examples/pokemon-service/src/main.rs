/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod authz;
mod plugin;

use std::{net::SocketAddr, sync::Arc};

use aws_smithy_http_server_metrics::{
    plugin::DefaultMetricsPlugin, MetricsLayer, ReqBody, ResBody,
};
use clap::Parser;
use http::{Request, Response};
use metrique_writer::GlobalEntrySink;
use pokemon_service_server_sdk::server::{
    extension::OperationExtensionExt,
    instrumentation::InstrumentExt,
    layer::alb_health_check::AlbHealthCheckLayer,
    plugin::{HttpPlugins, ModelPlugins, Scoped},
    request::request_id::ServerRequestIdProviderLayer,
    routing::IntoMakeServiceWithConnectInfo,
    AddExtensionLayer,
};
use tokio::net::TcpListener;

use http::StatusCode;
use plugin::PrintExt;

use metrique::{
    emf::Emf,
    writer::{sink::AttachHandle, AttachGlobalEntrySinkExt, FormatExt},
    ServiceMetrics,
};
use pokemon_service::{
    do_nothing_but_log_request_ids, get_storage_with_local_approved, DEFAULT_ADDRESS, DEFAULT_PORT,
};
use pokemon_service_common::{
    capture_pokemon, check_health, get_pokemon_species, get_server_statistics,
    metrics::{PokemonMetrics, PokemonMetricsBuildExt},
    setup_tracing, stream_pokemon_radio, State,
};
use pokemon_service_server_sdk::{scope, PokemonService, PokemonServiceConfig};
use tower::Layer;

use crate::authz::AuthorizationPlugin;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = DEFAULT_ADDRESS)]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value_t = DEFAULT_PORT)]
    port: u16,
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();
    setup_tracing();

    let _metrics_handle = setup_metrics();

    scope! {
        /// A scope containing `GetPokemonSpecies` and `GetStorage`.
        struct PrintScope {
            includes: [GetPokemonSpecies, GetStorage]
        }
    }

    // Scope the `PrintPlugin`, defined in `plugin.rs`, to `PrintScope`.
    let print_plugin = Scoped::new::<PrintScope>(HttpPlugins::new().print());

    let http_plugins = HttpPlugins::new()
        .push(DefaultMetricsPlugin)
        // Apply the scoped `PrintPlugin`
        .push(print_plugin)
        // Apply the `OperationExtensionPlugin` defined in `aws_smithy_http_server::extension`. This allows other
        // plugins or tests to access a `aws_smithy_http_server::extension::OperationExtension` from
        // `Response::extensions`, or infer routing failure when it's missing.
        .insert_operation_extension()
        // Adds `tracing` spans and events to the request lifecycle.
        .instrument();

    let authz_plugin = AuthorizationPlugin::new();
    let model_plugins = ModelPlugins::new().push(authz_plugin);

    let config = PokemonServiceConfig::builder()
        // Set up shared state and middlewares.
        .layer(AddExtensionLayer::new(Arc::new(State::default())))
        // Handle `/ping` health check requests.
        .layer(AlbHealthCheckLayer::from_handler("/ping", |_req| async {
            StatusCode::OK
        }))
        // Add server request IDs.
        .layer(ServerRequestIdProviderLayer::new())
        .http_plugin(http_plugins)
        .model_plugin(model_plugins)
        .build();

    let app = PokemonService::builder(config)
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage_with_local_approved)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing_but_log_request_ids)
        .check_health(check_health)
        .stream_pokemon_radio(stream_pokemon_radio)
        .build()
        .expect("failed to build an instance of PokemonService");

    let metrics_layer = MetricsLayer::builder()
        .init_metrics(|| PokemonMetrics::default().append_on_drop(ServiceMetrics::sink()))
        .request_metrics(
            |_req: &mut Request<ReqBody>, metrics: &mut PokemonMetrics| {
                metrics.request_metrics.test_request_metric =
                    Some("test request metric".to_string());
            },
        )
        .response_metrics(
            |_res: &mut Response<ResBody>, metrics: &mut PokemonMetrics| {
                metrics.response_metrics.test_response_metric =
                    Some("test response metric".to_string());
            },
        )
        .disable_default_request_id_metric()
        .build();

    let service = metrics_layer.layer(app);

    // Using `IntoMakeServiceWithConnectInfo`, rather than `into_make_service`, to adjoin the `SocketAddr`
    // connection info.
    let make_app = IntoMakeServiceWithConnectInfo::<_, SocketAddr>::new(service);

    // Bind the application to a socket.
    let bind: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");
    let listener = TcpListener::bind(bind)
        .await
        .expect("failed to bind TCP listener");

    // Get the actual bound address (important when port 0 is used for random port)
    let actual_addr = listener.local_addr().expect("failed to get local address");

    // Signal that the server is ready to accept connections, including the actual port
    eprintln!("SERVER_READY:{}", actual_addr.port());

    // Run forever-ish...
    if let Err(err) = serve(listener, make_app).await {
        eprintln!("server error: {err}");
    }
}

pub(crate) fn setup_metrics() -> AttachHandle {
    let emf = Emf::builder("Ns".to_string(), vec![vec![]])
        .build()
        .output_to(std::io::stdout());

    ServiceMetrics::attach_to_stream(emf)
}

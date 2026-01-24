/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod authz;
mod outer_middleware;
mod plugin;

use std::{net::SocketAddr, sync::Arc};

use clap::Parser;
use pokemon_service_server_sdk::{
    serve,
    server::{
        extension::OperationExtensionExt,
        instrumentation::InstrumentExt,
        layer::alb_health_check::AlbHealthCheckLayer,
        plugin::{HttpPlugins, ModelPlugins, Scoped},
        request::request_id::ServerRequestIdProviderLayer,
        AddExtensionLayer,
    },
};
use tokio::net::TcpListener;

use http::StatusCode;
use plugin::PrintExt;

use pokemon_service::{
    do_nothing_but_log_request_ids, get_storage_with_local_approved, DEFAULT_ADDRESS, DEFAULT_PORT,
};
use pokemon_service_common::{
    capture_pokemon, check_health, get_pokemon_species, get_server_statistics, setup_tracing,
    stream_pokemon_radio, State,
};
use pokemon_service_server_sdk::{scope, PokemonService, PokemonServiceConfig};

use crate::authz::AuthorizationPlugin;
use crate::outer_middleware::OuterMiddlewareLayer;
use tower::Layer;

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

    scope! {
        /// A scope containing `GetPokemonSpecies` and `GetStorage`.
        struct PrintScope {
            includes: [GetPokemonSpecies, GetStorage]
        }
    }

    // Scope the `PrintPlugin`, defined in `plugin.rs`, to `PrintScope`.
    let print_plugin = Scoped::new::<PrintScope>(HttpPlugins::new().print());

    let http_plugins = HttpPlugins::new()
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

    // Position A: Apply outer middleware that wraps the ENTIRE service.
    // This middleware sees ALL requests, even those that fail routing.
    let outer_layer = OuterMiddlewareLayer::new();
    let app = outer_layer.layer(app);

    // Using `into_make_service_with_connect_info`, rather than `into_make_service`, to adjoin the `SocketAddr`
    // connection info.
    use pokemon_service_server_sdk::server::routing::IntoMakeService;
    let make_app = IntoMakeService::new(app);

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

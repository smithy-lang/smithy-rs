/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
pub static POKEMON_SERVICE_URL: &str = "http://localhost:13734";

/// Sets up the tracing subscriber to print `tracing::info!` and `tracing::error!` messages on the console.
pub fn setup_tracing_subscriber() {
    // Add a tracing subscriber that uses the environment variable RUST_LOG
    // to figure out which log level should be emitted. By default use `tracing::info!`
    // as the logging level.
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::fmt::fmt()
        .with_env_filter(filter)
        .init();
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example showing how to configure HTTP/2 keep-alive settings.
//!
//! This demonstrates HTTP/2's PING frame mechanism for detecting idle connections.
//! The server periodically sends PING frames and closes the connection if the
//! client doesn't respond within the timeout.
//!
//! Run with:
//! ```
//! cargo run --example http2_keepalive
//! ```
//!
//! Test with:
//! ```
//! # Force HTTP/2
//! curl --http2-prior-knowledge http://localhost:3000
//!
//! # Or with h2 if available
//! curl --http2 https://localhost:3000
//! ```

use aws_smithy_http_server::{routing::IntoMakeService, serve::serve};
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::service_fn;
use tracing::info;

async fn handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!\n"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Starting server with HTTP/2 keep-alive...");

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    let app = service_fn(handler);

    info!("Server listening on http://0.0.0.0:3000");
    info!("HTTP/2 keep-alive configuration:");
    info!("  - PING interval: 60 seconds");
    info!("  - PING timeout: 20 seconds");
    info!("");
    info!("The server will send a PING frame every 60 seconds.");
    info!("If the client doesn't respond within 20 seconds, the connection closes.");

    serve(listener, IntoMakeService::new(app))
        .configure_hyper(|mut builder| {
            builder
                .http2()
                .keep_alive_interval(Duration::from_secs(60))
                .keep_alive_timeout(Duration::from_secs(20));
            builder
        })
        .await?;

    Ok(())
}

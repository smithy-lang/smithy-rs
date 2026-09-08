/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Basic HTTP server example using `aws_smithy_http_server::serve()`.
//!
//! **This is the recommended way to run an HTTP server** for most use cases.
//! It provides a batteries-included experience with sensible defaults.
//!
//! This example demonstrates:
//! - Using the `serve()` function for connection handling
//! - Configuring the Hyper builder with `.configure_hyper()`
//! - Graceful shutdown with `.with_graceful_shutdown()`
//!
//! For more control (e.g., custom connection duration limits, connection limiting),
//! see the `custom_accept_loop` example.
//!
//! Run with:
//! ```
//! cargo run --example basic_server
//! ```
//!
//! Test with curl:
//! ```
//! curl http://localhost:3000/
//! curl -X POST -d "Hello!" http://localhost:3000/echo
//! ```

use aws_smithy_http_server::{routing::IntoMakeService, serve::serve};
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::service_fn;
use tracing::{info, warn};

/// Simple handler that responds immediately
async fn hello_handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!\n"))))
}

/// Handler that echoes the request body
async fn echo_handler(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let body = req.into_body();

    // Collect all body frames into bytes
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            warn!("echo handler: error reading body: {}", e);
            return Ok(Response::new(Full::new(Bytes::from("Error reading body\n"))));
        }
    };

    info!("echo handler: received {} bytes", bytes.len());

    // Echo back the body, or send a default message if empty
    if bytes.is_empty() {
        Ok(Response::new(Full::new(Bytes::from("No body provided\n"))))
    } else {
        Ok(Response::new(Full::new(bytes)))
    }
}

/// Router that dispatches to handlers based on path
async fn router(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/echo" => echo_handler(req).await,
        _ => hello_handler(req).await,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Starting server with aws_smithy_http_server::serve()...");

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    let local_addr = listener.local_addr()?;

    info!("Server listening on http://{}", local_addr);
    info!("Press Ctrl+C to shutdown gracefully");

    // Build the service
    let app = service_fn(router);

    // Use aws_smithy_http_server::serve with:
    // - Hyper configuration (HTTP/2 keep-alive settings)
    // - Graceful shutdown (wait for in-flight requests)
    serve(listener, IntoMakeService::new(app))
        .configure_hyper(|mut builder| {
            // Configure HTTP/2 keep-alive to detect stale connections
            builder
                .http2()
                .keep_alive_interval(Duration::from_secs(60))
                .keep_alive_timeout(Duration::from_secs(20));
            builder
        })
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
            info!("Received Ctrl+C, shutting down gracefully...");
        })
        .await?;

    Ok(())
}

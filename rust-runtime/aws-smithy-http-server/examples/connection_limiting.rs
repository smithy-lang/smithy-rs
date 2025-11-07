/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example showing how to limit concurrent connections.
//!
//! This example demonstrates using `limit_connections()` to cap the number
//! of simultaneous TCP connections the server will accept.
//!
//! Run with:
//! ```
//! cargo run --example connection_limiting
//! ```
//!
//! Test with:
//! ```
//! # Single request works fine
//! curl http://localhost:3000
//!
//! # Try overwhelming with many concurrent connections
//! oha -n 200 -c 200 http://localhost:3000
//! ```

use aws_smithy_http_server::{
    routing::IntoMakeService,
    serve::{serve, ListenerExt},
};
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::service_fn;
use tracing::info;

async fn handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // Simulate some work
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(Response::new(Full::new(Bytes::from("OK\n"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Starting server with connection limit...");

    // The listener limits concurrent connections to 100.
    // Once 100 connections are active, new connections will wait at the OS level
    // until an existing connection completes.
    let listener = TcpListener::bind("0.0.0.0:3000").await?.limit_connections(100);

    let app = service_fn(handler);

    info!("Server listening on http://0.0.0.0:3000");
    info!("Max concurrent connections: 100");
    info!("Try: oha -n 200 -c 200 http://localhost:3000");

    serve(listener, IntoMakeService::new(app)).await?;

    Ok(())
}

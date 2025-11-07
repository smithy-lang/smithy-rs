/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example showing how to add request-level timeouts.
//!
//! This demonstrates using Tower's `TimeoutLayer` to limit how long a single
//! request can take to complete. If the handler exceeds this duration, the
//! request is cancelled and an error response is returned.
//!
//! Run with:
//! ```
//! cargo run --example request_timeout
//! ```
//!
//! Test with:
//! ```
//! # Fast request completes normally
//! curl http://localhost:3000/
//!
//! # Slow request hits timeout
//! curl http://localhost:3000/slow
//! ```

use aws_smithy_http_server::{routing::IntoMakeService, serve::serve};
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::{service_fn, ServiceBuilder};
use tower_http::timeout::TimeoutLayer;
use tracing::info;

async fn handler(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/slow" => {
            info!("slow handler: sleeping for 45 seconds (will timeout)");
            tokio::time::sleep(Duration::from_secs(45)).await;
            Ok(Response::new(Full::new(Bytes::from("This won't be sent\n"))))
        }
        _ => Ok(Response::new(Full::new(Bytes::from("Hello, World!\n")))),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Starting server with request timeout...");

    let listener = TcpListener::bind("0.0.0.0:3000").await?;

    // Add 30 second timeout to all requests
    let app = ServiceBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .service(service_fn(handler));

    info!("Server listening on http://0.0.0.0:3000");
    info!("Request timeout: 30 seconds");
    info!("");
    info!("Try:");
    info!("  curl http://localhost:3000/       # Completes immediately");
    info!("  curl http://localhost:3000/slow   # Times out after 30s");

    serve(listener, IntoMakeService::new(app)).await?;

    Ok(())
}

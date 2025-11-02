/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example showing how to limit concurrent requests (not connections).
//!
//! This demonstrates Tower's `ConcurrencyLimitLayer`, which limits in-flight
//! requests rather than connections. This is different from connection limiting:
//! - One HTTP/2 connection can have multiple requests in flight
//! - One HTTP/1.1 keep-alive connection may be idle between requests
//!
//! Run with:
//! ```
//! cargo run --example request_concurrency_limiting
//! ```
//!
//! Test with:
//! ```
//! curl http://localhost:3000
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
use tower::limit::ConcurrencyLimitLayer;
use tower::{service_fn, ServiceBuilder};
use tracing::info;

async fn handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // Simulate some work
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(Response::new(Full::new(Bytes::from("OK\n"))))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Starting server with request concurrency limit...");

    // Limit connections at the TCP level
    let listener = TcpListener::bind("0.0.0.0:3000").await?.limit_connections(100);

    // Limit concurrent requests at the application level
    let app = ServiceBuilder::new()
        .layer(ConcurrencyLimitLayer::new(50))
        .service(service_fn(handler));

    info!("Server listening on http://0.0.0.0:3000");
    info!("Configuration:");
    info!("  - Max connections: 100 (TCP level)");
    info!("  - Max concurrent requests: 50 (application level)");
    info!("");
    info!("Note: With HTTP/2, a single connection can have multiple requests.");
    info!("Connection count and request count are independent metrics.");

    serve(listener, IntoMakeService::new(app)).await?;

    Ok(())
}

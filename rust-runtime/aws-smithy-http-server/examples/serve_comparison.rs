/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example comparing handle_connection vs handle_connection_strategy performance.
//!
//! This example starts a simple HTTP server that responds with "Hello, World!"
//! and can be load tested with tools like `oha`.
//!
//! Usage:
//!   # Run with strategy pattern implementation (uses handle_connection_strategy)
//!   cargo run --example serve_comparison --release
//!
//!   # Then in another terminal, run load test:
//!   oha -z 30s -c 100 http://127.0.0.1:3000/
//!
//! To test the original branching implementation, you would need to modify
//! the serve module to export handle_connection and use it here.

use std::convert::Infallible;
use std::net::SocketAddr;

use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use tower::service_fn;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    println!("Starting HTTP server on {}", addr);
    println!();
    println!("Load test with:");
    println!("  oha -z 30s -c 100 http://127.0.0.1:3000/");
    println!();
    println!("Press Ctrl+C to stop");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Create a simple service that responds with "Hello, World!"
    let service = service_fn(handle_request);

    // Use the public serve API which internally uses handle_connection_strategy
    aws_smithy_http_server::serve(listener, aws_smithy_http_server::routing::IntoMakeService::new(service)).await?;

    Ok(())
}

async fn handle_request(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

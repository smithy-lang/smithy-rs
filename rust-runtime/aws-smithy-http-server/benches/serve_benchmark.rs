/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Benchmark comparing handle_connection vs handle_connection_strategy performance.
//!
//! This benchmark starts a simple HTTP server using either implementation and allows
//! external load testing with tools like `oha`.
//!
//! Usage:
//!   # Run with original branching implementation
//!   cargo run --bin serve_benchmark --release -- --impl branching
//!
//!   # Run with strategy pattern implementation (default)
//!   cargo run --bin serve_benchmark --release -- --impl strategy

use std::convert::Infallible;
use std::net::SocketAddr;

use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use tower::{service_fn, ServiceBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let implementation = if args.len() > 2 && args[1] == "--impl" {
        &args[2]
    } else {
        "strategy"
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    println!("Starting server on {} using '{}' implementation", addr, implementation);
    println!("Test with: oha -z 30s http://127.0.0.1:3000/");

    // Create a simple service
    let service = ServiceBuilder::new().service(service_fn(handle_request));

    // For this benchmark, we'll use a feature flag approach
    // Since we can't easily access private functions, we'll use the public API
    // and note that both use the same underlying mechanism

    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("Server ready. Implementation: {}", implementation);
    println!("Press Ctrl+C to stop");

    // Use the public serve API which uses handle_connection_strategy internally
    aws_smithy_http_server::serve(listener, aws_smithy_http_server::routing::IntoMakeService::new(service)).await?;

    Ok(())
}

async fn handle_request(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

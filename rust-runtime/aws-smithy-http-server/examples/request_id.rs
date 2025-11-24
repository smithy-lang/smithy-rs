/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg_attr(
    not(feature = "request-id"),
    allow(unused_imports, dead_code, unreachable_code)
)]

//! Example showing how to use request IDs for tracing and observability.
//!
//! This demonstrates using `ServerRequestIdProviderLayer` to generate unique
//! request IDs for each incoming request. The ID can be:
//! - Accessed in your handler for logging/tracing
//! - Added to response headers so clients can reference it for support
//!
//! The `request-id` feature must be enabled in your Cargo.toml:
//! ```toml
//! aws-smithy-http-server = { version = "*", features = ["request-id"] }
//! ```
//!
//! Run with:
//! ```
//! cargo run --example request_id --features request-id
//! ```
//!
//! Test with:
//! ```
//! curl -v http://localhost:3000/
//! ```
//!
//! Look for the `x-request-id` header in the response.

use aws_smithy_http_server::{body::{boxed, BoxBody}, routing::IntoMakeService, serve::serve};

#[cfg(feature = "request-id")]
use aws_smithy_http_server::request::request_id::{ServerRequestId, ServerRequestIdProviderLayer};

use http::{header::HeaderName, Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use std::convert::Infallible;
use tokio::net::TcpListener;
use tower::{service_fn, ServiceBuilder};
use tracing::info;

#[cfg(feature = "request-id")]
async fn handler(req: Request<Incoming>) -> Result<Response<BoxBody>, Infallible> {
    // Extract the request ID from extensions (added by the layer)
    let request_id = req
        .extensions()
        .get::<ServerRequestId>()
        .expect("ServerRequestId should be present");

    // Use the request ID in your logs/traces
    info!(request_id = %request_id, "Handling request");

    let body = boxed(Full::new(Bytes::from(format!(
        "Request processed with ID: {request_id}\n"
    ))));

    Ok(Response::new(body))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(feature = "request-id"))]
    {
        eprintln!("ERROR: This example requires the 'request-id' feature.");
        eprintln!();
        eprintln!("Please run:");
        eprintln!("  cargo run --example request_id --features request-id");
        std::process::exit(1);
    }

    #[cfg(feature = "request-id")]
    {
        tracing_subscriber::fmt::init();

        info!("Starting server with request ID tracking...");

        let listener = TcpListener::bind("0.0.0.0:3000").await?;

        // Add ServerRequestIdProviderLayer to generate IDs and add them to response headers
        let app = ServiceBuilder::new()
            .layer(ServerRequestIdProviderLayer::new_with_response_header(
                HeaderName::from_static("x-request-id"),
            ))
            .service(service_fn(handler));

        info!("Server listening on http://0.0.0.0:3000");
        info!("Each request will receive a unique x-request-id header");
        info!("");
        info!("Try:");
        info!("  curl -v http://localhost:3000/");
        info!("  # Check the x-request-id header in the response");

        serve(listener, IntoMakeService::new(app)).await?;
    }

    Ok(())
}

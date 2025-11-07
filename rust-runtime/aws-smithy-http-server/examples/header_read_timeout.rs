/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example showing how to configure header read timeout.
//!
//! This demonstrates setting a timeout for reading HTTP request headers.
//! By default, Hyper allows 30 seconds for a client to send complete headers.
//! This example shows how to customize that duration.
//!
//! Run with:
//! ```
//! cargo run --example header_read_timeout
//! ```
//!
//! Test with:
//! ```
//! # Normal request works fine
//! curl http://localhost:3000
//!
//! # Simulate slow header sending (will timeout after 10s)
//! (echo -n "GET / HTTP/1.1\r\n"; sleep 15; echo "Host: localhost\r\n\r\n") | nc localhost 3000
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

    info!("Starting server with custom header read timeout...");

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    let app = service_fn(handler);

    info!("Server listening on http://0.0.0.0:3000");
    info!("Header read timeout: 10 seconds (default is 30s)");
    info!("");
    info!("The client must send complete HTTP headers within 10 seconds,");
    info!("otherwise the connection will be closed.");

    serve(listener, IntoMakeService::new(app))
        .configure_hyper(|mut builder| {
            builder.http1().header_read_timeout(Duration::from_secs(10));
            builder
        })
        .await?;

    Ok(())
}

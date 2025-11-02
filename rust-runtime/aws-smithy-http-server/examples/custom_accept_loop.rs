/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example demonstrating a custom accept loop with connection-level timeouts.
//!
//! This example shows how to implement your own accept loop instead of using
//! the built-in `serve()` function. This gives you control over:
//! - Overall connection duration limits
//! - Connection-level configuration
//! - Per-connection decision making
//!
//! Run with:
//! ```
//! cargo run --example custom_accept_loop
//! ```
//!
//! Test with curl:
//! ```
//! curl http://localhost:3000/
//! curl http://localhost:3000/slow
//! ```

use aws_smithy_http_server::{routing::IntoMakeService, serve::IncomingStream};
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Semaphore};
use tower::{service_fn, ServiceBuilder, ServiceExt};
use tower_http::timeout::TimeoutLayer;
use tracing::{info, warn};

/// Simple handler that responds immediately
async fn hello_handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!\n"))))
}

/// Handler that simulates a slow response
async fn slow_handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    info!("slow handler: sleeping for 45 seconds");
    tokio::time::sleep(Duration::from_secs(45)).await;
    Ok(Response::new(Full::new(Bytes::from("Completed\n"))))
}

/// Router that dispatches to handlers based on path
async fn router(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/slow" => slow_handler(req).await,
        _ => hello_handler(req).await,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    let local_addr = listener.local_addr()?;

    info!("Server listening on http://{}", local_addr);
    info!("Configuration:");
    info!("  - Header read timeout: 10 seconds");
    info!("  - Request timeout: 30 seconds");
    info!("  - Connection duration limit: 5 minutes");
    info!("  - Max concurrent connections: 1000");
    info!("  - HTTP/2 keep-alive: 60s interval, 20s timeout");

    // Connection limiting with semaphore
    let connection_semaphore = Arc::new(Semaphore::new(1000));

    // Build the service with request timeout layer
    let base_service = ServiceBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .service(service_fn(router));

    let make_service = IntoMakeService::new(base_service);

    loop {
        // Accept new connection
        let (stream, remote_addr) = listener.accept().await?;

        // Try to acquire connection permit
        let permit = match connection_semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                warn!("connection limit reached, rejecting connection from {}", remote_addr);
                drop(stream);
                continue;
            }
        };

        info!("accepted connection from {}", remote_addr);

        let make_service = make_service.clone();

        tokio::spawn(async move {
            // The permit will be dropped when this task ends, freeing up a connection slot
            let _permit = permit;

            let io = TokioIo::new(stream);

            // Create service for this connection
            let tower_service =
                match ServiceExt::oneshot(make_service, IncomingStream::<TcpListener> { io: &io, remote_addr }).await {
                    Ok(svc) => svc,
                    Err(_) => {
                        warn!("failed to create service for connection from {}", remote_addr);
                        return;
                    }
                };

            let hyper_service = TowerToHyperService::new(tower_service);

            // Configure Hyper builder with timeouts
            let mut builder = Builder::new(TokioExecutor::new());
            builder
                .http1()
                .header_read_timeout(Duration::from_secs(10))
                .keep_alive(true);
            builder
                .http2()
                .keep_alive_interval(Duration::from_secs(60))
                .keep_alive_timeout(Duration::from_secs(20));

            // Serve the connection with overall duration timeout
            let conn = builder.serve_connection(io, hyper_service);

            // Wrap the entire connection in a timeout.
            // The connection will be closed after 5 minutes regardless of activity.
            match tokio::time::timeout(Duration::from_secs(300), conn).await {
                Ok(Ok(())) => {
                    info!("connection from {} closed normally", remote_addr);
                }
                Ok(Err(e)) => {
                    warn!("error serving connection from {}: {:?}", remote_addr, e);
                }
                Err(_) => {
                    info!("connection from {} exceeded 5 minute duration limit", remote_addr);
                }
            }
        });
    }
}

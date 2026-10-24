/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::net::Ipv4Addr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut args = std::env::args().skip(1);
    let port = match args.next().as_deref() {
        None => 13734,
        Some("--port") => args.next().ok_or("missing port")?.parse::<u16>()?,
        Some("--help" | "-h") => {
            println!("Usage: request-limits-server [--port PORT]\nListens on 127.0.0.1; use port 0 for an available port.");
            return Ok(());
        }
        Some(_) => return Err("Usage: request-limits-server [--port PORT]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    println!("SERVER_READY:{}", listener.local_addr()?.port());
    request_limits::serve(listener, async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    })
    .await?;
    Ok(())
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configurable benchmark server for testing different serve configurations.
//!
//! This example allows testing different HTTP server configurations via command-line
//! arguments, making it easy to benchmark different scenarios systematically.
//!
//! ## Usage Examples:
//!
//! ```bash
//! # Default: auto HTTP version, no graceful shutdown, port 3000
//! cargo run --example serve_benchmark --release
//!
//! # HTTP/1 only
//! cargo run --example serve_benchmark --release -- --http-version http1
//!
//! # HTTP/2 only with TLS (required for HTTP/2)
//! cargo run --example serve_benchmark --release -- --http-version http2 --tls
//!
//! # With graceful shutdown
//! cargo run --example serve_benchmark --release -- --graceful-shutdown
//!
//! # HTTP/1 only with graceful shutdown on port 8080
//! cargo run --example serve_benchmark --release -- --http-version http1 --graceful-shutdown --port 8080
//! ```
//!
//! ## Load Testing:
//!
//! ```bash
//! # In another terminal (for HTTP)
//! oha -z 30s -c 100 http://127.0.0.1:3000/
//!
//! # For HTTPS (with self-signed certs)
//! oha -z 30s -c 100 --insecure https://127.0.0.1:3443/
//! ```

use std::convert::Infallible;
use std::fs::File;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;
use tower::service_fn;

#[derive(Parser, Debug)]
#[command(name = "serve_benchmark")]
#[command(about = "HTTP benchmark server with configurable settings", long_about = None)]
struct Args {
    /// HTTP version: auto, http1, or http2
    #[arg(long, default_value = "auto", value_parser = ["auto", "http1", "http2"])]
    http_version: String,

    /// Enable graceful shutdown
    #[arg(long, default_value_t = false)]
    graceful_shutdown: bool,

    /// Server port (default: 3000 for HTTP, 3443 for HTTPS)
    #[arg(long, short)]
    port: Option<u16>,

    /// Enable TLS (required for HTTP/2)
    #[arg(long, default_value_t = false)]
    tls: bool,
}

/// TLS Listener implementation
pub struct TlsListener {
    tcp_listener: TcpListener,
    tls_acceptor: TlsAcceptor,
}

impl TlsListener {
    pub fn new(tcp_listener: TcpListener, tls_acceptor: TlsAcceptor) -> Self {
        Self {
            tcp_listener,
            tls_acceptor,
        }
    }
}

impl aws_smithy_http_server::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.tcp_listener.accept().await {
                Ok((tcp_stream, remote_addr)) => match self.tls_acceptor.accept(tcp_stream).await {
                    Ok(tls_stream) => return (tls_stream, remote_addr),
                    Err(err) => {
                        eprintln!("TLS handshake failed: {err}");
                        continue;
                    }
                },
                Err(err) => {
                    eprintln!("Failed to accept TCP connection: {err}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp_listener.local_addr()
    }
}

fn load_certs(path: &str) -> Vec<CertificateDer<'static>> {
    let mut reader = BufReader::new(File::open(path).expect("could not open certificate"));
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("could not parse certificate")
}

fn load_key(path: &str) -> PrivateKeyDer<'static> {
    let mut reader = BufReader::new(File::open(path).expect("could not open private key"));
    loop {
        match rustls_pemfile::read_one(&mut reader).expect("could not parse private key") {
            Some(rustls_pemfile::Item::Pkcs1Key(key)) => return key.into(),
            Some(rustls_pemfile::Item::Pkcs8Key(key)) => return key.into(),
            Some(rustls_pemfile::Item::Sec1Key(key)) => return key.into(),
            None => break,
            _ => {}
        }
    }
    panic!("invalid private key")
}

fn create_tls_acceptor(http_version: &str) -> TlsAcceptor {
    let cert_path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/testdata/localhost.crt");
    let key_path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/testdata/localhost.key");

    let certs = load_certs(cert_path);
    let key = load_key(key_path);

    let mut server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("could not create server config");

    // Configure ALPN based on http_version
    server_config.alpn_protocols = match http_version {
        "http1" => vec!["http/1.1".into()],
        "http2" => vec!["h2".into()],
        "auto" => vec!["h2".into(), "http/1.1".into()],
        _ => vec!["h2".into(), "http/1.1".into()],
    };

    TlsAcceptor::from(Arc::new(server_config))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Default ports: 3000 for HTTP, 3443 for HTTPS
    let default_port = if args.tls { 3443 } else { 3000 };
    let port = args.port.unwrap_or(default_port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let protocol = if args.tls { "https" } else { "http" };

    println!("╔════════════════════════════════════════╗");
    println!("║    HTTP Benchmark Server               ║");
    println!("╚════════════════════════════════════════╝");
    println!();
    println!("Configuration:");
    println!("  Address:            {}", addr);
    println!("  Protocol:           {}", if args.tls { "HTTPS" } else { "HTTP" });
    println!("  HTTP Version:       {}", args.http_version);
    println!("  Graceful Shutdown:  {}", args.graceful_shutdown);
    println!();
    println!("Load test with:");
    if args.tls {
        println!("  oha -z 30s -c 100 --insecure {}://127.0.0.1:{}/", protocol, port);
    } else {
        println!("  oha -z 30s -c 100 {}://127.0.0.1:{}/", protocol, port);
    }
    println!();
    println!("Press Ctrl+C to stop");
    println!();

    // Create a simple service that responds with "Hello, World!"
    let service = service_fn(handle_request);

    if args.tls {
        // TLS path
        let tcp_listener = TcpListener::bind(addr).await?;
        let tls_acceptor = create_tls_acceptor(&args.http_version);
        let tls_listener = TlsListener::new(tcp_listener, tls_acceptor);

        let server = aws_smithy_http_server::serve(
            tls_listener,
            aws_smithy_http_server::routing::IntoMakeService::new(service),
        );

        // Configure HTTP version if specified
        let server = match args.http_version.as_str() {
            "http1" => server.configure_hyper(|builder| builder.http1_only()),
            "http2" => server.configure_hyper(|builder| builder.http2_only()),
            "auto" | _ => server,
        };

        if args.graceful_shutdown {
            server
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c()
                        .await
                        .expect("failed to listen for Ctrl+C");
                    println!("\nShutting down gracefully...");
                })
                .await?;
        } else {
            server.await?;
        }
    } else {
        // Plain HTTP path
        let listener = TcpListener::bind(addr).await?;

        let server = aws_smithy_http_server::serve(
            listener,
            aws_smithy_http_server::routing::IntoMakeService::new(service),
        );

        // Configure HTTP version if specified
        let server = match args.http_version.as_str() {
            "http1" => server.configure_hyper(|builder| builder.http1_only()),
            "http2" => server.configure_hyper(|builder| builder.http2_only()),
            "auto" | _ => server,
        };

        if args.graceful_shutdown {
            server
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c()
                        .await
                        .expect("failed to listen for Ctrl+C");
                    println!("\nShutting down gracefully...");
                })
                .await?;
        } else {
            server.await?;
        }
    }

    Ok(())
}

async fn handle_request(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

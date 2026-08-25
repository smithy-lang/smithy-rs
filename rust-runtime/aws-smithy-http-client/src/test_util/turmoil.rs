/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Turmoil-backed test transport.
//!
//! [`turmoil_client`] builds a [`SharedHttpClient`] that connects over the
//! [`turmoil`] discrete-event network simulator, so tests can drive the *real*
//! HTTP client (connection pool, connect/read timeouts, error classification)
//! over a simulated network.
//!
//! Construct it from a [`SharedDnsResolver`] and a port. The helper deliberately
//! hides every hyper IO detail: consumers never touch
//! [`hyper_util::rt::TokioIo`], `hyper::rt::{Read, Write}`, or the hyper
//! `Connection`/`Connect` traits. All of that plumbing is crate-private
//! (`TurmoilTcpConnector`/`TurmoilConnection`).
//!
//! Enable exactly one of the mutually exclusive `turmoil-06` / `turmoil-07`
//! features to select which major version of the `turmoil` crate is linked; the
//! transport code below is identical across both.
//!
//! The transport is **plaintext**: no TLS is layered on top.

use std::future::Future;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use aws_smithy_runtime_api::client::dns::{ResolveDns, SharedDnsResolver};
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use http_1x::Uri;
use hyper::rt::{Read, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::TokioIo;

// The `turmoil-06` and `turmoil-07` features select which major version of the
// `turmoil` crate is linked. Their public APIs are identical for the surface we
// use, so alias the enabled one to a single `turmoil` path and keep the rest of
// this module version-agnostic.
#[cfg(all(feature = "turmoil-06", not(feature = "turmoil-07")))]
use turmoil_0_6 as turmoil;
#[cfg(feature = "turmoil-07")]
use turmoil_0_7 as turmoil;

use turmoil::net::TcpStream;

use crate::client::build_with_tcp_conn_fn;

/// Build a [`SharedHttpClient`] that drives requests over the [`turmoil`]
/// discrete-event network simulator.
///
/// Resolves target hosts with `resolver` and connects on `port`. The client runs
/// through the same connection pool, connect/read timeout, and error-classification
/// stack as the production client — only the transport is replaced. No hyper types
/// appear in this API; all `tower::Service`/`Connection` plumbing is crate-private.
///
/// The transport is **plaintext**: no TLS is layered on top.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "__turmoil")]
/// # {
/// use aws_smithy_http_client::test_util::turmoil_client;
/// use aws_smithy_runtime_api::client::dns::SharedDnsResolver;
///
/// # fn resolver() -> SharedDnsResolver { unimplemented!() }
/// let http_client = turmoil_client(resolver(), 8891);
/// # }
/// ```
pub fn turmoil_client(resolver: impl Into<SharedDnsResolver>, port: u16) -> SharedHttpClient {
    let connector = TurmoilTcpConnector {
        resolver: resolver.into(),
        port,
    };
    build_with_tcp_conn_fn(None, None, None, move || connector.clone())
}

/// Crate-private `tower::Service<Uri>` that performs the turmoil connect. Kept
/// out of the public API so the hyper IO bounds it satisfies never leak.
#[derive(Clone)]
struct TurmoilTcpConnector {
    resolver: SharedDnsResolver,
    port: u16,
}

impl tower::Service<Uri> for TurmoilTcpConnector {
    type Response = TurmoilConnection;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let resolver = self.resolver.clone();
        let port = self.port;
        Box::pin(async move {
            let host = match dst.host() {
                Some(host) => host.to_string(),
                None => {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        format!("URI has no host: {dst}"),
                    ))
                }
            };
            // Test transport: connect to the first resolved address only. Turmoil
            // hosts are single-node, so there is deliberately no happy-eyeballs /
            // multi-address fallback here (that lives in the production connector).
            let ip_addr = resolver
                .resolve_dns(&host)
                .await
                .map_err(|e| Error::new(ErrorKind::NotFound, e))?
                .into_iter()
                .next();
            match ip_addr {
                Some(ip_addr) => {
                    let stream = TcpStream::connect(SocketAddr::new(ip_addr, port)).await?;
                    Ok(TurmoilConnection(TokioIo::new(stream)))
                }
                None => Err(Error::new(
                    ErrorKind::NotFound,
                    format!("no IP address resolved for {host}"),
                )),
            }
        })
    }
}

/// Crate-private turmoil TCP connection that satisfies the smithy HTTP client's
/// IO bounds.
///
/// Wraps a [`turmoil::net::TcpStream`] (via [`TokioIo`]) and implements hyper's
/// [`Read`]/[`Write`] plus [`Connection`] — `turmoil::net::TcpStream` does not
/// implement hyper-util's `Connection`, so this newtype supplies it. Not part of
/// the public API.
#[derive(Debug)]
struct TurmoilConnection(TokioIo<TcpStream>);

impl Connection for TurmoilConnection {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl Read for TurmoilConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl Write for TurmoilConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

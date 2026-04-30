/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tower service adapters for hyper's HTTP protocol handshake.
//!
//! - `ConnectionLimit`: wraps a connector, acquires semaphore permits before
//!   connecting, and returns `(IO, Arc<ConnectionPermit>)`.
//! - `H1ConnectAndHandshake` / `H2ConnectAndHandshake`: perform the protocol
//!   handshake on an already-connected IO stream, producing a `ManagedConnection`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use hyper_util::rt::TokioExecutor;
use tokio::sync::Semaphore;
use tower::Service;

use super::connection::{ConnectionInfo, ConnectionPermit, ManagedConnection};
use super::BoxError;

/// Wraps a connector service, acquiring semaphore permits before connecting.
///
/// Returns `(IO, Arc<ConnectionPermit>)` so the permit can be stored on
/// `ManagedConnection` and held for the connection's lifetime.
pub(crate) struct ConnectionLimit<C> {
    inner: C,
    global: Option<Arc<Semaphore>>,
    per_host: Option<Arc<Semaphore>>,
}

impl<C> ConnectionLimit<C> {
    pub(crate) fn new(
        inner: C,
        global: Option<Arc<Semaphore>>,
        per_host: Option<Arc<Semaphore>>,
    ) -> Self {
        Self {
            inner,
            global,
            per_host,
        }
    }
}

impl<C: Clone> Clone for ConnectionLimit<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            global: self.global.clone(),
            per_host: self.per_host.clone(),
        }
    }
}

impl<C, IO, Req> Service<Req> for ConnectionLimit<C>
where
    C: Service<Req, Response = IO> + Clone + Send + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Send + 'static,
    IO: Send + 'static,
    Req: Send + 'static,
{
    type Response = (IO, Arc<ConnectionPermit>);
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let mut inner = self.inner.clone();
        let global = self.global.clone();
        let per_host = self.per_host.clone();
        Box::pin(async move {
            // Acquire per-host permit first, then global. This ordering
            // prevents deadlock: a request never holds a global permit
            // while waiting on a per-host permit (which could starve
            // other hosts' requests that need global permits).
            let per_host_permit = match per_host {
                Some(sem) => Some(sem.acquire_owned().await.map_err(|_| "pool closed")?),
                None => None,
            };
            let global_permit = match global {
                Some(sem) => Some(sem.acquire_owned().await.map_err(|_| "pool closed")?),
                None => None,
            };
            let permit = Arc::new(ConnectionPermit::new(global_permit, per_host_permit));

            std::future::poll_fn(|cx| inner.poll_ready(cx))
                .await
                .map_err(Into::into)?;
            let io = inner.call(req).await.map_err(Into::into)?;
            Ok((io, permit))
        })
    }
}

// ---------------------------------------------------------------------------
// Tower Service wrappers for hyper's SendRequest
// ---------------------------------------------------------------------------

/// Tower `Service` adapter for `hyper::client::conn::http1::SendRequest`.
pub(crate) struct H1SendRequest {
    inner: hyper::client::conn::http1::SendRequest<SdkBody>,
}

impl H1SendRequest {
    pub(crate) fn new(inner: hyper::client::conn::http1::SendRequest<SdkBody>) -> Self {
        Self { inner }
    }
}

impl Service<http_1x::Request<SdkBody>> for H1SendRequest {
    type Response = http_1x::Response<hyper::body::Incoming>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        Box::pin(self.inner.send_request(req))
    }
}

/// Tower `Service` adapter for `hyper::client::conn::http2::SendRequest`.
///
/// Clone is supported — HTTP/2 multiplexes requests over a single connection.
#[derive(Clone)]
pub(crate) struct H2SendRequest {
    inner: hyper::client::conn::http2::SendRequest<SdkBody>,
}

impl H2SendRequest {
    pub(crate) fn new(inner: hyper::client::conn::http2::SendRequest<SdkBody>) -> Self {
        Self { inner }
    }
}

impl Service<http_1x::Request<SdkBody>> for H2SendRequest {
    type Response = http_1x::Response<hyper::body::Incoming>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        Box::pin(self.inner.send_request(req))
    }
}

// ---------------------------------------------------------------------------
// Connect-and-handshake services
// ---------------------------------------------------------------------------

/// Extract `ConnectionInfo` from a just-connected IO stream.
fn capture_info<IO: Connection>(io: &IO) -> ConnectionInfo {
    let connected = io.connected();
    let mut extras = http_1x::Extensions::new();
    connected.get_extras(&mut extras);
    let http_info = extras.get::<HttpInfo>();
    ConnectionInfo {
        remote_addr: http_info.map(|i| i.remote_addr()),
        local_addr: http_info.map(|i| i.local_addr()),
    }
}

/// Connects and performs an HTTP/1.1 handshake.
///
/// The connector is expected to return `(IO, Arc<ConnectionPermit>)` — typically
/// produced by [`ConnectionLimit`] wrapping a TCP/TLS connector.
pub(crate) struct H1ConnectAndHandshake<C> {
    connector: C,
}

impl<C> H1ConnectAndHandshake<C> {
    pub(crate) fn new(connector: C) -> Self {
        Self { connector }
    }
}

impl<C: Clone> Clone for H1ConnectAndHandshake<C> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
        }
    }
}

impl<C, IO> Service<http_1x::Uri> for H1ConnectAndHandshake<C>
where
    C: Service<http_1x::Uri, Response = (IO, Arc<ConnectionPermit>)>,
    C::Error: Into<BoxError> + 'static,
    C::Future: Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + Connection + Unpin + Send + 'static,
{
    type Response = ManagedConnection<H1SendRequest>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connector.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, uri: http_1x::Uri) -> Self::Future {
        let fut = self.connector.call(uri);
        Box::pin(async move {
            let (io, permit) = fut.await.map_err(Into::into)?;
            let info = capture_info(&io);

            let (tx, conn) = hyper::client::conn::http1::Builder::new()
                .handshake(io)
                .await
                .map_err(|e| Box::new(e) as BoxError)?;

            tokio::spawn({
                let remote_addr = info.remote_addr;
                let local_addr = info.local_addr;
                async move {
                    if let Err(e) = conn.with_upgrades().await {
                        tracing::debug!(
                            protocol = "h1",
                            ?remote_addr,
                            ?local_addr,
                            error = %e,
                            "connection driver error"
                        );
                    }
                }
            });

            Ok(ManagedConnection::new(H1SendRequest::new(tx), info, permit))
        })
    }
}

/// Connects and performs an HTTP/2 handshake.
///
/// Pinned to `Service<()>` because this service sits in the Negotiate
/// upgrade path — the connection is already established.
pub(crate) struct H2ConnectAndHandshake<C> {
    connector: C,
}

impl<C> H2ConnectAndHandshake<C> {
    pub(crate) fn new(connector: C) -> Self {
        Self { connector }
    }
}

impl<C: Clone> Clone for H2ConnectAndHandshake<C> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
        }
    }
}

impl<C, IO> Service<()> for H2ConnectAndHandshake<C>
where
    C: Service<(), Response = (IO, Arc<ConnectionPermit>)>,
    C::Error: Into<BoxError> + 'static,
    C::Future: Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + Connection + Unpin + Send + 'static,
{
    type Response = ManagedConnection<H2SendRequest>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connector.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let fut = self.connector.call(());
        Box::pin(async move {
            let (io, permit) = fut.await.map_err(Into::into)?;
            let info = capture_info(&io);

            let (tx, conn) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
                .map_err(|e| Box::new(e) as BoxError)?;

            tokio::spawn({
                let remote_addr = info.remote_addr;
                let local_addr = info.local_addr;
                async move {
                    if let Err(e) = conn.await {
                        tracing::debug!(
                            protocol = "h2",
                            ?remote_addr,
                            ?local_addr,
                            error = %e,
                            "connection driver error"
                        );
                    }
                }
            });

            Ok(ManagedConnection::new(H2SendRequest::new(tx), info, permit))
        })
    }
}

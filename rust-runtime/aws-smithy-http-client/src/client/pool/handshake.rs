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
use hyper::rt::Executor;
use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use hyper_util::rt::TokioExecutor;
use tokio::sync::Semaphore;
use tower::Service;

use super::connection::{ConnectCtx, ConnectionInfo, ConnectionPermit, ManagedConnection};
use super::BoxError;

/// Wraps a connector service, acquiring semaphore permits before connecting.
///
/// Returns `(IO, Arc<ConnectionPermit>)` so the permit can be stored on
/// `ManagedConnection` and held for the connection's lifetime.
///
/// Target type is `ConnectCtx`: the inner TCP connector is `Service<Uri>`,
/// so this layer extracts `ctx.uri` to pass through. Per-operation timeouts
/// on the context are applied here — `connect_timeout` wraps the inner
/// connector call (which includes TCP + TLS since the inner is the
/// TLS-wrapped connector). Cache hits skip this layer entirely, so
/// `connect_timeout` is automatically new-connection-only.
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

impl<C, IO> Service<ConnectCtx> for ConnectionLimit<C>
where
    C: Service<http_1x::Uri, Response = IO> + Clone + Send + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Send + 'static,
    IO: Send + 'static,
{
    type Response = (IO, Arc<ConnectionPermit>);
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
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

            // Apply connect_timeout only around the actual connector call
            // (TCP + TLS). If `ctx.connect_timeout` is `None`, this is a
            // plain `inner.call(uri).await`.
            let uri = ctx.uri;
            let connect_fut = inner.call(uri);
            let io = super::super::timeout::maybe_timeout_future(
                connect_fut,
                ctx.connect_timeout.as_ref().map(|t| t.duration),
                ctx.connect_timeout.as_ref().map(|t| &t.sleep_impl),
                super::super::timeout::TimeoutKind::Connect,
            )
            .await?;
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

/// Spawn a connection driver future onto the runtime.
///
// TODO - revisit tokio default without flags
fn spawn_driver(future: impl Future<Output = ()> + Send + 'static) {
    TokioExecutor::new().execute(future);
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

impl<C, IO> Service<ConnectCtx> for H1ConnectAndHandshake<C>
where
    C: Service<ConnectCtx, Response = (IO, Arc<ConnectionPermit>)>,
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

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        let fut = self.connector.call(ctx);
        Box::pin(async move {
            let (io, permit) = fut.await.map_err(Into::into)?;
            let info = capture_info(&io);

            let (tx, conn) = hyper::client::conn::http1::Builder::new()
                .handshake(io)
                .await
                .map_err(|e| Box::new(e) as BoxError)?;

            spawn_driver({
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
/// upgrade path — the connection is already established via the shared
/// `Inspected` slot.
pub(crate) struct H2ConnectAndHandshake<C> {
    connector: C,
    h2_ref: super::connection::H2ConnectionRef,
}

impl<C> H2ConnectAndHandshake<C> {
    /// Create an H2 handshake service that publishes every newly established
    /// connection's `ConnectionMetadata` into `h2_ref`. The same ref is held
    /// on the read side by `SingletonConnection` (clones of the ref share
    /// the underlying slot), so the H2 checkout path can expose connection
    /// metadata and poison support to the adapter layer even though
    /// `Singled<…>` itself is opaque.
    pub(crate) fn new(connector: C, h2_ref: super::connection::H2ConnectionRef) -> Self {
        Self { connector, h2_ref }
    }
}

impl<C: Clone> Clone for H2ConnectAndHandshake<C> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
            h2_ref: self.h2_ref.clone(),
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
        let h2_ref = self.h2_ref.clone();
        Box::pin(async move {
            let (io, permit) = fut.await.map_err(Into::into)?;
            let info = capture_info(&io);

            let (tx, conn) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
                .map_err(|e| Box::new(e) as BoxError)?;

            spawn_driver({
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

            let managed = ManagedConnection::new(H2SendRequest::new(tx), info, permit);
            // Publish this connection's metadata for the checkout side.
            // Singleton replaces its stored connection wholesale on each new
            // handshake, so last-writer-wins on the ref is correct.
            h2_ref.publish(managed.metadata());
            Ok(managed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::timeout::test::NeverConnects;
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use std::time::Duration;
    use tower::Service as _;

    /// Verify `ConnectionLimit` applies `connect_timeout` from `ConnectCtx`
    /// to the inner TCP connector. `NeverConnects` returns a connector
    /// future that never resolves; a short connect_timeout should fire
    /// and produce an `HTTP connect timeout occurred after …` error.
    #[tokio::test(start_paused = true)]
    async fn connect_timeout_fires_on_slow_connector() {
        let mut svc = ConnectionLimit::new(NeverConnects::default(), None, None);
        let sleep = SharedAsyncSleep::new(TokioSleep::new());
        let ctx = ConnectCtx::new(
            "http://example.com".parse().unwrap(),
            Some(super::super::connection::TimeoutContext::new(
                Duration::from_millis(500),
                sleep,
            )),
        );
        let err = match svc.call(ctx).await {
            Ok(_) => panic!("connect timeout should fire against a never-resolving connector"),
            Err(err) => err,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("HTTP connect"),
            "expected `HTTP connect` in error, got: {msg}"
        );
        assert!(
            msg.contains("500ms"),
            "expected `500ms` in error, got: {msg}"
        );
    }

    /// Without a connect_timeout set, `ConnectionLimit` passes through the
    /// connector call with no wrapping — verify the absence of a timeout
    /// means the slow connector stays pending (we just check we can start
    /// the call; with `start_paused`, nothing advances so the future is
    /// still pending after a short yield).
    #[tokio::test(start_paused = true)]
    async fn no_timeout_does_not_bound_connector() {
        let mut svc = ConnectionLimit::new(NeverConnects::default(), None, None);
        let ctx = ConnectCtx::new("http://example.com".parse().unwrap(), None);
        let fut = svc.call(ctx);
        // A brief tokio yield shouldn't resolve the never-connects future.
        tokio::pin!(fut);
        tokio::select! {
            _ = &mut fut => panic!("future should not resolve without a timeout"),
            _ = tokio::task::yield_now() => {}
        }
    }
}

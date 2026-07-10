/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tower service adapters for hyper's HTTP protocol handshake.
//!
//! - `H1ConnectAndHandshake` / `H2ConnectAndHandshake`: perform the protocol
//!   handshake on an already-connected IO stream, producing a `ManagedConnection`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use hyper_util::rt::TokioExecutor;
use tower::Service;

use super::connection::{
    Authority, ConnectCtx, ConnectionCreatedEvent, ConnectionFailedEvent, ConnectionInfo,
    ConnectionTiming, EstablishedConnection, ManagedConnection, NegotiatedProtocol,
};
use super::partition::DriverSpawner;

/// Pool-scoped instrumentation primitives shared across layers in the
/// pool stack. Held by `ConnectionPool` for the pool's lifetime and
/// cloned into individual layers (the H1/H2 handshake services) at
/// construction.
///
/// Cheap to clone (one `Arc` per primitive).
#[derive(Clone)]
pub(crate) struct PoolHooks {
    conn_id_counter: Arc<std::sync::atomic::AtomicU64>,
    pub(crate) listener: Option<Arc<dyn super::connection::ConnectionEventListener>>,
}

impl PoolHooks {
    pub(crate) fn new(
        listener: Option<Arc<dyn super::connection::ConnectionEventListener>>,
    ) -> Self {
        Self {
            conn_id_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            listener,
        }
    }

    /// Mint the next connection id. Stable for the connection's lifetime;
    /// the underlying counter wraps at `u64::MAX`.
    pub(crate) fn next_conn_id(&self) -> ConnectionId {
        ConnectionId::new(
            self.conn_id_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Fire the listener's connection-created callback, if a listener is set.
    pub(crate) fn on_created(&self, event: &ConnectionCreatedEvent) {
        if let Some(ref l) = self.listener {
            l.on_created(event);
        }
    }

    /// Fire the listener's connection-failed callback, if a listener is set.
    pub(crate) fn on_connection_failed(&self, event: &ConnectionFailedEvent) {
        if let Some(ref l) = self.listener {
            l.on_connection_failed(event);
        }
    }

    /// Fire the listener's connection-reused callback, if a listener is set.
    pub(crate) fn on_reused(&self, event: &super::connection::ConnectionReusedEvent) {
        if let Some(ref l) = self.listener {
            l.on_reused(event);
        }
    }

    /// Fire the listener's connection-closed callback, if a listener is set.
    pub(crate) fn on_closed(&self, event: &super::connection::ConnectionClosedEvent) {
        if let Some(ref l) = self.listener {
            l.on_closed(event);
        }
    }

    /// Fire the listener's connection-borrowed callback, if a listener is set.
    pub(crate) fn on_borrowed(&self, event: &super::connection::ConnectionBorrowedEvent) {
        if let Some(ref l) = self.listener {
            l.on_borrowed(event);
        }
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
/// Clone is supported because HTTP/2 multiplexes requests over a single connection.
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
fn capture_info<IO: Connection>(io: &IO, authority: Authority) -> ConnectionInfo {
    let connected = io.connected();
    let is_proxied = connected.is_proxied();
    let mut extras = http_1x::Extensions::new();
    connected.get_extras(&mut extras);
    let http_info = extras.get::<HttpInfo>();
    ConnectionInfo {
        remote_addr: http_info.map(|i| i.remote_addr()),
        local_addr: http_info.map(|i| i.local_addr()),
        is_proxied,
        authority,
    }
}

/// Connects and performs an HTTP/1.1 handshake, spawning the connection
/// driver onto the partition's runtime via the captured [`DriverSpawner`].
///
/// The connector is expected to return an [`EstablishedConnection`], typically
/// produced by the admission stack wrapping a TCP/TLS connector.
pub(crate) struct H1ConnectAndHandshake<C> {
    connector: C,
    hooks: PoolHooks,
    spawner: Arc<dyn DriverSpawner>,
}

impl<C> H1ConnectAndHandshake<C> {
    pub(crate) fn new(connector: C, hooks: PoolHooks, spawner: Arc<dyn DriverSpawner>) -> Self {
        Self {
            connector,
            hooks,
            spawner,
        }
    }
}

impl<C: Clone> Clone for H1ConnectAndHandshake<C> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
            hooks: self.hooks.clone(),
            spawner: self.spawner.clone(),
        }
    }
}

impl<C, IO> Service<ConnectCtx> for H1ConnectAndHandshake<C>
where
    C: Service<ConnectCtx, Response = EstablishedConnection<IO>>,
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
        let hooks = self.hooks.clone();
        let spawner = self.spawner.clone();
        let authority = Authority::new(
            ctx.uri
                .authority()
                .expect("request URI has authority")
                .as_str(),
        );
        let fut = self.connector.call(ctx);
        Box::pin(async move {
            let connect_start = Instant::now();
            let EstablishedConnection {
                io,
                permit,
                establishing,
            } = match fut.await.map_err(Into::into) {
                Ok(v) => v,
                Err(e) => {
                    hooks.on_connection_failed(&ConnectionFailedEvent::new(
                        authority,
                        None,
                        Into::into(e.to_string()),
                    ));
                    return Err(e);
                }
            };
            let connect_duration = connect_start.elapsed();
            let info = capture_info(&io, authority.clone());
            let conn_id = hooks.next_conn_id();

            let (tx, conn) = match hyper::client::conn::http1::Builder::new()
                .handshake(io)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    let boxed: BoxError = Box::new(e);
                    hooks.on_connection_failed(&ConnectionFailedEvent::new(
                        authority,
                        info.remote_addr,
                        Into::into(boxed.to_string()),
                    ));
                    return Err(boxed);
                }
            };

            tracing::debug!(
                conn_id = %conn_id,
                protocol = "h1",
                remote = ?info.remote_addr,
                local = ?info.local_addr,
                "pool: connection established"
            );

            hooks.on_created(&ConnectionCreatedEvent::new(
                conn_id,
                authority,
                info.remote_addr,
                NegotiatedProtocol::Http1,
                ConnectionTiming::new(connect_duration),
            ));

            let established = establishing.promote(super::stats::PROTO_H1);

            spawner.spawn(Box::pin({
                let remote_addr = info.remote_addr;
                let local_addr = info.local_addr;
                async move {
                    if let Err(e) = conn.with_upgrades().await {
                        tracing::debug!(
                            conn_id = %conn_id,
                            protocol = "h1",
                            ?remote_addr,
                            ?local_addr,
                            error = %e,
                            "pool: connection driver error"
                        );
                    }
                }
            }));

            Ok(ManagedConnection::new(
                H1SendRequest::new(tx),
                info,
                conn_id,
                permit,
                established,
            ))
        })
    }
}

/// Connects and performs an HTTP/2 handshake, spawning the connection
/// driver onto the partition's runtime via the captured [`DriverSpawner`].
///
/// Pinned to `Service<()>` because this service sits in the Negotiate
/// upgrade path: the connection is already established via the shared
/// `Inspected` slot.
pub(crate) struct H2ConnectAndHandshake<C> {
    connector: C,
    h2_ref: super::connection::H2ConnectionRef,
    hooks: PoolHooks,
    authority: Authority,
    spawner: Arc<dyn DriverSpawner>,
}

impl<C> H2ConnectAndHandshake<C> {
    /// Create an H2 handshake service that publishes each newly established
    /// connection's state — `ConnectionMetadata` plus the shared
    /// `active_streams` counter and `idle_at` timestamp — into `h2_ref`. The
    /// same ref is held on the read side by `SingletonConnection` (clones of
    /// the ref share the underlying slot), so the H2 checkout path can expose
    /// connection metadata and poison support to the adapter layer, and track
    /// stream occupancy, even though `Singled<…>` itself is opaque.
    pub(crate) fn new(
        connector: C,
        h2_ref: super::connection::H2ConnectionRef,
        hooks: PoolHooks,
        authority: Authority,
        spawner: Arc<dyn DriverSpawner>,
    ) -> Self {
        Self {
            connector,
            h2_ref,
            hooks,
            authority,
            spawner,
        }
    }
}

impl<C: Clone> Clone for H2ConnectAndHandshake<C> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
            h2_ref: self.h2_ref.clone(),
            hooks: self.hooks.clone(),
            authority: self.authority.clone(),
            spawner: self.spawner.clone(),
        }
    }
}

impl<C, IO> Service<()> for H2ConnectAndHandshake<C>
where
    C: Service<(), Response = EstablishedConnection<IO>>,
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
        let hooks = self.hooks.clone();
        let authority = self.authority.clone();
        let spawner = self.spawner.clone();
        Box::pin(async move {
            let connect_start = Instant::now();
            let EstablishedConnection {
                io,
                permit,
                establishing,
            } = match fut.await.map_err(Into::into) {
                Ok(v) => v,
                Err(e) => {
                    hooks.on_connection_failed(&ConnectionFailedEvent::new(
                        authority,
                        None,
                        Into::into(e.to_string()),
                    ));
                    return Err(e);
                }
            };
            let connect_duration = connect_start.elapsed();
            let info = capture_info(&io, authority.clone());
            let conn_id = hooks.next_conn_id();

            let (tx, conn) = match hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    let boxed: BoxError = Box::new(e);
                    hooks.on_connection_failed(&ConnectionFailedEvent::new(
                        authority,
                        info.remote_addr,
                        Into::into(boxed.to_string()),
                    ));
                    return Err(boxed);
                }
            };

            tracing::debug!(
                conn_id = %conn_id,
                protocol = "h2",
                remote = ?info.remote_addr,
                local = ?info.local_addr,
                "pool: connection established"
            );

            hooks.on_created(&ConnectionCreatedEvent::new(
                conn_id,
                authority,
                info.remote_addr,
                NegotiatedProtocol::Http2,
                ConnectionTiming::new(connect_duration),
            ));

            let established = establishing.promote(super::stats::PROTO_H2);

            spawner.spawn(Box::pin({
                let remote_addr = info.remote_addr;
                let local_addr = info.local_addr;
                async move {
                    if let Err(e) = conn.await {
                        tracing::debug!(
                            conn_id = %conn_id,
                            protocol = "h2",
                            ?remote_addr,
                            ?local_addr,
                            error = %e,
                            "pool: connection driver error"
                        );
                    }
                }
            }));

            let managed =
                ManagedConnection::new(H2SendRequest::new(tx), info, conn_id, permit, established);
            // Publish this connection's state (metadata + active_streams +
            // idle_at refs) for the checkout side. Singleton replaces its
            // stored connection wholesale on each new handshake, so
            // last-writer-wins on the ref is correct.
            h2_ref.publish(super::connection::H2ConnectionState {
                metadata: managed.metadata(),
                active_streams: managed.active_streams_ref(),
                idle_at: managed.idle_at_ref(),
            });
            Ok(managed)
        })
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection state tracking for pooled connections.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::client::connection::{ConnectionId, ConnectionMetadata};
use aws_smithy_types::body::SdkBody;
use pin_project_lite::pin_project;
use tokio::sync::OwnedSemaphorePermit;
use tower::Service;

use super::cache;
use super::handshake::H1SendRequest;
use super::BoxError;

/// Opaque authority (host:port) associated with a pooled connection.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Authority(Arc<str>);

impl Authority {
    pub(crate) fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }

    /// The authority as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Authority {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Protocol negotiated for a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NegotiatedProtocol {
    /// HTTP/1.1
    Http1,
    /// HTTP/2
    Http2,
}

/// Why a connection was removed from the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseReason {
    /// Idle longer than the configured pool idle timeout.
    IdleTimeout,
    /// Marked as poisoned (unhealthy) by the SDK or pool.
    Poisoned,
    /// The pool itself was dropped.
    PoolDropped,
}

/// Emitted when a new connection is established (TCP + TLS + HTTP handshake).
#[derive(Debug)]
pub struct ConnectionCreatedEvent {
    conn_id: ConnectionId,
    authority: Authority,
    remote_addr: Option<SocketAddr>,
    protocol: NegotiatedProtocol,
}

impl ConnectionCreatedEvent {
    pub(crate) fn new(
        conn_id: ConnectionId,
        authority: Authority,
        remote_addr: Option<SocketAddr>,
        protocol: NegotiatedProtocol,
    ) -> Self {
        Self {
            conn_id,
            authority,
            remote_addr,
            protocol,
        }
    }

    /// The pool-assigned connection identifier.
    pub fn conn_id(&self) -> ConnectionId {
        self.conn_id
    }

    /// The authority (host:port) this connection is for.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Remote address of the peer, if known.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Negotiated protocol.
    pub fn protocol(&self) -> NegotiatedProtocol {
        self.protocol
    }
}

/// Emitted when an existing idle connection is checked out from the pool.
#[derive(Debug)]
pub struct ConnectionReusedEvent {
    conn_id: ConnectionId,
    authority: Authority,
}

impl ConnectionReusedEvent {
    pub(crate) fn new(conn_id: ConnectionId, authority: Authority) -> Self {
        Self { conn_id, authority }
    }

    /// The pool-assigned connection identifier.
    pub fn conn_id(&self) -> ConnectionId {
        self.conn_id
    }

    /// The authority (host:port) this connection is for.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }
}

/// Emitted when a connection is removed from the pool.
pub struct ConnectionClosedEvent {
    conn_id: ConnectionId,
    authority: Authority,
    remote_addr: Option<SocketAddr>,
    reason: CloseReason,
    error: Option<BoxError>,
}

impl std::fmt::Debug for ConnectionClosedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("ConnectionClosedEvent");
        s.field("conn_id", &self.conn_id)
            .field("authority", &self.authority)
            .field("remote_addr", &self.remote_addr)
            .field("reason", &self.reason);
        if let Some(ref e) = self.error {
            s.field("error", &format_args!("{e}"));
        }
        s.finish()
    }
}

impl ConnectionClosedEvent {
    pub(crate) fn new(
        conn_id: ConnectionId,
        authority: Authority,
        remote_addr: Option<SocketAddr>,
        reason: CloseReason,
        error: Option<BoxError>,
    ) -> Self {
        Self {
            conn_id,
            authority,
            remote_addr,
            reason,
            error,
        }
    }

    /// The pool-assigned connection identifier.
    pub fn conn_id(&self) -> ConnectionId {
        self.conn_id
    }

    /// The authority (host:port) this connection was for.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Remote address of the peer, if known.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Why the connection was closed.
    pub fn reason(&self) -> CloseReason {
        self.reason
    }

    /// The error associated with this close, if any. Present for
    /// server-initiated closes; `None` for policy-driven closes
    /// (idle timeout, poisoning).
    pub fn error(&self) -> Option<&(dyn std::error::Error + Send + Sync)> {
        self.error.as_ref().map(|e| e.as_ref())
    }
}

/// Emitted when a connection attempt fails before completing the handshake.
pub struct ConnectionFailedEvent {
    authority: Authority,
    remote_addr: Option<SocketAddr>,
    error: BoxError,
}

impl std::fmt::Debug for ConnectionFailedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionFailedEvent")
            .field("authority", &self.authority)
            .field("remote_addr", &self.remote_addr)
            .field("error", &format_args!("{}", self.error))
            .finish()
    }
}

impl ConnectionFailedEvent {
    pub(crate) fn new(
        authority: Authority,
        remote_addr: Option<SocketAddr>,
        error: BoxError,
    ) -> Self {
        Self {
            authority,
            remote_addr,
            error,
        }
    }

    /// The authority (host:port) the connection was attempting to reach.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Remote address of the peer, if known. `None` when the failure occurred
    /// before a peer address was established (e.g., DNS resolution failure or
    /// connection refused before address binding).
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// The error that caused the connection attempt to fail.
    pub fn error(&self) -> &(dyn std::error::Error + Send + Sync) {
        self.error.as_ref()
    }
}

/// Callback for connection lifecycle events within the pool.
///
/// Implementations receive notifications when connections are created,
/// reused from the pool, closed, or fail to establish. Useful for metrics,
/// DNS failure feedback, or adaptive concurrency control.
///
/// Implementations must be cheap (nanoseconds). Expensive work should be
/// deferred to a background task.
pub trait ConnectionEventListener: Send + Sync + 'static {
    /// A new connection was established.
    fn on_created(&self, _event: &ConnectionCreatedEvent) {}
    /// An existing idle connection was checked out from the pool.
    fn on_reused(&self, _event: &ConnectionReusedEvent) {}
    /// A connection was removed from the pool.
    fn on_closed(&self, _event: &ConnectionClosedEvent) {}
    /// A connection attempt failed before completing the handshake.
    fn on_connection_failed(&self, _event: &ConnectionFailedEvent) {}
}

/// A duration paired with the sleep implementation used to realize it.
///
/// This type makes "timeout without sleep impl" unrepresentable: you cannot
/// construct one without committing to a way to actually wait. Created at the
/// adapter layer from `HttpConnectorSettings` + `RuntimeComponents::sleep_impl()`
/// and passed down the pool stack where timeouts are applied.
#[derive(Clone, Debug)]
pub(crate) struct TimeoutContext {
    pub(crate) duration: Duration,
    pub(crate) sleep_impl: SharedAsyncSleep,
}

impl TimeoutContext {
    pub(crate) fn new(duration: Duration, sleep_impl: SharedAsyncSleep) -> Self {
        Self {
            duration,
            sleep_impl,
        }
    }
}

/// Target type for the connect portion of the pool stack.
///
/// Replaces bare `Uri` so per-operation connect metadata flows through the
/// composable pool types (Map → Negotiate → Cache → handshake → ConnectionLimit
/// → TCP connector) to the layers that need them.
#[derive(Clone, Debug)]
pub(crate) struct ConnectCtx {
    pub(crate) uri: http_1x::Uri,
    /// Bounds new-connection establishment (TCP + TLS handshake). `None`
    /// means no connect timeout; cached connections skip the connector
    /// entirely so this is automatically a no-op on cache hit.
    pub(crate) connect_timeout: Option<TimeoutContext>,
}

impl ConnectCtx {
    pub(crate) fn new(uri: http_1x::Uri, connect_timeout: Option<TimeoutContext>) -> Self {
        Self {
            uri,
            connect_timeout,
        }
    }
}

/// Request extension set by the adapter to hint a read timeout to the
/// checkout services (`H{1,2}Checkout`).
///
/// `Some(...)` means: once the connection is selected (cache hit or fresh
/// handshake), wrap `conn.call(req)` with this timeout. Bounds request-write
/// + response-headers-wait only. Does NOT include pool acquire or connect
/// establishment (those have their own timeouts).
#[derive(Clone, Debug)]
pub(crate) struct ReadTimeoutHint(pub(crate) TimeoutContext);

/// Permits acquired from connection limit semaphores.
/// Held for the lifetime of the connection; dropped when the connection is dropped.
pub(crate) struct ConnectionPermit {
    _global: Option<OwnedSemaphorePermit>,
    _per_host: Option<OwnedSemaphorePermit>,
}

impl ConnectionPermit {
    pub(crate) fn new(
        global: Option<OwnedSemaphorePermit>,
        per_host: Option<OwnedSemaphorePermit>,
    ) -> Self {
        Self {
            _global: global,
            _per_host: per_host,
        }
    }
}

/// Error returned by `ManagedConnection::poll_ready` when the connection
/// has been marked poisoned and should not be reused.
#[derive(Debug)]
pub(crate) struct PoisonedError;

impl std::fmt::Display for PoisonedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection poisoned")
    }
}

impl std::error::Error for PoisonedError {}

/// Metadata about a connection captured at establishment time.
///
/// Captured between TLS connector output and protocol handshake (the last point
/// where the raw transport stream is accessible).
#[derive(Debug, Clone)]
pub(crate) struct ConnectionInfo {
    /// Remote address of the peer. `None` when the underlying connector
    /// did not attach `HttpInfo` to its `Connected` extras.
    pub(crate) remote_addr: Option<SocketAddr>,
    /// Local address of this end of the connection.
    pub(crate) local_addr: Option<SocketAddr>,
    /// `true` when this connection is to an HTTP proxy server (rather than
    /// directly to the origin). Drives request-target form selection: H1
    /// requests dispatched on a proxied connection use absolute-form URIs;
    /// direct connections use origin-form. Populated from
    /// [`hyper_util::client::legacy::connect::Connected::is_proxied`] at
    /// handshake.
    pub(crate) is_proxied: bool,
    /// The authority (host:port) this connection is for. Populated from the
    /// URI at handshake time.
    pub(crate) authority: Authority,
    // Future: tls_info, timing (tcp_connect_duration, tls_handshake_duration)
}

/// A one-shot "this connection is dead, don't reuse it" flag.
///
/// Shared via `Arc`; all clones observe and control the same flag.
/// `ManagedConnection` holds one and hands out clones (via `metadata()`)
/// that let the adapter layer mark the connection poisoned through
/// smithy's `ConnectionMetadata::poison_fn`. On next checkout / return,
/// the pool sees the flag set and drops the connection instead of reusing
/// it. Modeled after hyper-util's legacy `PoisonPill`, but owned by us.
#[derive(Debug, Clone, Default)]
pub(crate) struct PoisonPill {
    flag: Arc<AtomicBool>,
}

impl PoisonPill {
    /// Create a fresh, non-poisoned pill.
    pub(crate) fn healthy() -> Self {
        Self::default()
    }

    /// Mark the connection as poisoned.
    pub(crate) fn poison(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Whether the connection has been poisoned.
    pub(crate) fn is_poisoned(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

/// A connection with SDK-owned lifecycle metadata.
///
/// Wraps the inner service (typically `SendRequest<SdkBody>`) with state needed
/// for pool management: poisoning, connection identity, and permit lifetime.
///
/// Clone is supported when the inner service is Clone (e.g., HTTP/2 multiplexed
/// connections). Clones share the same poison pill and connection info, so
/// poisoning one clone poisons all of them.
pub(crate) struct ManagedConnection<S> {
    inner: S,
    pub(crate) info: ConnectionInfo,
    /// Stable identifier for this physical connection, unique within the
    /// owning pool. Shared across `Clone`s (an H2 connection's multiplexed
    /// request handles all carry the same `conn_id`). Used in tracing and
    /// surfaced through `ConnectionMetadata` for cross-layer correlation.
    conn_id: ConnectionId,
    created_at: Instant,
    /// Timestamp the connection last became idle (or its creation time, if
    /// it has never been returned to the pool).
    ///
    /// - **H1**: stamped by [`CachedConnection::drop`] on the unpoisoned
    ///   return-to-pool path. Presence in the cache implies idle.
    /// - **H2**: stamped by [`SingletonConnection::drop`] on the
    ///   `active_streams` 1 → 0 transition. Combined with
    ///   `active_streams > 0` in the H2 retain predicate, this prevents
    ///   eviction of a multiplexed connection that's still serving streams.
    ///
    /// `Arc<Mutex<_>>` because all `Clone`s of a `ManagedConnection`
    /// observe the same timestamp (last-write-wins). `Mutex` over an
    /// atomic-encoded `u64` because writes happen at most once per
    /// request and reads at most once per eviction tick.
    idle_at: Arc<Mutex<Instant>>,
    /// In-flight request count, used by the H2 retain predicate to keep
    /// actively-multiplexed connections alive regardless of `idle_at`.
    ///
    /// - **H1**: never incremented (always 0). H1 is serial; the cache's
    ///   idle set is the authoritative idleness signal.
    /// - **H2**: incremented by [`SingletonConnection::call`] on dispatch,
    ///   decremented by [`SingletonConnection::drop`] when the body guard
    ///   releases.
    ///
    /// Arc-shared so all `Clone`s of an H2 `ManagedConnection` mutate the
    /// same counter.
    active_streams: Arc<std::sync::atomic::AtomicUsize>,
    poison: PoisonPill,
    _permit: Arc<ConnectionPermit>,
}

impl<S> ManagedConnection<S> {
    /// Create a new managed connection wrapping the given service.
    pub(crate) fn new(
        inner: S,
        info: ConnectionInfo,
        conn_id: ConnectionId,
        permit: Arc<ConnectionPermit>,
    ) -> Self {
        let now = Instant::now();
        Self {
            inner,
            info,
            conn_id,
            created_at: now,
            idle_at: Arc::new(Mutex::new(now)),
            active_streams: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            poison: PoisonPill::healthy(),
            _permit: permit,
        }
    }

    /// Stable identifier for this physical connection. Shared across
    /// clones (H2 multiplexing).
    pub(crate) fn conn_id(&self) -> ConnectionId {
        self.conn_id
    }

    /// Whether this connection has been poisoned.
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poison.is_poisoned()
    }

    /// Connection info (remote/local addresses).
    #[allow(dead_code)] // future telemetry / debugging
    pub(crate) fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    /// When this connection was established.
    #[allow(dead_code)] // future max-lifetime eviction
    pub(crate) fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Timestamp of the last return-to-idle (or creation).
    ///
    /// Guaranteed meaningful for any connection sitting in the pool: the
    /// initial value is `created_at`, and every return-to-pool overwrites
    /// it via [`Self::mark_idle`].
    pub(crate) fn idle_at(&self) -> Instant {
        *self.idle_at.lock().expect("idle_at lock poisoned")
    }

    /// Stamp the return-to-idle moment. Called by [`CachedConnection::drop`]
    /// on the unpoisoned path.
    pub(crate) fn mark_idle(&self) {
        *self.idle_at.lock().expect("idle_at lock poisoned") = Instant::now();
    }

    /// Current in-flight stream count. See [`Self::active_streams`]
    /// (field doc) for semantics.
    pub(crate) fn active_streams_count(&self) -> usize {
        self.active_streams
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Clone of the Arc'd stream counter, for publication through the H2
    /// side-channel so `SingletonConnection` can increment/decrement it
    /// without reaching through the opaque `Singled<…>`.
    pub(crate) fn active_streams_ref(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.active_streams.clone()
    }

    /// Clone of the Arc'd idle timestamp, for publication through the H2
    /// side-channel so `SingletonConnection::drop` can stamp it on the
    /// `active_streams` 1 → 0 transition.
    pub(crate) fn idle_at_ref(&self) -> Arc<Mutex<Instant>> {
        self.idle_at.clone()
    }

    /// Mutable access to the inner service.
    pub(crate) fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Build a smithy `ConnectionMetadata` for this connection.
    ///
    /// The returned metadata captures a clone of the `PoisonPill`, so
    /// calling `ConnectionMetadata::poison()` flips this connection's
    /// poison flag (the same flag the pool checks on checkout/return).
    /// Address fields are copied.
    pub(crate) fn metadata(&self) -> ConnectionMetadata {
        let poison = self.poison.clone();
        let conn_id = self.conn_id;
        let remote = self.info.remote_addr;
        let mut builder = ConnectionMetadata::builder()
            .proxied(self.info.is_proxied)
            .connection_id(self.conn_id)
            .poison_fn(move || {
                tracing::debug!(conn_id = %conn_id, ?remote, "pool: connection poisoned");
                poison.poison();
            });
        builder
            .set_remote_addr(self.info.remote_addr)
            .set_local_addr(self.info.local_addr);
        builder.build()
    }

    /// `true` when the underlying connection is to an HTTP proxy. Used by
    /// H1 dispatch to choose absolute-form URIs over origin-form.
    pub(crate) fn is_proxied(&self) -> bool {
        self.info.is_proxied
    }
}

impl<S: Clone> Clone for ManagedConnection<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            info: self.info.clone(),
            conn_id: self.conn_id,
            created_at: self.created_at,
            idle_at: self.idle_at.clone(),
            active_streams: self.active_streams.clone(),
            poison: self.poison.clone(),
            _permit: self._permit.clone(),
        }
    }
}

impl<S> Service<http_1x::Request<SdkBody>> for ManagedConnection<S>
where
    S: Service<http_1x::Request<SdkBody>>,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
    S::Response: 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.is_poisoned() {
            return Poll::Ready(Err(PoisonedError.into()));
        }
        self.inner_mut().poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        let fut = self.inner_mut().call(req);
        Box::pin(async move { fut.await.map_err(Into::into) })
    }
}

/// A connection checked out from the H1 cache.
///
/// Wraps `cache::Cached<ManagedConnection<S>>` so that poisoned connections
/// are dropped from the pool (via `Cached::discard`) instead of being
/// returned for reuse when dropped. Healthy connections return to the pool
/// normally through `Cached::Drop`.
pub(crate) struct CachedConnection<S> {
    inner: Option<cache::Cached<ManagedConnection<S>>>,
    listener: Option<Arc<dyn ConnectionEventListener>>,
}

impl<S> CachedConnection<S> {
    pub(crate) fn new(
        cached: cache::Cached<ManagedConnection<S>>,
        listener: Option<Arc<dyn ConnectionEventListener>>,
    ) -> Self {
        let is_reuse = *cached.inner().idle_at.lock().unwrap() > cached.inner().created_at;
        if is_reuse {
            let conn_id = cached.inner().conn_id();
            let authority = cached.inner().info.authority.clone();
            tracing::trace!(conn_id = %conn_id, "pool: connection reused");
            if let Some(ref l) = listener {
                l.on_reused(&ConnectionReusedEvent::new(conn_id, authority));
            }
        }
        Self {
            inner: Some(cached),
            listener,
        }
    }

    /// Build a smithy `ConnectionMetadata` for the underlying H1 connection.
    ///
    /// See [`ManagedConnection::metadata`].
    ///
    /// Panics if called after the inner cached handle has been consumed.
    pub(crate) fn metadata(&self) -> ConnectionMetadata {
        self.inner
            .as_ref()
            .expect("CachedConnection metadata after drop")
            .inner()
            .metadata()
    }

    /// Forwards [`ManagedConnection::is_proxied`].
    pub(crate) fn is_proxied(&self) -> bool {
        self.inner
            .as_ref()
            .expect("CachedConnection is_proxied after drop")
            .inner()
            .is_proxied()
    }

    /// Forwards [`ManagedConnection::conn_id`].
    pub(crate) fn conn_id(&self) -> ConnectionId {
        self.inner
            .as_ref()
            .expect("CachedConnection conn_id after drop")
            .inner()
            .conn_id()
    }
}

impl<S, Req> Service<Req> for CachedConnection<S>
where
    cache::Cached<ManagedConnection<S>>: Service<Req>,
{
    type Response = <cache::Cached<ManagedConnection<S>> as Service<Req>>::Response;
    type Error = <cache::Cached<ManagedConnection<S>> as Service<Req>>::Error;
    type Future = <cache::Cached<ManagedConnection<S>> as Service<Req>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.as_mut().unwrap().poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        self.inner.as_mut().unwrap().call(req)
    }
}

impl<S> Drop for CachedConnection<S> {
    fn drop(&mut self) {
        if let Some(cached) = self.inner.take() {
            let managed = cached.inner();
            let conn_id = managed.conn_id;
            if managed.is_poisoned() {
                tracing::debug!(conn_id = %conn_id, "pool: connection discarded (poisoned)");
                if let Some(ref listener) = self.listener {
                    listener.on_closed(&ConnectionClosedEvent::new(
                        managed.conn_id(),
                        managed.info.authority.clone(),
                        managed.info.remote_addr,
                        CloseReason::Poisoned,
                        None,
                    ));
                }
                cached.discard();
            } else {
                managed.mark_idle();
                tracing::trace!(conn_id = %conn_id, "pool: connection returned to idle");
            }
        }
    }
}

/// Per-host shared slot that bridges H2 handshake completion to H2 checkout.
///
/// `H2ConnectAndHandshake` calls [`Self::publish`] with a fresh
/// `ConnectionMetadata` every time a new H2 connection is established.
/// `SingletonConnection::metadata` reads the current value through
/// [`Self::current`].
///
/// Necessary because `hyper_util::client::pool::singleton::Singled<…>` is
/// opaque to us; we can't reach through it to the underlying
/// `ManagedConnection` to build its metadata at checkout time. The
/// handshake side has direct access, so it publishes and the checkout
/// side reads.
///
/// Last-writer-wins is correct. Singleton holds at most one live H2
/// connection per host at a time: on `Singled::poll_ready` error (or a
/// `Singleton::retain` predicate returning false), its state transitions
/// from `Made(svc)` back to `Empty`, dropping the old service. The next
/// request's `call` runs a fresh handshake through `H2ConnectAndHandshake`,
/// which publishes new metadata, overwriting whatever was there before.
/// State an H2 checkout (`SingletonConnection`) needs but cannot reach
/// through the opaque `Singled<…>` to read from the underlying
/// `ManagedConnection`. Published by the H2 handshake on each new
/// connection; consumed by `SingletonConnection::new` as a snapshot.
///
/// `active_streams` and `idle_at` are clones of the Arcs held by the
/// `ManagedConnection`, so updates through this state are visible to the
/// retain predicate operating on the connection directly.
#[derive(Clone)]
pub(crate) struct H2ConnectionState {
    pub(crate) metadata: ConnectionMetadata,
    pub(crate) active_streams: Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) idle_at: Arc<Mutex<Instant>>,
}

/// Per-host side-channel between H2 handshake (writer) and H2 checkout
/// (reader).
///
/// `hyper_util::client::pool::singleton::Singled<…>` is opaque, so the
/// checkout side cannot reach through it to the underlying
/// `ManagedConnection`. This ref carries everything the checkout side
/// needs (stamped fresh on each handshake): the user-facing
/// `ConnectionMetadata` for poison and address surfacing, plus the
/// Arc-shared `active_streams` counter and `idle_at` timestamp that the
/// retain predicate consults and `SingletonConnection` mutates.
///
/// On re-handshake (poisoning, GOAWAY, etc.), the entire state is
/// replaced last-writer-wins. A `SingletonConnection` constructed against
/// the previous handshake holds its own snapshot, so its
/// increments/decrements still target the right counter.
#[derive(Clone, Default)]
pub(crate) struct H2ConnectionRef {
    inner: Arc<std::sync::Mutex<Option<H2ConnectionState>>>,
}

impl H2ConnectionRef {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn publish(&self, state: H2ConnectionState) {
        *self.inner.lock().unwrap() = Some(state);
    }

    /// Snapshot of the published state. `None` until the first
    /// successful handshake for this host.
    pub(crate) fn current(&self) -> Option<H2ConnectionState> {
        self.inner.lock().unwrap().clone()
    }
}

/// RAII guard for one in-flight dispatch against an H2 connection.
///
/// Existence reflects "this checkout has dispatched a request; the
/// connection is busy on its behalf." Constructed by
/// [`SingletonConnection::call`] when a request is dispatched; dropped
/// when the response body's guard releases.
///
/// Drop decrements `active_streams` and, on the 1 → 0 transition, stamps
/// `idle_at` (the moment the connection becomes truly idle).
struct DispatchGuard {
    active_streams: Arc<std::sync::atomic::AtomicUsize>,
    idle_at: Arc<Mutex<Instant>>,
}

impl DispatchGuard {
    /// Start a dispatch against the connection described by `state`.
    /// Increments `active_streams`; the returned guard releases the
    /// increment on drop.
    fn start(state: &H2ConnectionState) -> Self {
        state
            .active_streams
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self {
            active_streams: state.active_streams.clone(),
            idle_at: state.idle_at.clone(),
        }
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        let prev = self
            .active_streams
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        if prev == 1 {
            if let Ok(mut idle_at) = self.idle_at.lock() {
                *idle_at = Instant::now();
            }
        }
    }
}

/// A checked-out H2 connection ready to dispatch requests.
///
/// H2 is multiplexed: the underlying `ManagedConnection` stays resident in
/// `Singleton` for the duration of every concurrent request, so "presence
/// in a cache" (the idleness signal we use for H1) doesn't apply.
/// Instead, `SingletonConnection` mints a [`DispatchGuard`] on each
/// `call`; the guard's lifetime tracks one in-flight request and its
/// drop releases the corresponding `active_streams` slot, stamping
/// `idle_at` on the final release. The H2 retain predicate keeps the
/// connection alive while `active_streams > 0` regardless of `idle_at`.
///
/// Lifecycle:
/// - Constructed in the H2 upgrade `map_response` after `Singleton::call`
///   completes. Snapshots the current [`H2ConnectionState`] and holds it
///   for the checkout's lifetime.
/// - [`Self::call`] starts a [`DispatchGuard`] before delegating to
///   `Singled::call`. The guard is held in `dispatch` for the rest of
///   this `SingletonConnection`'s lifetime.
/// - On drop, the guard's drop releases the active-stream count and may
///   stamp `idle_at`. If `call` was never invoked (e.g. the surrounding
///   checkout was abandoned by the post-checkout `poll_ready` retry
///   loop), `dispatch` is `None` and drop is a no-op.
pub(crate) struct SingletonConnection<T> {
    inner: T,
    /// Snapshot of the H2 side-channel state taken at construction.
    ///
    /// Held for the lifetime of this checkout so dispatch guards always
    /// target the connection this checkout was issued against, even if a
    /// re-handshake replaces the underlying `H2ConnectionRef`'s published
    /// state in the meantime.
    state: Option<H2ConnectionState>,
    /// Active-stream guard. `Some` between [`Self::call`] and drop.
    /// `None` if `call` was never invoked or before `call` runs.
    dispatch: Option<DispatchGuard>,
}

impl<T> SingletonConnection<T> {
    pub(crate) fn new(inner: T, h2_ref: H2ConnectionRef) -> Self {
        let state = h2_ref.current();
        Self {
            inner,
            state,
            dispatch: None,
        }
    }

    /// Metadata for the H2 connection this checkout was issued against.
    ///
    /// Returns `None` only if `SingletonConnection` was constructed before
    /// any handshake had published (which doesn't happen in normal flow,
    /// since `Singleton::call` always completes a handshake before
    /// yielding the `Singled` we wrap).
    pub(crate) fn metadata(&self) -> Option<ConnectionMetadata> {
        self.state.as_ref().map(|s| s.metadata.clone())
    }
}

impl<T, Req> Service<Req> for SingletonConnection<T>
where
    T: Service<Req>,
{
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        if let Some(state) = self.state.as_ref() {
            self.dispatch = Some(DispatchGuard::start(state));
        }
        self.inner.call(req)
    }
}

pin_project! {
    /// A response body that keeps the originating pool checkout alive until
    /// the body is fully consumed.
    ///
    /// # Why this exists
    ///
    /// When a checked-out pool connection's `Service::call` returns, the
    /// response head is available but the body may still be streaming over
    /// the same underlying HTTP connection. For H1 this is critical: if the
    /// `CachedConnection` drops at that point, the connection returns to the
    /// pool mid-body-stream and the next checkout would be handed a still-busy
    /// connection.
    ///
    /// `GuardedBody` holds the checkout (`CachedConnection` for H1,
    /// `SingletonConnection` for H2) in its guard field until the body is
    /// fully dropped. Body streaming continues through the held inner
    /// `Incoming`; when the `GuardedBody` is dropped the guard drops, which
    /// for H1 triggers `CachedConnection::Drop` (return-to-pool or `discard`
    /// if poisoned), and for H2 simply drops the singleton clone (no-op on
    /// singleton state).
    ///
    /// The H2 variant carries a generic type parameter because
    /// `SingletonConnection<T>`'s inner `T` is `hyper_util::client::pool::
    /// singleton::Singled<...>`, which is unnameable outside hyper-util. The
    /// H1 variant is fully concrete since our vendored cache makes
    /// `cache::Cached<...>` nameable.
    pub(crate) struct GuardedBody<H2Unnameable> {
        #[pin]
        inner: hyper::body::Incoming,
        _guard: ConnectionGuard<H2Unnameable>,
    }
}

/// What a `GuardedBody` holds alive while the body streams.
///
/// Explicit per-leg variants so H1 vs H2 bifurcation is visible at the
/// type level and in debugger output.
pub(crate) enum ConnectionGuard<H2Unnameable> {
    H1(CachedConnection<H1SendRequest>),
    H2(SingletonConnection<H2Unnameable>),
}

impl<H2Unnameable> GuardedBody<H2Unnameable> {
    pub(crate) fn new(inner: hyper::body::Incoming, guard: ConnectionGuard<H2Unnameable>) -> Self {
        Self {
            inner,
            _guard: guard,
        }
    }
}

impl<H2Unnameable> hyper::body::Body for GuardedBody<H2Unnameable> {
    type Data = <hyper::body::Incoming as hyper::body::Body>::Data;
    type Error = <hyper::body::Incoming as hyper::body::Body>::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        self.project().inner.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

/// The response type both H1 and H2 pool checkouts produce.
///
/// Carries `GuardedBody<PoolUnnameable>` so the H2 leg can hold its
/// checkout guard (`Singled<…>`, type-erased through `PoolUnnameable`)
/// for the response body's lifetime. `Negotiate` requires uniform
/// response types across its legs; this alias is the uniform type that
/// consumers above the Negotiate composition point work with.
pub(crate) type CheckoutResponse<PoolUnnameable> = http_1x::Response<GuardedBody<PoolUnnameable>>;

#[cfg(test)]
mod tests {
    //! Unit tests for H2 active-stream tracking.
    //!
    //! The test harness (`ConnectionTestHarness`) is plain HTTP only, without
    //! ALPN, so we cannot exercise the full H2 pipeline end-to-end yet
    //! (tracked in bosun.md as the "H2 test coverage gap"). These tests
    //! verify the atomic transitions directly, which is the architectural
    //! correctness property we need: an in-flight H2 connection must not be
    //! evicted by the retain predicate, and the connection's `idle_at` must
    //! be stamped only on the `active_streams` 1 → 0 transition.
    use super::*;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::Service as _;

    /// A minimal `Service<()>` that returns `Ok(())` (stand-in for
    /// `Singled::call`). We only care about the wrapper's stream-count
    /// side effects.
    #[derive(Clone, Default)]
    struct OkService;
    impl Service<()> for OkService {
        type Response = ();
        type Error = BoxError;
        type Future = std::pin::Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, _: ()) -> Self::Future {
            Box::pin(async { Ok(()) })
        }
    }

    fn publish_state(h2_ref: &H2ConnectionRef) -> (Arc<AtomicUsize>, Arc<Mutex<Instant>>) {
        use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
        let active = Arc::new(AtomicUsize::new(0));
        let idle_at = Arc::new(Mutex::new(Instant::now()));
        h2_ref.publish(H2ConnectionState {
            metadata: ConnectionMetadata::builder()
                .proxied(false)
                .poison_fn(|| {})
                .build(),
            active_streams: active.clone(),
            idle_at: idle_at.clone(),
        });
        (active, idle_at)
    }

    /// A round-trip through `SingletonConnection::call` mints a
    /// `DispatchGuard` that releases on drop. Counter goes 0 → 1 → 0.
    #[tokio::test]
    async fn singleton_dispatch_round_trip_is_net_zero() {
        let h2_ref = H2ConnectionRef::new();
        let (active, _) = publish_state(&h2_ref);
        assert_eq!(active.load(Ordering::Acquire), 0);

        let mut sc = SingletonConnection::new(OkService, h2_ref);
        let _ = sc.call(()).await;
        assert_eq!(
            active.load(Ordering::Acquire),
            1,
            "call should mint a DispatchGuard, incrementing active_streams"
        );
        drop(sc);
        assert_eq!(
            active.load(Ordering::Acquire),
            0,
            "DispatchGuard's Drop should release the active_streams slot"
        );
    }

    /// Constructing without dispatching leaves no `DispatchGuard`, so
    /// dropping is a no-op. The post-checkout `poll_ready` retry loop
    /// relies on this: a checkout discarded before `call` must not
    /// underflow the counter.
    #[tokio::test]
    async fn singleton_drop_without_dispatch_is_noop() {
        let h2_ref = H2ConnectionRef::new();
        let (active, _) = publish_state(&h2_ref);
        active.store(5, Ordering::Release);

        let sc = SingletonConnection::new(OkService, h2_ref);
        drop(sc);
        assert_eq!(
            active.load(Ordering::Acquire),
            5,
            "uncalled SingletonConnection must not release a DispatchGuard"
        );
    }

    /// Concurrent dispatches against the same H2 connection (simulating
    /// multiplexed requests) all land on the same counter. `idle_at` is
    /// stamped only on the LAST `DispatchGuard` drop (the 1 → 0
    /// transition).
    #[tokio::test]
    async fn idle_at_stamped_only_on_last_dispatch_release() {
        let h2_ref = H2ConnectionRef::new();
        let (active, idle_at) = publish_state(&h2_ref);

        // Capture the stamp from construction to detect updates.
        let original_idle = *idle_at.lock().unwrap();

        let mut a = SingletonConnection::new(OkService, h2_ref.clone());
        let mut b = SingletonConnection::new(OkService, h2_ref.clone());
        let mut c = SingletonConnection::new(OkService, h2_ref);

        let _ = a.call(()).await;
        let _ = b.call(()).await;
        let _ = c.call(()).await;
        assert_eq!(active.load(Ordering::Acquire), 3);

        // Sleep so any stamp is distinguishable from construction time.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Release two; idle_at must NOT yet be stamped (counter > 0).
        drop(a);
        drop(b);
        assert_eq!(active.load(Ordering::Acquire), 1);
        assert_eq!(
            *idle_at.lock().unwrap(),
            original_idle,
            "idle_at must not be stamped while dispatches are in flight"
        );

        // Last release: 1 → 0 transition. idle_at must be stamped.
        drop(c);
        assert_eq!(active.load(Ordering::Acquire), 0);
        let final_idle = *idle_at.lock().unwrap();
        assert!(
            final_idle > original_idle,
            "idle_at should be stamped when the last DispatchGuard releases"
        );
    }
}

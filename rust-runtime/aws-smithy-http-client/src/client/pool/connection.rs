/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection state tracking for pooled connections.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
use aws_smithy_types::body::SdkBody;
use pin_project_lite::pin_project;
use tokio::sync::OwnedSemaphorePermit;
use tower::Service;

use super::cache;
use super::handshake::H1SendRequest;
use super::BoxError;

/// A duration paired with the sleep implementation used to realize it.
///
/// This type makes "timeout without sleep impl" unrepresentable — you cannot
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
    /// means no connect timeout — cached connections skip the connector
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
/// + response-headers-wait only — does NOT include pool acquire or connect
/// establishment (those have their own timeouts).
#[derive(Clone, Debug)]
pub(crate) struct ReadTimeoutHint(pub(crate) TimeoutContext);

/// Permits acquired from connection limit semaphores.
/// Held for the lifetime of the connection — dropped when the connection is dropped.
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
/// Captured between TLS connector output and protocol handshake — the last point
/// where the raw transport stream is accessible.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionInfo {
    /// Remote address of the peer. `None` when the underlying connector did
    /// not attach `HttpInfo` (e.g. the s2n-tls provider today).
    pub(crate) remote_addr: Option<SocketAddr>,
    /// Local address of this end of the connection.
    pub(crate) local_addr: Option<SocketAddr>,
    // Future: tls_info, timing (tcp_connect_duration, tls_handshake_duration)
}

/// A one-shot "this connection is dead, don't reuse it" flag.
///
/// Shared via `Arc` — all clones observe and control the same flag.
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
    info: ConnectionInfo,
    created_at: Instant,
    poison: PoisonPill,
    _permit: Arc<ConnectionPermit>,
}

impl<S> ManagedConnection<S> {
    /// Create a new managed connection wrapping the given service.
    pub(crate) fn new(inner: S, info: ConnectionInfo, permit: Arc<ConnectionPermit>) -> Self {
        Self {
            inner,
            info,
            created_at: Instant::now(),
            poison: PoisonPill::healthy(),
            _permit: permit,
        }
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

    /// Mutable access to the inner service.
    pub(crate) fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Build a smithy `ConnectionMetadata` for this connection.
    ///
    /// The returned metadata captures a clone of the `PoisonPill`, so
    /// calling `ConnectionMetadata::poison()` flips this connection's
    /// poison flag — the same flag the pool checks on checkout/return.
    /// Address fields are copied.
    pub(crate) fn metadata(&self) -> ConnectionMetadata {
        let poison = self.poison.clone();
        let mut builder = ConnectionMetadata::builder()
            // Hardcoded `false`: v2 does not yet plumb proxy configuration
            // through the pool stack. When proxy support lands,
            // `ConnectionInfo` should carry an `is_proxied` flag populated
            // at connection establishment and passed through here.
            .proxied(false)
            .poison_fn(move || poison.poison());
        builder
            .set_remote_addr(self.info.remote_addr)
            .set_local_addr(self.info.local_addr);
        builder.build()
    }
}

impl<S: Clone> Clone for ManagedConnection<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            info: self.info.clone(),
            created_at: self.created_at,
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
}

impl<S> CachedConnection<S> {
    pub(crate) fn new(cached: cache::Cached<ManagedConnection<S>>) -> Self {
        Self {
            inner: Some(cached),
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
            if cached.inner().is_poisoned() {
                cached.discard();
            }
            // else: cached drops normally, returning the connection to the cache
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
/// opaque to us — we can't reach through it to the underlying
/// `ManagedConnection` to build its metadata at checkout time. The
/// handshake side has direct access, so it publishes and the checkout
/// side reads.
///
/// Last-writer-wins is correct. Singleton holds at most one live H2
/// connection per host at a time: on `Singled::poll_ready` error (or a
/// `Singleton::retain` predicate returning false), its state transitions
/// from `Made(svc)` back to `Empty`, dropping the old service. The next
/// request's `call` runs a fresh handshake through `H2ConnectAndHandshake`,
/// which publishes new metadata — overwriting whatever was there before.
/// Clients that grabbed metadata for the previous connection retain a
/// `ConnectionMetadata` pointing at the dropped connection's `PoisonPill`;
/// poisoning it is a no-op because the connection it described is already
/// gone.
#[derive(Clone, Default)]
pub(crate) struct H2ConnectionRef {
    inner: Arc<std::sync::Mutex<Option<ConnectionMetadata>>>,
}

impl H2ConnectionRef {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn publish(&self, metadata: ConnectionMetadata) {
        *self.inner.lock().unwrap() = Some(metadata);
    }

    pub(crate) fn current(&self) -> Option<ConnectionMetadata> {
        self.inner.lock().unwrap().clone()
    }
}

/// A connection checked out from the H2 singleton.
///
/// Symmetric wrapper to `CachedConnection` for the H2 leg. The inner type
/// is parameterized because hyper-util's `Singled<S>` is unnameable outside
/// its crate; we accept whatever type comes out of the singleton. Gives us
/// a named, pool-owned H2 handle we can hold as a guard, inspect, and
/// instrument later without vendoring `singleton.rs`.
///
/// No Drop hook: dropping a Singled clone does not affect singleton state;
/// dead/poisoned H2 connections are handled reactively via
/// `Singled::poll_ready` clearing the state to `Empty`.
///
/// `h2_ref` is a clone of the same [`H2ConnectionRef`] that the handshake
/// service writes to — it lets `SingletonConnection::metadata` expose the
/// current H2 connection's metadata to the adapter even though `Singled<…>`
/// itself is opaque.
pub(crate) struct SingletonConnection<T> {
    inner: T,
    h2_ref: H2ConnectionRef,
}

impl<T> SingletonConnection<T> {
    pub(crate) fn new(inner: T, h2_ref: H2ConnectionRef) -> Self {
        Self { inner, h2_ref }
    }

    /// Metadata for the current H2 connection, if a handshake has completed.
    ///
    /// Returns `None` before the first successful H2 handshake for this host.
    pub(crate) fn metadata(&self) -> Option<ConnectionMetadata> {
        self.h2_ref.current()
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
    /// the same underlying HTTP connection. For H1 this is critical — if the
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
/// Carries `GuardedBody<PoolUnnameable>` because some pool-internal leg
/// holds a checkout guard whose type is unnameable outside hyper-util
/// (today: the H2 leg's `Singled<...>`). `Negotiate` requires uniform
/// response types across legs; this alias is the uniform type that
/// consumers above the Negotiate composition point work with.
pub(crate) type CheckoutResponse<PoolUnnameable> = http_1x::Response<GuardedBody<PoolUnnameable>>;

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection state tracking for pooled connections.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use aws_smithy_types::body::SdkBody;
use pin_project_lite::pin_project;
use tower::Service;

use super::cache;
use super::handshake::H1SendRequest;
use super::BoxError;

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

/// A connection with SDK-owned lifecycle metadata.
///
/// Wraps the inner service (typically `SendRequest<SdkBody>`) with state needed
/// for pool management: idle tracking, poisoning, and connection identity.
///
/// Clone is supported when the inner service is Clone (e.g., HTTP/2 multiplexed
/// connections). Clones share the same poison flag and connection metadata.
pub(crate) struct ManagedConnection<S> {
    inner: S,
    info: ConnectionInfo,
    created_at: Instant,
    last_used: Instant,
    poisoned: Arc<AtomicBool>,
}

impl<S> ManagedConnection<S> {
    /// Create a new managed connection wrapping the given service.
    pub(crate) fn new(inner: S, info: ConnectionInfo) -> Self {
        let now = Instant::now();
        Self {
            inner,
            info,
            created_at: now,
            last_used: now,
            poisoned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark this connection as poisoned so it will not be reused.
    ///
    /// Poisoned connections are skipped on checkout and dropped on return to the pool.
    pub(crate) fn poison(&self) {
        self.poisoned.store(true, Ordering::Release);
    }

    /// Returns a closure that poisons this connection when called.
    ///
    /// The returned closure can be passed to components that need to signal
    /// connection failure without holding a direct reference to the connection.
    pub(crate) fn poison_fn(&self) -> impl Fn() + Send + Sync + 'static {
        let flag = self.poisoned.clone();
        move || flag.store(true, Ordering::Release)
    }

    /// Whether this connection has been poisoned.
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Acquire)
    }

    /// How long this connection has been idle (since last request completed).
    pub(crate) fn idle_duration(&self) -> std::time::Duration {
        self.last_used.elapsed()
    }

    /// Connection metadata.
    pub(crate) fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    /// When this connection was established.
    pub(crate) fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Mark this connection as recently used.
    pub(crate) fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    /// Mutable access to the inner service.
    pub(crate) fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S: Clone> Clone for ManagedConnection<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            info: self.info.clone(),
            created_at: self.created_at,
            last_used: self.last_used,
            poisoned: self.poisoned.clone(),
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
        self.touch();
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
pub(crate) struct SingletonConnection<T> {
    inner: T,
}

impl<T> SingletonConnection<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self { inner }
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

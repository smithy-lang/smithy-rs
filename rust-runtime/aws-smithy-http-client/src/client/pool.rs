/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool for the v2 HTTP client.
//!
//! Routes requests by `(scheme, authority)` to per-host pool stacks,
//! negotiates HTTP/1.1 vs HTTP/2 via ALPN, caches H1 connections for
//! reuse, multiplexes over a singleton H2 connection per host, enforces
//! total and per-host connection limits, and surfaces connection
//! metadata (remote/local address, poison handle) to the adapter layer
//! for the runtime's connection-poisoning interceptor.

mod connection;
mod handshake;
mod vendored_cache;

/// Connection-caching pool layer.
///
/// Re-exports [`vendored_cache`], our vendored copy of hyper-util's
/// `pool::cache` with SDK-specific modifications. The re-export insulates
/// callers from the vendoring detail; all pool code uses `cache::…`
/// regardless of where the implementation lives.
mod cache {
    pub(crate) use super::vendored_cache::*;
}

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::task::Poll;
use std::time::{Duration, Instant};

use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::Connection as HyperConnection;
use hyper_util::client::pool as hpool;
use hyper_util::rt::TokioExecutor;
use tokio::sync::{oneshot, Semaphore};
use tower::{Service, ServiceExt};

use connection::{
    CachedConnection, CheckoutResponse, ConnectionGuard, GuardedBody, H2ConnectionRef,
    SingletonConnection,
};
pub(crate) use connection::{ConnectCtx, ReadTimeoutHint, TimeoutContext};
use handshake::{H1ConnectAndHandshake, H1SendRequest, H2ConnectAndHandshake};

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Request-extension slot the pool checkout fills in with the
/// `ConnectionMetadata` for the selected connection.
///
/// Read later by the adapter's `CaptureSmithyConnection` retriever, which
/// is what `ConnectionPoisoningInterceptor` uses to decide whether to call
/// `ConnectionMetadata::poison()` on a transient error. Poisoning flips the
/// shared `PoisonPill` on the actual `ManagedConnection`, so the pool
/// skips it on checkout and drops it on return.
///
/// Write-once per request: `H{1,2}Checkout::call` sets it exactly once
/// during checkout. Subsequent sets are no-ops (the `OnceLock` guarantees
/// single-init). Retries produce fresh `HttpConnector::call` invocations
/// which create fresh capture slots; each attempt's metadata points at
/// the connection used for that attempt.
#[derive(Clone, Default)]
pub(crate) struct ConnectionMetadataCapture {
    slot: Arc<std::sync::OnceLock<aws_smithy_runtime_api::client::connection::ConnectionMetadata>>,
}

impl ConnectionMetadataCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set(
        &self,
        metadata: aws_smithy_runtime_api::client::connection::ConnectionMetadata,
    ) {
        // Silently ignore duplicate sets: single-set is the contract, extra
        // sets would only happen via pool-internal bugs.
        let _ = self.slot.set(metadata);
    }

    pub(crate) fn get(
        &self,
    ) -> Option<aws_smithy_runtime_api::client::connection::ConnectionMetadata> {
        self.slot.get().cloned()
    }
}

/// Pool-level configuration.
///
/// Plumbed from the eventual `Builder::new_v2()` public API through
/// `build_pool` to `ConnectionPool`. Internal type: the public surface
/// is the builder's per-knob setters, not this struct.
///
/// All fields are `None` by default (defaults applied by the pool where
/// they take effect, not here). Fields are `pub(crate)` because
/// consumers of this struct are all within the pool module; accessor
/// methods would add noise without benefit.
#[derive(Clone, Debug, Default)]
pub(crate) struct PoolConfig {
    /// Upper bound on concurrent connections (total, across all hosts).
    /// Enforced via semaphore at the connection establishment layer.
    /// `None` = unlimited.
    pub(crate) max_connections: Option<usize>,

    /// Upper bound on concurrent connections per host.
    /// Each unique (scheme, authority) pair gets an independent semaphore.
    /// `None` = unlimited.
    pub(crate) max_connections_per_host: Option<usize>,

    /// How long an idle connection may stay in the pool before being
    /// evicted. `None` = no eviction (hyper-util's default behavior).
    pub(crate) pool_idle_timeout: Option<std::time::Duration>,
}

/// Key for per-host connection pool routing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PoolKey {
    scheme: http_1x::uri::Scheme,
    authority: http_1x::uri::Authority,
}

impl PoolKey {
    fn from_uri(uri: &http_1x::Uri) -> Option<Self> {
        Some(Self {
            scheme: uri.scheme()?.clone(),
            authority: uri.authority()?.clone(),
        })
    }
}

/// Type-erased handle to one Negotiate leg's eviction interface.
///
/// Each `TypedPoolEntry` holds at most two of these (one for its H1 cache,
/// one for its H2 singleton). The two underlying types (`Cache<…>` and
/// `Singleton<…>`) carry upstream-unnameable parameters from `Negotiate`,
/// so they're captured inside `Box<dyn Fn>` closures. The closures are
/// `Fn` (so multiple callers can invoke through `Arc<dyn Fn>`); interior
/// mutability lives in a `std::sync::Mutex` over a clone of the leg, which
/// is uncontended in practice; retain runs at most once per
/// `max(pool_idle_timeout, MIN_EVICTION_TICK)`, and the leg's own state
/// already serializes through its internal `Arc<Mutex<Shared>>`.
pub(crate) struct BoxedRetainer {
    retain_fn: Box<dyn Fn(Duration) + Send + Sync + 'static>,
    is_empty_fn: Box<dyn Fn() -> bool + Send + Sync + 'static>,
}

impl BoxedRetainer {
    fn retain_idle(&self, timeout: Duration) {
        (self.retain_fn)(timeout)
    }

    fn is_empty(&self) -> bool {
        (self.is_empty_fn)()
    }
}

/// Per-host retainer registry. Bounded at two entries (one H1 cache, one
/// H2 singleton); populated lazily by the H1 fallback and H2 upgrade
/// `layer_fn`s the first time `Negotiate` constructs each leg.
type RetainerSlot = Arc<Mutex<Vec<BoxedRetainer>>>;

pub(crate) fn build_pool<C, IO>(connector: C, config: PoolConfig) -> ConnectionPool
where
    C: Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Unpin + Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + HyperConnection + Unpin + Send + 'static,
{
    let global_sem = config.max_connections.map(|n| Arc::new(Semaphore::new(n)));
    let max_per_host = config.max_connections_per_host;
    let pool_hooks = handshake::PoolHooks::new();

    tracing::debug!(
        max_connections = ?config.max_connections,
        max_connections_per_host = ?config.max_connections_per_host,
        pool_idle_timeout = ?config.pool_idle_timeout,
        "v2 pool: initialized"
    );

    let make_entry = {
        let pool_hooks = pool_hooks.clone();
        move |_uri: &http_1x::Uri| -> Box<dyn PoolEntry> {
            let per_host_sem = max_per_host.map(|n| Arc::new(Semaphore::new(n)));
            let limited = handshake::ConnectionLimit::new(
                connector.clone(),
                global_sem.clone(),
                per_host_sem,
            );

            // Per-host bridge: `H2ConnectAndHandshake` publishes on each new
            // handshake; `SingletonConnection` reads the current entry at
            // checkout. Clones share the underlying slot.
            let h2_ref = H2ConnectionRef::new();

            // Bounded at two: the H1 fallback and H2 upgrade `layer_fn`s
            // each push one entry on first construction.
            let retainers: RetainerSlot = Arc::new(Mutex::new(Vec::with_capacity(2)));

            let stack = hpool::negotiate::builder()
                .connect(limited)
                .inspect(
                    |(conn, _permit): &(IO, Arc<connection::ConnectionPermit>)| {
                        conn.connected().is_negotiated_h2()
                    },
                )
                .fallback({
                    let retainers = retainers.clone();
                    let pool_hooks = pool_hooks.clone();
                    tower::layer::layer_fn(move |inspector| {
                        let cache = cache::builder()
                            .executor(TokioExecutor::new())
                            .build(H1ConnectAndHandshake::new(inspector, pool_hooks.clone()));
                        // Capture a clone of the Cache for eviction. `Cache`
                        // is Clone and shares state via its internal
                        // `Arc<Mutex<Shared>>`, so the clone here and the
                        // original-stack consumption below observe the same
                        // idle set.
                        let cache_for_retain = Arc::new(std::sync::Mutex::new(cache.clone()));
                        let cache_for_empty = cache_for_retain.clone();
                        retainers
                            .lock()
                            .expect("retainer slot poisoned")
                            .push(BoxedRetainer {
                                retain_fn: Box::new(move |timeout| {
                                    let now = Instant::now();
                                    cache_for_retain
                                        .lock()
                                        .expect("retain cache lock poisoned")
                                        .retain(|managed| {
                                            let keep = !managed.is_poisoned()
                                                && now.saturating_duration_since(managed.idle_at())
                                                    < timeout;
                                            if !keep {
                                                let reason = if managed.is_poisoned() {
                                                    "poisoned"
                                                } else {
                                                    "idle_expired"
                                                };
                                                let idle_duration = now
                                                    .saturating_duration_since(managed.idle_at());
                                                tracing::debug!(
                                                    conn_id = managed.conn_id(),
                                                    reason,
                                                    ?idle_duration,
                                                    "v2 pool: connection evicted"
                                                );
                                            }
                                            keep
                                        });
                                }),
                                is_empty_fn: Box::new(move || {
                                    cache_for_empty
                                        .lock()
                                        .expect("is_empty cache lock poisoned")
                                        .is_empty()
                                }),
                            });
                        cache.map_response(|cached| H1Checkout::new(CachedConnection::new(cached)))
                    })
                })
                .upgrade({
                    let h2_ref = h2_ref.clone();
                    let retainers = retainers.clone();
                    let pool_hooks = pool_hooks.clone();
                    tower::layer::layer_fn(move |inspected| {
                        let singleton =
                            hpool::singleton::Singleton::new(H2ConnectAndHandshake::new(
                                inspected,
                                h2_ref.clone(),
                                pool_hooks.clone(),
                            ));
                        let singleton_for_retain =
                            Arc::new(std::sync::Mutex::new(singleton.clone()));
                        let singleton_for_empty = singleton_for_retain.clone();
                        retainers
                            .lock()
                            .expect("retainer slot poisoned")
                            .push(BoxedRetainer {
                                retain_fn: Box::new(move |timeout| {
                                    let now = Instant::now();
                                    singleton_for_retain
                                        .lock()
                                        .expect("retain singleton lock poisoned")
                                        .retain(|managed| {
                                            // For H2 the connection stays in
                                            // Singleton while serving streams, so
                                            // `idle_at` alone is not a valid
                                            // idleness signal. Keep the
                                            // connection if any stream is in
                                            // flight; only evict when truly idle
                                            // AND `idle_at` has exceeded the
                                            // timeout (stamped on the 1 → 0
                                            // transition by
                                            // `SingletonConnection::drop`).
                                            let keep = !managed.is_poisoned()
                                                && (managed.active_streams_count() > 0
                                                    || now.saturating_duration_since(
                                                        managed.idle_at(),
                                                    ) < timeout);
                                            if !keep {
                                                let reason = if managed.is_poisoned() {
                                                    "poisoned"
                                                } else {
                                                    "idle_expired"
                                                };
                                                let idle_duration = now
                                                    .saturating_duration_since(managed.idle_at());
                                                tracing::debug!(
                                                    conn_id = managed.conn_id(),
                                                    reason,
                                                    ?idle_duration,
                                                    "v2 pool: connection evicted"
                                                );
                                            }
                                            keep
                                        });
                                }),
                                is_empty_fn: Box::new(move || {
                                    singleton_for_empty
                                        .lock()
                                        .expect("is_empty singleton lock poisoned")
                                        .is_empty()
                                }),
                            });
                        singleton.map_response({
                            let h2_ref = h2_ref.clone();
                            move |singled| {
                                H2Checkout::new(SingletonConnection::new(singled, h2_ref.clone()))
                            }
                        })
                    })
                })
                .build();
            Box::new(TypedPoolEntry { stack, retainers })
        }
    };

    ConnectionPool {
        config,
        hosts: Mutex::new(HashMap::new()),
        make_entry: Box::new(make_entry),
        eviction_spawned: AtomicBool::new(false),
        drop_notifier: OnceLock::new(),
    }
}

/// The connection pool.
///
/// Routes requests by (scheme, authority) to per-host pool stacks.
/// Each host gets a Negotiate stack that selects between HTTP/1.1 (Cache)
/// and HTTP/2 (Singleton) based on ALPN negotiation.
pub(crate) struct ConnectionPool {
    /// Pool-wide configuration.
    config: PoolConfig,
    hosts: Mutex<HashMap<PoolKey, Box<dyn PoolEntry>>>,
    make_entry: Box<dyn Fn(&http_1x::Uri) -> Box<dyn PoolEntry> + Send + Sync>,

    /// Latches `true` once the eviction task has been spawned. Subsequent
    /// `send_request` calls observe the latch and skip the spawn path.
    /// Read only when `config.pool_idle_timeout` is set.
    eviction_spawned: AtomicBool,

    /// Drop signal for the eviction task. The task awaits the matching
    /// `Receiver`; we never send. When `ConnectionPool` drops, this
    /// `OnceLock`'s contents drop, the sender drops, the receiver errors
    /// with `Canceled`, and the task exits its `select!` loop.
    ///
    /// `OnceLock` because the task spawns lazily on first `send_request`
    /// (at most one task per pool). The task additionally holds a
    /// `Weak<ConnectionPool>` as a fallback exit signal.
    drop_notifier: OnceLock<oneshot::Sender<Infallible>>,
}

/// Minimum eviction tick period, matching legacy hyper-util pool's
/// `MIN_CHECK` constant. Prevents very short `pool_idle_timeout`s from
/// causing the task to spin hot.
const MIN_EVICTION_TICK: Duration = Duration::from_millis(90);

impl ConnectionPool {
    /// Send a request through the pool.
    ///
    /// Routes to the appropriate per-host pool stack and sends the request.
    /// `ctx` carries the routing URI plus per-operation connect-time data
    /// (connect_timeout). Per-operation read_timeout, if any, must be
    /// attached to `req.extensions_mut()` as [`ReadTimeoutHint`] before
    /// calling; the checkout services read it from there.
    ///
    /// Takes `self: &Arc<Self>` so the lazy eviction task can hold a
    /// `Weak<Self>` as a fallback exit signal (primary exit is the drop
    /// of `drop_notifier`; the `Weak` just insulates the task against a
    /// missed drop signal).
    pub(crate) async fn send_request(
        self: &Arc<Self>,
        ctx: ConnectCtx,
        req: http_1x::Request<SdkBody>,
    ) -> Result<http_1x::Response<SdkBody>, BoxError> {
        let key =
            PoolKey::from_uri(&ctx.uri).ok_or("request URI must have scheme and authority")?;

        // Lazily spawn the idle-eviction task on first use. Matches the
        // legacy hyper-util pool's approach: no task if the pool is never
        // used, no task if `pool_idle_timeout` is `None` or zero.
        self.maybe_spawn_eviction_task();

        // Dispatch the request through the per-host entry. The lock is
        // held only long enough to look up / create the entry and call
        // `send`; all I/O happens in the returned future, outside the
        // lock.
        let fut = {
            let mut hosts = self.hosts.lock().unwrap();
            if !hosts.contains_key(&key) {
                hosts.insert(key.clone(), (self.make_entry)(&ctx.uri));
            }
            hosts.get_mut(&key).unwrap().send(ctx, req)
        };

        fut.await
    }

    /// Spawn the idle-eviction task if it hasn't been spawned yet and
    /// `pool_idle_timeout` is configured.
    ///
    /// Idempotent and cheap after the first call (single relaxed
    /// `AtomicBool` load returns early). Matches legacy
    /// `spawn_idle_interval` semantics: no-op without a timeout, no-op
    /// with a zero timeout, no-op if already spawned.
    fn maybe_spawn_eviction_task(self: &Arc<Self>) {
        let timeout = match self.config.pool_idle_timeout {
            Some(d) if d > Duration::ZERO => d,
            _ => return,
        };
        // Fast path: already spawned.
        if self.eviction_spawned.load(Ordering::Acquire) {
            return;
        }
        // Claim the spawn slot. Only one task per pool ever.
        if self
            .eviction_spawned
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let (tx, rx) = oneshot::channel::<Infallible>();
        // Invariant: this code path runs exactly once per pool; the
        // `compare_exchange` on `eviction_spawned` above is the unique
        // claim. If `drop_notifier` is already populated, that invariant
        // is broken.
        self.drop_notifier
            .set(tx)
            .expect("drop_notifier set after exclusive spawn-slot claim");
        let weak = Arc::downgrade(self);
        let tick = timeout.max(MIN_EVICTION_TICK);
        tracing::debug!(
            interval = ?tick,
            pool_idle_timeout = ?timeout,
            "v2 pool: eviction task spawned"
        );
        tokio::spawn(eviction_task(weak, rx, timeout));
    }

    /// Walk the host map, dropping idle connections that have exceeded
    /// `timeout` and removing host entries whose retainers are empty.
    ///
    /// Called by the eviction task on each tick. Safe to call when the
    /// host map is empty (no-op).
    fn retain_idle_hosts(&self, timeout: Duration) {
        let mut hosts = self.hosts.lock().unwrap();
        let before = hosts.len();
        hosts.retain(|key, entry| {
            entry.retain_idle(timeout);
            if entry.is_empty() {
                tracing::debug!(
                    authority = %key.authority,
                    scheme = %key.scheme,
                    "v2 pool: host entry removed (empty after retain)"
                );
                false
            } else {
                true
            }
        });
        let after = hosts.len();
        drop(hosts);
        tracing::trace!(
            hosts_before = before,
            hosts_after = after,
            "v2 pool: eviction tick complete"
        );
    }
}

/// Background loop that drops idle connections past `pool_idle_timeout`.
///
/// On each tick:
/// 1. If the pool has been dropped (`Weak::upgrade` returns `None`), exit.
/// 2. Otherwise, walk the host map, drop connections whose `idle_at` is
///    older than `timeout`, and remove host entries whose retainers
///    report empty.
///
/// Tick interval is `max(timeout, MIN_EVICTION_TICK)`. The floor exists
/// so a very short `pool_idle_timeout` doesn't spin the task hot.
/// Connections live on average between 1× and 2× the tick interval past
/// last use (sawtooth eviction).
///
/// Exit paths:
/// - Primary: `drop_notifier` (the `Sender<Infallible>` half) drops when
///   `ConnectionPool` drops, the receiver errors with `Canceled`, the
///   `select!` left branch fires, the task exits immediately.
/// - Secondary: `Weak::upgrade` returns `None`. Belt-and-suspenders for
///   the unlikely case that the notifier didn't fire; the task will
///   exit on the next tick.
async fn eviction_task(
    pool: std::sync::Weak<ConnectionPool>,
    mut drop_notifier: oneshot::Receiver<Infallible>,
    timeout: Duration,
) {
    let tick = timeout.max(MIN_EVICTION_TICK);
    let mut interval = tokio::time::interval(tick);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick of `tokio::time::interval` resolves immediately;
    // burn it so subsequent ticks fire at `now + tick`.
    interval.tick().await;
    loop {
        tokio::select! {
            _ = &mut drop_notifier => break,
            _ = interval.tick() => {
                match pool.upgrade() {
                    Some(pool) => pool.retain_idle_hosts(timeout),
                    None => break,
                }
            }
        }
    }
    tracing::trace!("v2 pool eviction task exiting");
}

/// Rewrite an HTTP/1.1 request's URI to the appropriate request-target
/// form for dispatch:
///
/// - `CONNECT` → authority-form (`host:port`)
/// - proxied non-CONNECT → absolute-form (full URL with scheme + authority)
/// - direct non-CONNECT → origin-form (path + query only)
///
/// Mirrors `hyper-util`'s legacy `client::Client::send_request` request
/// preparation. Required because `hyper::client::conn::http1` sends the
/// URI as-is; the form selection lives in the layer above the
/// connection.
fn rewrite_h1_request_target(req: &mut http_1x::Request<SdkBody>, is_proxied: bool) {
    use http_1x::{uri::Parts, Method, Uri};
    if req.method() == Method::CONNECT {
        // Authority-form: keep just the authority component.
        if let Some(auth) = req.uri().authority().cloned() {
            let mut parts = Parts::default();
            parts.authority = Some(auth);
            *req.uri_mut() = Uri::from_parts(parts).expect("authority is valid uri");
        }
        return;
    }
    if is_proxied {
        // Absolute-form: leave the URI as-is (scheme + authority + path).
        return;
    }
    // Origin-form: strip scheme + authority, keep path + query (defaulting
    // to "/" if the URI had nothing after the authority).
    let path_and_query = req
        .uri()
        .path_and_query()
        .filter(|p| p.as_str() != "/")
        .cloned();
    *req.uri_mut() = match path_and_query {
        Some(pq) => {
            let mut parts = Parts::default();
            parts.path_and_query = Some(pq);
            Uri::from_parts(parts).expect("path-and-query is valid uri")
        }
        None => Uri::default(),
    };
}

/// Tower `Service` wrapper for an H1 pool checkout.
///
/// Produced by the H1 fallback leg (one per checkout from the cache).
/// Its `Service::call(req)` consumes the held `CachedConnection`, runs
/// the request, and wraps the response body so the connection returns
/// to the pool only when the body is fully drained.
///
/// # Single-use
///
/// Each checkout represents one `CachedConnection`. `call` moves the
/// connection into the response future via `Option::take()`; calling
/// `call` twice panics (unreachable given our checkout-per-request flow).
///
/// The `UnusedH2Phantom` generic is a phantom type parameter: the H1 path
/// never populates the H2 variant of `ConnectionGuard` and never holds an
/// H2 checkout, but both legs must produce the same `CheckoutResponse<…>`
/// so `Negotiate` can compose them uniformly. The phantom slot here is
/// whatever type the H2 leg's `SingletonConnection` holds, resolved by
/// type inference at the pool composition site; this wrapper ignores it.
pub(crate) struct H1Checkout<UnusedH2Phantom> {
    conn: Option<CachedConnection<H1SendRequest>>,
    _marker: std::marker::PhantomData<fn() -> UnusedH2Phantom>,
}

impl<UnusedH2Phantom> H1Checkout<UnusedH2Phantom> {
    pub(crate) fn new(conn: CachedConnection<H1SendRequest>) -> Self {
        Self {
            conn: Some(conn),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<UnusedH2Phantom> Service<http_1x::Request<SdkBody>> for H1Checkout<UnusedH2Phantom> {
    type Response = CheckoutResponse<UnusedH2Phantom>;
    type Error = BoxError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn
            .as_mut()
            .expect("H1Checkout::poll_ready after call")
            .poll_ready(cx)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        let mut conn = self.conn.take().expect("H1Checkout::call called twice");
        // Populate the adapter-provided capture so the
        // `CaptureSmithyConnection` retriever can return a live
        // `ConnectionMetadata` pointing at THIS connection's poison pill.
        if let Some(capture) = req.extensions().get::<ConnectionMetadataCapture>() {
            capture.set(conn.metadata());
        }
        // Read-timeout hint: bounds request-write + response-headers only.
        let read_timeout = req.extensions().get::<ReadTimeoutHint>().cloned();
        let mut req = req;
        // HTTP/1.1 request-target form depends on whether we're talking to
        // a proxy or directly to the origin. CONNECT always uses
        // authority-form; otherwise proxied = absolute-form, direct =
        // origin-form. Matches `hyper-util`'s legacy client behavior.
        rewrite_h1_request_target(&mut req, conn.is_proxied());
        Box::pin(async move {
            let send_fut = conn.call(req);
            let resp = super::timeout::maybe_timeout_future(
                send_fut,
                read_timeout.as_ref().map(|h| h.0.duration),
                read_timeout.as_ref().map(|h| &h.0.sleep_impl),
                super::timeout::TimeoutKind::Read,
            )
            .await?;
            let (parts, body) = resp.into_parts();
            let body = GuardedBody::new(body, ConnectionGuard::H1(conn));
            Ok(http_1x::Response::from_parts(parts, body))
        })
    }
}

/// Tower `Service` wrapper for an H2 pool checkout.
///
/// Symmetric to `H1Checkout`. The `Singleton` type parameter is the
/// `SingletonConnection<T>` inner `T`, which at the composition site
/// resolves to the unnameable `hyper_util::client::pool::singleton::
/// Singled<ManagedConnection<H2SendRequest>>`. We carry it through
/// generics and never name it concretely.
///
/// # Single-use
///
/// Same contract as `H1Checkout`.
pub(crate) struct H2Checkout<Singleton> {
    conn: Option<SingletonConnection<Singleton>>,
}

impl<Singleton> H2Checkout<Singleton> {
    pub(crate) fn new(conn: SingletonConnection<Singleton>) -> Self {
        Self { conn: Some(conn) }
    }
}

impl<Singleton> Service<http_1x::Request<SdkBody>> for H2Checkout<Singleton>
where
    Singleton: Service<http_1x::Request<SdkBody>, Response = http_1x::Response<hyper::body::Incoming>>
        + Send
        + 'static,
    Singleton::Error: Into<BoxError>,
    Singleton::Future: Send + 'static,
{
    type Response = CheckoutResponse<Singleton>;
    type Error = BoxError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn
            .as_mut()
            .expect("H2Checkout::poll_ready after call")
            .poll_ready(cx)
            .map_err(Into::into)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        let mut conn = self.conn.take().expect("H2Checkout::call called twice");
        // Populate the adapter-provided capture. The metadata is published
        // by `H2ConnectAndHandshake` when a fresh H2 connection is
        // established; if a request somehow reaches us before any metadata
        // is available we leave the capture empty (poison becomes a no-op
        // for that request, matching the capture-absent default).
        if let Some(capture) = req.extensions().get::<ConnectionMetadataCapture>() {
            if let Some(metadata) = conn.metadata() {
                capture.set(metadata);
            }
        }
        // Read-timeout hint: bounds request-write + response-headers only.
        let read_timeout = req.extensions().get::<ReadTimeoutHint>().cloned();
        Box::pin(async move {
            let send_fut = conn.call(req);
            let resp = super::timeout::maybe_timeout_future(
                send_fut,
                read_timeout.as_ref().map(|h| h.0.duration),
                read_timeout.as_ref().map(|h| &h.0.sleep_impl),
                super::timeout::TimeoutKind::Read,
            )
            .await?;
            let (parts, body) = resp.into_parts();
            let body = GuardedBody::new(body, ConnectionGuard::H2(conn));
            Ok(http_1x::Response::from_parts(parts, body))
        })
    }
}

/// Type-erased per-host pool entry.
///
/// Each host has one of these, wrapping the unnameable Negotiate stack.
/// Dyn erasure is structural: hosts have different Negotiate compositions
/// with different unnameable inner types, so `ConnectionPool::hosts` can't
/// hold a concrete type.
///
/// `send` performs checkout, health filtering, request dispatch, and
/// body-guard setup atomically for a single request.
trait PoolEntry: Send + Sync {
    fn send(
        &mut self,
        ctx: ConnectCtx,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>>;

    /// Drop idle connections that have exceeded the given timeout, and
    /// drop any connection flagged as poisoned.
    ///
    /// Called by the background eviction task at each tick. Called on the
    /// `Box<dyn PoolEntry>` directly (no `&mut`) because the retainers
    /// carry their own interior mutability (see [`BoxedRetainer`]).
    fn retain_idle(&self, timeout: Duration);

    /// Whether this entry has no idle connections (H1 cache empty AND H2
    /// singleton empty). Used to drop empty host entries from the map
    /// after `retain_idle`.
    fn is_empty(&self) -> bool;
}

/// Concrete PoolEntry wrapping a Negotiate stack.
///
/// `PoolUnnameable` propagates whatever pool-internal unnameable type
/// flows through the checkout services' `CheckoutResponse`; it's carried
/// through so `Conn::Response` can name `CheckoutResponse<PoolUnnameable>`.
struct TypedPoolEntry<S> {
    stack: S,
    /// Retainers for this entry's H1 cache + H2 singleton. Populated
    /// lazily on first use of each leg by the `Negotiate` `layer_fn`s.
    /// Empty until the first request against this host; safe to iterate
    /// at any point (no leg = no-op retain, trivially empty).
    retainers: RetainerSlot,
}

impl<S, Conn, PoolUnnameable> PoolEntry for TypedPoolEntry<S>
where
    S: Service<ConnectCtx, Response = Conn> + Clone + Send + Sync + 'static,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
    Conn: Service<http_1x::Request<SdkBody>, Response = CheckoutResponse<PoolUnnameable>>
        + Send
        + 'static,
    Conn::Error: Into<BoxError>,
    Conn::Future: Send + 'static,
    PoolUnnameable: Send + Sync + 'static,
{
    fn send(
        &mut self,
        ctx: ConnectCtx,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>> {
        let mut svc = self.stack.clone();
        Box::pin(async move {
            // Checkout loop. Pops past stale idle entries (poisoned or dead).
            // Converges because:
            //   - Cache idle set is bounded; each post-checkout `poll_ready`
            //     Err flips `is_closed` so `Cached::Drop` skips reinsertion,
            //     shrinking the set by one.
            //   - Singleton clears to `Empty` on `poll_ready` Err, forcing
            //     the next `call` to run a fresh handshake; a handshake
            //     failure surfaces as an error from `svc.call` (not the
            //     inner `poll_ready`), which we propagate.
            let mut conn = loop {
                std::future::poll_fn(|cx| svc.poll_ready(cx))
                    .await
                    .map_err(Into::into)?;
                let mut checkout = svc.call(ctx.clone()).await.map_err(Into::into)?;
                if std::future::poll_fn(|cx| checkout.poll_ready(cx))
                    .await
                    .is_ok()
                {
                    break checkout;
                }
                // drop `checkout` → triggers pool cleanup (H1: discard via
                // CachedConnection::Drop; H2: Singleton already cleared
                // state).
            };

            // Dispatch the request; convert the body into `SdkBody` at the
            // boundary so consumers (and `dyn PoolEntry`) don't see our
            // internal `GuardedBody<...>` type. The guard inside
            // `GuardedBody` lives on inside `SdkBody` until the body is
            // fully drained.
            let resp = conn.call(req).await.map_err(Into::into)?;
            let (parts, body) = resp.into_parts();
            Ok(http_1x::Response::from_parts(
                parts,
                SdkBody::from_body_1_x(body),
            ))
        })
    }

    fn retain_idle(&self, timeout: Duration) {
        let retainers = self.retainers.lock().expect("retainer slot poisoned");
        for r in retainers.iter() {
            r.retain_idle(timeout);
        }
    }

    fn is_empty(&self) -> bool {
        let retainers = self.retainers.lock().expect("retainer slot poisoned");
        retainers.iter().all(|r| r.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_key() {
        let uri: http_1x::Uri = "http://example.com:8080/path".parse().unwrap();
        let key = PoolKey::from_uri(&uri).unwrap();
        assert_eq!(key.authority.as_str(), "example.com:8080");
    }

    #[test]
    fn test_pool_key_missing_scheme() {
        let uri: http_1x::Uri = "/path".parse().unwrap();
        assert!(PoolKey::from_uri(&uri).is_none());
    }

    /// A `PoolEntry` that panics if dispatched through and reports itself
    /// empty. Used as a host entry for eviction-lifecycle unit tests where
    /// no request is actually sent.
    struct NullPoolEntry;
    impl PoolEntry for NullPoolEntry {
        fn send(
            &mut self,
            _ctx: ConnectCtx,
            _req: http_1x::Request<SdkBody>,
        ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>> {
            unreachable!("NullPoolEntry::send called in a test")
        }
        fn retain_idle(&self, _timeout: Duration) {}
        fn is_empty(&self) -> bool {
            true
        }
    }

    fn pool_with_config(config: PoolConfig) -> Arc<ConnectionPool> {
        Arc::new(ConnectionPool {
            config,
            hosts: Mutex::new(HashMap::new()),
            make_entry: Box::new(|_| Box::new(NullPoolEntry)),
            eviction_spawned: AtomicBool::new(false),
            drop_notifier: OnceLock::new(),
        })
    }

    /// Without a `pool_idle_timeout`, `maybe_spawn_eviction_task` is a no-op.
    /// Matches legacy `spawn_idle_interval` which also skips without a
    /// timeout.
    #[tokio::test]
    async fn eviction_task_no_spawn_without_timeout() {
        let pool = pool_with_config(PoolConfig {
            pool_idle_timeout: None,
            ..PoolConfig::default()
        });
        pool.maybe_spawn_eviction_task();
        assert!(!pool.eviction_spawned.load(Ordering::Acquire));
        assert!(pool.drop_notifier.get().is_none());
    }

    /// A zero `pool_idle_timeout` is treated the same as `None`. Matches
    /// legacy `spawn_idle_interval` (returns early on `Duration::ZERO`).
    #[tokio::test]
    async fn eviction_task_no_spawn_with_zero_timeout() {
        let pool = pool_with_config(PoolConfig {
            pool_idle_timeout: Some(Duration::ZERO),
            ..PoolConfig::default()
        });
        pool.maybe_spawn_eviction_task();
        assert!(!pool.eviction_spawned.load(Ordering::Acquire));
    }

    /// Multiple spawn calls only spawn a single task. The second call
    /// observes `eviction_spawned = true` and returns without touching
    /// `drop_notifier`.
    #[tokio::test]
    async fn eviction_task_spawn_is_idempotent() {
        let pool = pool_with_config(PoolConfig {
            pool_idle_timeout: Some(Duration::from_millis(100)),
            ..PoolConfig::default()
        });
        pool.maybe_spawn_eviction_task();
        assert!(pool.eviction_spawned.load(Ordering::Acquire));
        // drop_notifier gets set once. Grab a pointer to it so we can
        // confirm the second call didn't replace it.
        let first_notifier = pool.drop_notifier.get().expect("notifier set");
        let first_ptr = first_notifier as *const _;
        pool.maybe_spawn_eviction_task();
        let second_notifier = pool.drop_notifier.get().expect("notifier still set");
        let second_ptr = second_notifier as *const _;
        assert_eq!(
            first_ptr, second_ptr,
            "drop_notifier should not be replaced on repeat spawn"
        );
    }

    /// `retain_idle_hosts` drops host entries whose retainers report empty.
    /// Uses `NullPoolEntry` whose `is_empty` returns `true`, so the first
    /// tick should remove every entry.
    #[tokio::test]
    async fn retain_idle_hosts_drops_empty_entries() {
        let pool = pool_with_config(PoolConfig::default());
        let uri: http_1x::Uri = "http://example.com".parse().unwrap();
        let key = PoolKey::from_uri(&uri).unwrap();
        pool.hosts
            .lock()
            .unwrap()
            .insert(key.clone(), Box::new(NullPoolEntry));
        assert_eq!(pool.hosts.lock().unwrap().len(), 1);
        pool.retain_idle_hosts(Duration::from_secs(30));
        assert_eq!(
            pool.hosts.lock().unwrap().len(),
            0,
            "empty entry should have been evicted"
        );
    }
}

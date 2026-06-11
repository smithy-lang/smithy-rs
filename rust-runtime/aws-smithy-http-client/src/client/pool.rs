/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP connection pool.
//!
//! Pool ownership and partition topology are declared at build time:
//!
//! - [`SharedPool`] owns connection lifecycle: TLS, DNS resolution,
//!   connection limits, idle eviction, proxy routing, event listening,
//!   and the partition registry. Built via [`SharedPool::builder`] which
//!   returns a [`Builder`].
//! - [`Client`] is a per-partition view over a [`SharedPool`] that
//!   implements [`HttpClient`]. Multiple [`Client`]s may share one
//!   [`SharedPool`], each targeting a distinct declared partition.
//!
//! For partition semantics, topologies, and cross-partition checkout
//! policy, see the [`partition`] module.
//!
//! # Example
//!
//! ```no_run
//! # #[cfg(feature = "rustls-aws-lc")]
//! # {
//! use aws_smithy_http_client::pool::{Client, SharedPool};
//! use aws_smithy_http_client::tls;
//! use std::time::Duration;
//!
//! let pool = SharedPool::builder()
//!     .tls_provider(tls::Provider::Rustls(
//!         tls::rustls_provider::CryptoMode::AwsLc,
//!     ))
//!     .max_connections(125)
//!     .pool_idle_timeout(Duration::from_secs(20))
//!     .build_https();
//! let client = Client::new(&pool);
//! # }
//! ```
//!
//! [`HttpClient`]: aws_smithy_runtime_api::client::http::HttpClient

pub(crate) mod connection;
mod handshake;
pub(crate) mod stats;
mod vendored_cache;

pub mod builder;
pub mod client;
pub mod partition;

// Public re-exports.
pub use builder::Builder;
pub use client::Client;
pub use connection::{
    Authority, CloseReason, ConnectionClosedEvent, ConnectionCreatedEvent, ConnectionEventListener,
    ConnectionFailedEvent, ConnectionReusedEvent, ConnectionTiming, NegotiatedProtocol,
};
pub use partition::{
    CrossPartitionPolicy, DriverSpawner, Partition, PartitionId, TokioDriverSpawner,
};
pub use stats::{AuthorityStats, PartitionStats};

pub(crate) use stats::{ConnectionCounters, StatsIndex};

/// Connection-caching pool layer.
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

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::Connection as HyperConnection;
use hyper_util::client::pool as hpool;
use hyper_util::client::proxy::matcher::Matcher as ProxyMatcher;
use hyper_util::rt::TokioExecutor;
use tokio::sync::{oneshot, Semaphore};
use tower::{Service, ServiceExt};

use connection::{
    CachedConnection, CheckoutResponse, ConnectionGuard, GuardedBody, H2ConnectionRef,
    SingletonConnection,
};
pub(crate) use connection::{ConnectCtx, ReadTimeoutHint, TimeoutContext};
use handshake::{H1ConnectAndHandshake, H1SendRequest, H2ConnectAndHandshake};

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
    slot: Arc<std::sync::OnceLock<ConnectionMetadata>>,
}

impl ConnectionMetadataCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set(&self, metadata: ConnectionMetadata) {
        // Silently ignore duplicate sets: single-set is the contract, extra
        // sets would only happen via pool-internal bugs.
        let _ = self.slot.set(metadata);
    }

    pub(crate) fn get(&self) -> Option<ConnectionMetadata> {
        self.slot.get().cloned()
    }
}

/// Pool-level configuration.
///
/// Defaults are applied at the point each setting takes effect rather
/// than in this struct.
#[derive(Clone, Default)]
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

    /// Optional listener for connection lifecycle events.
    pub(crate) connection_event_listener: Option<Arc<dyn connection::ConnectionEventListener>>,
}

/// The connection pool's configuration surface.
///
/// Owns the connection lifecycle (creation, caching, eviction, health
/// checking) and proxy routing decisions. Multiple [`Client`] instances
/// can reference one `SharedPool`, each presenting a different
/// per-partition view of the same underlying connections.
///
/// Construct via [`SharedPool::builder`], which returns a [`Builder`].
/// Cloning is cheap (shared via `Arc`).
#[derive(Clone, Debug)]
pub struct SharedPool {
    pub(crate) inner: Arc<SharedPoolInner>,
}

pub(crate) struct SharedPoolInner {
    pub(crate) pool: Arc<ConnectionPool>,
    pub(crate) proxy_matcher: Option<Arc<ProxyMatcher>>,
}

impl std::fmt::Debug for SharedPoolInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedPoolInner").finish_non_exhaustive()
    }
}

impl SharedPool {
    /// Create a [`Builder`] for configuring a new connection pool.
    pub fn builder() -> Builder<super::TlsUnset> {
        Builder::default()
    }

    /// Point-in-time, per-partition snapshot of connection counts for `authority`.
    ///
    /// Relaxed atomics; values may be stale. Sparse — only partitions that have
    /// opened a connection to this authority appear.
    ///
    /// The `authority` must match the form the pool keys on (see
    /// [`Authority::from_host`]); a value that matches no keyed authority
    /// yields empty stats.
    ///
    /// [`Authority::from_host`]: crate::client::pool::Authority::from_host
    pub fn stats(&self, authority: &Authority) -> stats::AuthorityStats {
        self.inner.pool.shared.stats_index().snapshot(authority)
    }
}

/// Key for per-host connection pool routing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PoolKey {
    scheme: http_1x::uri::Scheme,
    authority: http_1x::uri::Authority,
}

impl PoolKey {
    pub(crate) fn from_uri(uri: &http_1x::Uri) -> Option<Self> {
        Some(Self {
            scheme: uri.scheme()?.clone(),
            authority: uri.authority()?.clone(),
        })
    }
}

/// Type-erased, single-use handle to a checked-out connection that can
/// dispatch one request.
///
/// A borrowed peer connection crosses the peer's `dyn PoolEntry` boundary
/// as one of these: the concrete checkout type (`H1Checkout`, holding the
/// peer's `CachedConnection`) carries upstream-unnameable parameters, so
/// it is boxed. Dispatching consumes the handle; the response body holds
/// the underlying connection alive and returns it to *the peer's* pool on
/// drop (it was never the borrower's). Boxed only on the actual-borrow
/// path — the common local path dispatches through the concrete checkout
/// with no erasure.
pub(crate) trait DispatchConn: Send {
    /// Liveness check before dispatch. `Err` means the connection is dead;
    /// the caller drops the handle and falls back.
    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), BoxError>>;

    /// Dispatch one request, erasing the response body to `SdkBody`.
    fn dispatch(
        self: Box<Self>,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>>;
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
    /// Pop one idle connection and drop it to free its permit (active
    /// reclaim). Returns `true` if a connection was freed. The H1 cache
    /// leg pops from its idle Vec; the H2 singleton leg has no
    /// simply-reclaimable idle (an idle H2 connection still multiplexes),
    /// so its `reclaim_fn` is a no-op returning `false`.
    reclaim_fn: Box<dyn Fn() -> bool + Send + Sync + 'static>,
    /// Take one idle connection wrapped as a dispatchable handle that
    /// returns to *this* (the owner's) pool on drop — cross-partition
    /// borrow. Returns `None` if no idle connection is available. The H1
    /// cache leg checks out an idle connection; the H2 singleton leg is a
    /// no-op returning `None` (H2 borrow is not supported — an idle H2
    /// connection still multiplexes on its owner).
    borrow_fn: Box<dyn Fn() -> Option<Box<dyn DispatchConn>> + Send + Sync + 'static>,
}

impl BoxedRetainer {
    fn retain_idle(&self, timeout: Duration) {
        (self.retain_fn)(timeout)
    }

    fn is_empty(&self) -> bool {
        (self.is_empty_fn)()
    }

    fn reclaim_one(&self) -> bool {
        (self.reclaim_fn)()
    }

    fn borrow_one(&self) -> Option<Box<dyn DispatchConn>> {
        (self.borrow_fn)()
    }
}

/// Per-host retainer registry. Bounded at two entries (one H1 cache, one
/// H2 singleton); populated lazily by the H1 fallback and H2 upgrade
/// `layer_fn`s the first time `Negotiate` constructs each leg.
type RetainerSlot = Arc<Mutex<Vec<BoxedRetainer>>>;

/// Per-partition stack factory: builds a host's Negotiate(Cache,Singleton)
/// entry on first touch. Captures the partition's connector; the shared
/// budget/hooks arrive via `&SharedPoolState` at call time.
pub(crate) type MakeStack =
    Arc<dyn Fn(&http_1x::Uri, &SharedPoolState) -> Box<dyn PoolEntry> + Send + Sync>;

/// Which semaphore bound a connect attempt at the cap, identifying what a
/// reclaim must free to relieve it. Acquire order is per-host then global
/// so a per-host failure yields `PerHost`, and a failure on the
/// global semaphore while already holding the per-host permit yields
/// `Global`.
#[derive(Debug, Clone)]
pub(crate) enum BindingConstraint {
    /// The global `max_connections` semaphore is exhausted. Any
    /// over-supplied peer's idle connection frees a fungible permit.
    Global,
    /// The per-host `max_connections_per_host` semaphore for this key is
    /// exhausted. Only a peer's idle connection *to the same host* frees
    /// the relevant permit.
    PerHost(PoolKey),
}

/// Handle for cross-partition active reclaim, held by `ConnectionLimit`.
///
/// At a cap-bound connect, the requesting partition uses this to free one
/// over-supplied peer's idle connection (dropping it returns its permit to
/// the bound semaphore) before blocking-acquiring. Connection-shaped and
/// NIC-blind: the freed *permit* is what matters; P0 then connects on its
/// own NIC. Candidates are narrowed by the (advisory) stats index and
/// confirmed by the authoritative cache pop.
#[derive(Clone)]
pub(crate) struct PeerReclaimHandle {
    /// `Weak` to avoid a cycle: the registry transitively owns the
    /// partitions whose `ConnectionLimit`s hold this handle.
    registry: std::sync::Weak<partition::PartitionRegistry>,
    /// Owned (Arc) so the handle does not borrow `SharedPoolState`.
    stats_index: Arc<StatsIndex>,
    /// The requesting partition — excluded from its own candidate walk.
    self_partition: PartitionId,
}

impl PeerReclaimHandle {
    /// Free one over-supplied peer's idle connection to relieve
    /// `constraint`, returning `true` if a permit was freed. Best-effort:
    /// `false` if the pool is gone, there are no NIC-group peers, or no
    /// peer holds reclaimable idle (P0 then just blocks on the permit).
    pub(crate) fn try_free_under_load(&self, constraint: &BindingConstraint) -> bool {
        let registry = match self.registry.upgrade() {
            Some(r) => r,
            None => return false, // pool dropped → nothing to reclaim
        };
        let peers = registry.nic_group_peers(self.self_partition);
        if peers.is_empty() {
            return false; // alone in the NIC group (e.g. single-partition default)
        }
        match constraint {
            BindingConstraint::PerHost(key) => {
                // Candidates = peers with idle to this authority (index
                // narrows; cache pop confirms). Round-robin start offset.
                let authority = Authority::new(key.authority.as_str());
                let mut candidates: Vec<PartitionId> = self
                    .stats_index
                    .idle_partitions_for(&authority)
                    .into_iter()
                    .map(|(p, _idle)| p)
                    .filter(|p| *p != self.self_partition && peers.contains(p))
                    .collect();
                self.rotate(&mut candidates);
                candidates
                    .into_iter()
                    .any(|peer| registry.try_reclaim_on(peer, key))
            }
            BindingConstraint::Global => {
                // Fungible permit: any same-NIC-group peer's idle (any
                // authority) relieves the global cap. Narrow to peers that
                // the index shows holding idle.
                let mut candidates: Vec<PartitionId> = self
                    .stats_index
                    .idle_cells()
                    .into_iter()
                    .map(|(_authority, p)| p)
                    .filter(|p| *p != self.self_partition && peers.contains(p))
                    .collect();
                candidates.sort_unstable_by_key(|p| p.as_u64());
                candidates.dedup();
                self.rotate(&mut candidates);
                candidates
                    .into_iter()
                    .any(|peer| registry.try_reclaim_any(peer))
            }
        }
    }

    /// Rotate the candidate vec by the partition's advisory `peer_cursor`,
    /// so concurrent reclaims from this partition do not all probe the
    /// lowest-numbered candidate first. No-op if the registry or partition
    /// is gone, or the candidate set is empty.
    fn rotate(&self, candidates: &mut [PartitionId]) {
        if candidates.len() < 2 {
            return;
        }
        if let Some(registry) = self.registry.upgrade() {
            if let Some(state) = registry.partition_opt(self.self_partition) {
                let n = candidates.len();
                let start = state
                    .peer_cursor
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    % n;
                candidates.rotate_left(start);
            }
        }
    }
}

/// Handle for cross-partition borrow (`PreferLocal`), held by the pool
/// entry under a binding cap.
///
/// At a cap-bound local checkout (`AcquireMode::NonBlocking` returned
/// `CapBound`), the requesting partition uses this to borrow one peer's
/// idle connection and dispatch its request through it — no permit moves,
/// no cold start. Response-shaped and NIC-*bounded*: the borrowed
/// connection physically lives on the peer's NIC, so candidates are drawn
/// only from the same NIC group (unlike reclaim, which frees a fungible
/// permit and is NIC-blind). Always keyed to the requested authority (the
/// borrowed connection must already be connected to that host).
#[derive(Clone)]
pub(crate) struct PeerBorrowHandle {
    /// `Weak` to avoid a cycle: the registry transitively owns the
    /// partitions whose entries hold this handle.
    registry: std::sync::Weak<partition::PartitionRegistry>,
    /// Owned (Arc) so the handle does not borrow `SharedPoolState`.
    stats_index: Arc<StatsIndex>,
    /// The requesting partition — excluded from its own candidate walk.
    self_partition: PartitionId,
}

impl PeerBorrowHandle {
    /// Borrow one same-NIC-group peer's idle connection to `key`'s
    /// authority, as a dispatchable handle. `None` if the pool is gone,
    /// there are no NIC-group peers, or no peer holds idle to this
    /// authority (the caller then falls back to a blocking local acquire).
    pub(crate) fn try_borrow(&self, key: &PoolKey) -> Option<Box<dyn DispatchConn>> {
        let registry = self.registry.upgrade()?;
        let peers = registry.nic_group_peers(self.self_partition);
        if peers.is_empty() {
            return None; // alone in the NIC group (e.g. single-partition default)
        }
        // Candidates = peers with idle to this authority (index narrows;
        // the cache checkout confirms). Round-robin start offset.
        let authority = Authority::new(key.authority.as_str());
        let mut candidates: Vec<PartitionId> = self
            .stats_index
            .idle_partitions_for(&authority)
            .into_iter()
            .map(|(p, _idle)| p)
            .filter(|p| *p != self.self_partition && peers.contains(p))
            .collect();
        self.rotate(&mut candidates);
        candidates
            .into_iter()
            .find_map(|peer| registry.try_borrow_on(peer, key))
    }

    /// Rotate the candidate vec by the partition's advisory `peer_cursor`
    /// (shared with reclaim), so concurrent borrows from this partition do
    /// not all probe the lowest-numbered candidate first.
    fn rotate(&self, candidates: &mut [PartitionId]) {
        if candidates.len() < 2 {
            return;
        }
        if let Some(registry) = self.registry.upgrade() {
            if let Some(state) = registry.partition_opt(self.self_partition) {
                let n = candidates.len();
                let start = state
                    .peer_cursor
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    % n;
                candidates.rotate_left(start);
            }
        }
    }
}

/// Connection-lifecycle machinery shared across every partition: event
/// hooks, the global connection budget, and the per-host budgets. One
/// instance per pool, held behind `Arc` on `ConnectionPool`.
pub(crate) struct SharedPoolState {
    pub(crate) hooks: handshake::PoolHooks,
    pub(crate) global_sem: Option<Arc<Semaphore>>,
    max_connections_per_host: Option<usize>,
    per_host_sems: Mutex<HashMap<PoolKey, Arc<Semaphore>>>,
    stats_index: Arc<StatsIndex>,
    /// Late-bound back-reference to the registry, for cross-partition
    /// reclaim. `Weak` (not `Arc`): the registry transitively owns the
    /// partitions whose `ConnectionLimit`s reach back through here — a
    /// strong reference would be a cycle (pool never drops). Set once,
    /// immediately after the registry is built (`build_pool`), because the
    /// registry does not exist when `SharedPoolState` is constructed.
    /// Upgrade-or-skip: a dropped pool makes reclaim a correct no-op.
    registry: OnceLock<std::sync::Weak<partition::PartitionRegistry>>,
}

impl SharedPoolState {
    pub(crate) fn new(config: &PoolConfig) -> Self {
        Self {
            hooks: handshake::PoolHooks::new(config.connection_event_listener.clone()),
            global_sem: config.max_connections.map(|n| Arc::new(Semaphore::new(n))),
            max_connections_per_host: config.max_connections_per_host,
            per_host_sems: Mutex::new(HashMap::new()),
            stats_index: Arc::new(StatsIndex::default()),
            registry: OnceLock::new(),
        }
    }

    /// Bind the registry back-reference. Called once in `build_pool`
    /// right after the registry is constructed. Idempotent-safe: a second
    /// call is ignored (the first binding wins).
    pub(crate) fn set_registry(&self, registry: &Arc<partition::PartitionRegistry>) {
        let _ = self.registry.set(Arc::downgrade(registry));
    }

    /// A reclaim handle for the given requesting partition, if the
    /// registry is bound and still alive. `None` when no registry is set
    /// or the pool has been dropped (reclaim is then a no-op).
    pub(crate) fn reclaim_handle(&self, self_partition: PartitionId) -> Option<PeerReclaimHandle> {
        let registry = self.registry.get()?.clone();
        Some(PeerReclaimHandle {
            registry,
            stats_index: self.stats_index.clone(),
            self_partition,
        })
    }

    /// A borrow handle for the given requesting partition, if the registry
    /// is bound and still alive. `None` when no registry is set or the
    /// pool has been dropped (borrow is then unavailable; the caller
    /// blocks locally).
    pub(crate) fn borrow_handle(&self, self_partition: PartitionId) -> Option<PeerBorrowHandle> {
        let registry = self.registry.get()?.clone();
        Some(PeerBorrowHandle {
            registry,
            stats_index: self.stats_index.clone(),
            self_partition,
        })
    }

    /// The shared per-host semaphore for `key`, created on first request
    /// for that key. Returns `None` when no per-host limit is configured.
    /// Shared across all partitions: two partitions to the same host
    /// contend on the same budget.
    pub(crate) fn per_host_sem(&self, key: &PoolKey) -> Option<Arc<Semaphore>> {
        let n = self.max_connections_per_host?;
        let mut map = self.per_host_sems.lock().expect("per_host_sems poisoned");
        Some(
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(Semaphore::new(n)))
                .clone(),
        )
    }

    /// Pool-level inverted index for stats reads.
    pub(crate) fn stats_index(&self) -> &StatsIndex {
        &self.stats_index
    }
}

pub(crate) fn build_pool<C, IO>(
    connector: C,
    config: PoolConfig,
    partitions: Vec<partition::Partition>,
    cross_partition_policy: partition::CrossPartitionPolicy,
) -> ConnectionPool
where
    C: Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Unpin + Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + HyperConnection + Unpin + Send + 'static,
{
    let shared = Arc::new(SharedPoolState::new(&config));

    tracing::debug!(
        max_connections = ?config.max_connections,
        max_connections_per_host = ?config.max_connections_per_host,
        pool_idle_timeout = ?config.pool_idle_timeout,
        "pool: initialized"
    );

    let make_stack_for = {
        let connector = connector.clone();
        move |partition_id: partition::PartitionId,
              spawner: &Arc<dyn partition::DriverSpawner>|
              -> MakeStack {
            let connector = connector.clone();
            let spawner = spawner.clone();
            let cross_partition_policy = cross_partition_policy;
            Arc::new(
                move |uri: &http_1x::Uri, shared: &SharedPoolState| -> Box<dyn PoolEntry> {
                    let authority = Authority::new(
                        uri.authority()
                            .expect("pool entry URI has authority")
                            .as_str(),
                    );

                    let counters = Arc::new(ConnectionCounters::default());
                    shared
                        .stats_index()
                        .register(authority.clone(), partition_id, &counters);

                    let key = PoolKey::from_uri(uri).expect("pool entry URI has scheme+authority");
                    let per_host_sem = shared.per_host_sem(&key);
                    let limited = handshake::ConnectionLimit::new(
                        connector.clone(),
                        shared.global_sem.clone(),
                        per_host_sem,
                        counters.clone(),
                        shared.reclaim_handle(partition_id),
                    );

                    let pool_hooks = shared.hooks.clone();

                    // Per-host bridge: `H2ConnectAndHandshake` publishes on each new
                    // handshake; `SingletonConnection` reads the current entry at
                    // checkout. Clones share the underlying slot.
                    let h2_ref = H2ConnectionRef::new();

                    // Bounded at two: the H1 fallback and H2 upgrade `layer_fn`s
                    // each push one entry on first construction.
                    let retainers: RetainerSlot = Arc::new(Mutex::new(Vec::with_capacity(2)));

                    let stack =
                        hpool::negotiate::builder()
                            .connect(limited)
                            .inspect(|established: &connection::EstablishedConnection<IO>| {
                                established.io.connected().is_negotiated_h2()
                            })
                            .fallback({
                                let retainers = retainers.clone();
                                let pool_hooks = pool_hooks.clone();
                                let spawner = spawner.clone();
                                let counters = counters.clone();
                                tower::layer::layer_fn(move |inspector| {
                                    let cache = cache::builder()
                                        .executor(TokioExecutor::new())
                                        .build(H1ConnectAndHandshake::new(
                                            inspector,
                                            pool_hooks.clone(),
                                            spawner.clone(),
                                        ));
                                    // Capture a clone of the Cache for eviction. `Cache`
                                    // is Clone and shares state via its internal
                                    // `Arc<Mutex<Shared>>`, so the clone here and the
                                    // original-stack consumption below observe the same
                                    // idle set.
                                    let cache_for_retain =
                                        Arc::new(std::sync::Mutex::new(cache.clone()));
                                    let cache_for_empty = cache_for_retain.clone();
                                    let cache_for_reclaim = cache_for_retain.clone();
                                    let cache_for_borrow = cache_for_retain.clone();
                                    retainers.lock().expect("retainer slot poisoned").push(
                                        BoxedRetainer {
                                            retain_fn: Box::new({
                                                let listener = pool_hooks.listener.clone();
                                                move |timeout| {
                                                    let now = Instant::now();
                                                    cache_for_retain
                                                .lock()
                                                .expect("retain cache lock poisoned")
                                                .retain(|managed| {
                                                    let keep = !managed.is_poisoned()
                                                        && now.saturating_duration_since(
                                                            managed.idle_at(),
                                                        ) < timeout;
                                                    if !keep {
                                                        let reason = if managed.is_poisoned() {
                                                            "poisoned"
                                                        } else {
                                                            "idle_expired"
                                                        };
                                                        let idle_duration = now
                                                            .saturating_duration_since(
                                                                managed.idle_at(),
                                                            );
                                                        tracing::debug!(
                                                            conn_id = %managed.conn_id(),
                                                            reason,
                                                            ?idle_duration,
                                                            "pool: connection evicted"
                                                        );
                                                        if let Some(ref l) = listener {
                                                            l.on_closed(&ConnectionClosedEvent::new(
                                                                managed.conn_id(),
                                                                managed.info.authority.clone(),
                                                                managed.info.remote_addr,
                                                                if managed.is_poisoned() {
                                                                    CloseReason::Poisoned
                                                                } else {
                                                                    CloseReason::IdleTimeout
                                                                },
                                                                None,
                                                            ));
                                                        }
                                                    }
                                                    keep
                                                });
                                                }
                                            }),
                                            is_empty_fn: Box::new(move || {
                                                cache_for_empty
                                                    .lock()
                                                    .expect("is_empty cache lock poisoned")
                                                    .is_empty()
                                            }),
                                            reclaim_fn: Box::new({
                                                let listener = pool_hooks.listener.clone();
                                                move || {
                                                    // Pop under the cache lock, RELEASE the
                                                    // lock, THEN drop the connection (never
                                                    // drop while holding the cache Mutex).
                                                    let managed = cache_for_reclaim
                                                        .lock()
                                                        .expect("reclaim cache lock poisoned")
                                                        .try_pop_idle();
                                                    match managed {
                                                        Some(managed) => {
                                                            tracing::debug!(
                                                                conn_id = %managed.conn_id(),
                                                                "pool: connection reclaimed"
                                                            );
                                                            if let Some(ref l) = listener {
                                                                l.on_closed(
                                                                    &ConnectionClosedEvent::new(
                                                                        managed.conn_id(),
                                                                        managed
                                                                            .info
                                                                            .authority
                                                                            .clone(),
                                                                        managed.info.remote_addr,
                                                                        CloseReason::Reclaimed,
                                                                        None,
                                                                    ),
                                                                );
                                                            }
                                                            // `managed` drops here, after the
                                                            // cache lock is released: its
                                                            // `ConnectionPermit` returns a
                                                            // permit to the shared semaphore.
                                                            drop(managed);
                                                            true
                                                        }
                                                        None => false,
                                                    }
                                                }
                                            }),
                                            borrow_fn: Box::new({
                                                let listener = pool_hooks.listener.clone();
                                                let counters = counters.clone();
                                                move || {
                                                    // Take an idle connection wrapped so it
                                                    // returns to THIS (the owner's) cache on
                                                    // drop — the borrower dispatches one
                                                    // request through it but never owns it.
                                                    // `None` if no idle connection is
                                                    // available (borrower falls through to
                                                    // the next peer or to a blocking local
                                                    // acquire).
                                                    let cached = cache_for_borrow
                                                        .lock()
                                                        .expect("borrow cache lock poisoned")
                                                        .try_checkout_idle()?;
                                                    let checkout = H1Checkout::<()>::new(
                                                        CachedConnection::new(
                                                            cached,
                                                            listener.clone(),
                                                            counters.clone(),
                                                        ),
                                                    );
                                                    Some(Box::new(checkout)
                                                        as Box<dyn DispatchConn>)
                                                }
                                            }),
                                        },
                                    );
                                    cache.map_response({
                                        let listener = pool_hooks.listener.clone();
                                        let counters = counters.clone();
                                        move |cached| {
                                            H1Checkout::new(CachedConnection::new(
                                                cached,
                                                listener.clone(),
                                                counters.clone(),
                                            ))
                                        }
                                    })
                                })
                            })
                            .upgrade({
                                let h2_ref = h2_ref.clone();
                                let retainers = retainers.clone();
                                let pool_hooks = pool_hooks.clone();
                                let spawner = spawner.clone();
                                let counters = counters.clone();
                                tower::layer::layer_fn(move |inspected| {
                                    let singleton = hpool::singleton::Singleton::new(
                                        H2ConnectAndHandshake::new(
                                            inspected,
                                            h2_ref.clone(),
                                            pool_hooks.clone(),
                                            authority.clone(),
                                            spawner.clone(),
                                        ),
                                    );
                                    let singleton_for_retain =
                                        Arc::new(std::sync::Mutex::new(singleton.clone()));
                                    let singleton_for_empty = singleton_for_retain.clone();
                                    retainers.lock().expect("retainer slot poisoned").push(
                                        BoxedRetainer {
                                            retain_fn: Box::new({
                                                let listener = pool_hooks.listener.clone();
                                                move |timeout| {
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
                                                            .saturating_duration_since(
                                                                managed.idle_at(),
                                                            );
                                                        tracing::debug!(
                                                            conn_id = %managed.conn_id(),
                                                            reason,
                                                            ?idle_duration,
                                                            "pool: connection evicted"
                                                        );
                                                        if let Some(ref l) = listener {
                                                            l.on_closed(&ConnectionClosedEvent::new(
                                                                managed.conn_id(),
                                                                managed.info.authority.clone(),
                                                                managed.info.remote_addr,
                                                                if managed.is_poisoned() {
                                                                    CloseReason::Poisoned
                                                                } else {
                                                                    CloseReason::IdleTimeout
                                                                },
                                                                None,
                                                            ));
                                                        }
                                                    }
                                                    keep
                                                });
                                                }
                                            }),
                                            is_empty_fn: Box::new(move || {
                                                singleton_for_empty
                                                    .lock()
                                                    .expect("is_empty singleton lock poisoned")
                                                    .is_empty()
                                            }),
                                            // An idle H2 connection still multiplexes; it is
                                            // not simply-reclaimable like an H1 cache entry.
                                            // Reclaim skips the H2 leg.
                                            reclaim_fn: Box::new(|| false),
                                            // H2 borrow is unsupported: an idle H2
                                            // connection still multiplexes on its owner,
                                            // so there is no idle connection to hand to a
                                            // borrower. Borrow skips the H2 leg.
                                            borrow_fn: Box::new(|| None),
                                        },
                                    );
                                    singleton.map_response({
                                        let h2_ref = h2_ref.clone();
                                        let counters = counters.clone();
                                        move |singled| {
                                            H2Checkout::new(SingletonConnection::new(
                                                singled,
                                                h2_ref.clone(),
                                                counters.clone(),
                                            ))
                                        }
                                    })
                                })
                            })
                            .build();
                    Box::new(TypedPoolEntry {
                        stack,
                        retainers,
                        counters,
                        borrow: match cross_partition_policy {
                            partition::CrossPartitionPolicy::PreferLocal => {
                                shared.borrow_handle(partition_id)
                            }
                            partition::CrossPartitionPolicy::Never => None,
                        },
                    })
                },
            )
        }
    };

    let partitions = partition::normalize_partitions(partitions, || {
        Arc::new(partition::TokioDriverSpawner::current())
    });
    let registry = Arc::new(partition::PartitionRegistry::build(
        partitions,
        make_stack_for,
    ));

    // Late-bind the registry back-reference for cross-partition reclaim.
    // Done here, after the registry exists, because `SharedPoolState` is
    // built before it (the `make_stack` closures capture `shared`).
    shared.set_registry(&registry);

    ConnectionPool {
        config,
        shared,
        eviction_spawned: AtomicBool::new(false),
        drop_notifier: OnceLock::new(),
        registry,
        cross_partition_policy,
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

    /// Shared connection-lifecycle machinery (hooks, semaphores).
    shared: Arc<SharedPoolState>,

    /// Immutable partition registry, resolved at build time.
    registry: Arc<partition::PartitionRegistry>,

    // used by cross-partition checkout
    #[allow(dead_code)]
    cross_partition_policy: partition::CrossPartitionPolicy,

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

/// Minimum eviction tick period. Prevents very short `pool_idle_timeout`s
/// from causing the eviction task to spin hot.
const MIN_EVICTION_TICK: Duration = Duration::from_millis(90);

impl ConnectionPool {
    /// Access the immutable partition registry.
    pub(crate) fn registry(&self) -> &Arc<partition::PartitionRegistry> {
        &self.registry
    }

    /// Send a request through the pool.
    ///
    /// Routes to the appropriate per-host pool stack in the given partition
    /// and sends the request. `ctx` carries the routing URI plus
    /// per-operation connect-time data (connect_timeout). Per-operation
    /// read_timeout, if any, must be attached to `req.extensions_mut()` as
    /// [`ReadTimeoutHint`] before calling; the checkout services read it
    /// from there.
    ///
    /// Takes `self: &Arc<Self>` so the lazy eviction task can hold a
    /// `Weak<Self>` as a fallback exit signal (primary exit is the drop
    /// of `drop_notifier`; the `Weak` just insulates the task against a
    /// missed drop signal).
    pub(crate) async fn send_request(
        self: &Arc<Self>,
        partition: &Arc<partition::PartitionState>,
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
            let mut auth = partition.authorities.lock().unwrap();
            if !auth.contains_key(&key) {
                let entry = (partition.make_stack)(&ctx.uri, &self.shared);
                auth.insert(key.clone(), entry);
            }
            auth.get_mut(&key).unwrap().send(ctx, req)
        };

        fut.await
    }

    /// Spawn the idle-eviction task if it hasn't been spawned yet and
    /// `pool_idle_timeout` is configured.
    ///
    /// Idempotent and cheap after the first call (single relaxed
    /// `AtomicBool` load returns early). No-op without a timeout, with
    /// a zero timeout, or if already spawned.
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
            "pool: eviction task spawned"
        );
        tokio::spawn(eviction_task(weak, rx, timeout));
    }

    /// Walk all partitions, dropping idle connections that have exceeded
    /// `timeout` and removing host entries whose retainers are empty.
    ///
    /// Called by the eviction task on each tick. Safe to call when
    /// partitions are empty (no-op).
    fn retain_idle(&self, timeout: Duration) {
        for partition in self.registry.partitions() {
            let partition_id = partition.id;
            let mut evicted: Vec<Authority> = Vec::new();
            {
                let mut auth = partition.authorities.lock().unwrap();
                auth.retain(|key, entry| {
                    entry.retain_idle(timeout);
                    if entry.is_empty() {
                        tracing::debug!("pool: host entry removed (empty after retain)");
                        evicted.push(Authority::new(key.authority.as_str()));
                        false
                    } else {
                        true
                    }
                });
                // `auth` guard drops here, dropping the removed entries (and
                // the strong `Arc<ConnectionCounters>` they held). Only then
                // can the index prune observe a zero strong count — unless a
                // checkout is still in flight, in which case the cell stays.
            }
            for authority in evicted {
                self.shared
                    .stats_index()
                    .prune_if_dead(&authority, partition_id);
            }
        }
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
                    Some(pool) => pool.retain_idle(timeout),
                    None => break,
                }
            }
        }
    }
    tracing::trace!("pool eviction task exiting");
}

/// Rewrite an HTTP/1.1 request's URI to the appropriate request-target
/// form for dispatch:
///
/// - `CONNECT` → authority-form (`host:port`)
/// - proxied non-CONNECT → absolute-form (full URL with scheme + authority)
/// - direct non-CONNECT → origin-form (path + query only)
///
/// The HTTP/1 connection sends request URIs as-is, so request-target
/// form selection is the caller's responsibility.
/// Recognize hyper-util's `Singleton` coalescing-cancel sentinel by its
/// `Display`. The error type (`SingletonError`) and its inner `Canceled`
/// are upstream-private and not downcastable, so the chain is matched by
/// string. Deliberately narrow: only the exact "singleton connection
/// canceled" message re-enters a checkout; any other error propagates.
/// The string match is exercised by the multi-threaded concurrency test,
/// which fails if the upstream message changes; if upstream stops
/// producing the sentinel, this becomes inert.
fn is_singleton_canceled(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut e = Some(err);
    while let Some(cur) = e {
        if cur.to_string() == "singleton connection canceled" {
            return true;
        }
        e = cur.source();
    }
    false
}

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
        tracing::trace!(conn_id = %conn.conn_id(), uri = %req.uri(), "pool: dispatching request");
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

impl<UnusedH2Phantom> DispatchConn for H1Checkout<UnusedH2Phantom>
where
    UnusedH2Phantom: Send + Sync + 'static,
{
    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), BoxError>> {
        Service::poll_ready(self, cx)
    }

    fn dispatch(
        mut self: Box<Self>,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>> {
        // `H1Checkout::call` produces `CheckoutResponse<…>` whose body is
        // the internal `GuardedBody`; erase it to `SdkBody` at this
        // boundary, exactly as `TypedPoolEntry::send` does for the local
        // path. The guard inside lives on in `SdkBody` until the body is
        // drained, returning the connection to the peer's pool.
        let fut = Service::call(&mut *self, req);
        Box::pin(async move {
            let resp = fut.await?;
            let (parts, body) = resp.into_parts();
            Ok(http_1x::Response::from_parts(
                parts,
                SdkBody::from_body_1_x(body),
            ))
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
        if let Some(id) = conn.metadata().and_then(|m| m.connection_id()) {
            tracing::trace!(conn_id = %id, uri = %req.uri(), "pool: dispatching request");
        }
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
pub(crate) trait PoolEntry: Send + Sync {
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

    /// Pop one idle connection and drop it to free its permit, returning
    /// `true` if one was freed. Drives cross-partition active reclaim: a
    /// starved partition frees an over-supplied peer's idle capacity at
    /// the cap-bound point. The dropped connection's `ConnectionPermit`
    /// releases back to the shared semaphore. Fires `on_closed` with
    /// [`CloseReason::Reclaimed`] for the freed connection. Best-effort:
    /// returns `false` if the entry has no reclaimable idle.
    fn try_reclaim_one(&self) -> bool;

    /// Take one idle connection as a dispatchable handle that returns to
    /// this entry's pool on drop, for cross-partition borrow. The borrower
    /// dispatches one request through it; the connection stays this
    /// entry's (no permit moves). Returns `None` if no idle connection is
    /// available. Best-effort — only the H1 cache leg yields a handle (H2
    /// idle still multiplexes on its owner, so the H2 leg returns `None`).
    fn try_borrow_one(&self) -> Option<Box<dyn DispatchConn>>;
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
    // active write-handle; read API added in a later change
    #[allow(dead_code)]
    counters: Arc<ConnectionCounters>,
    /// Cross-partition borrow handle. `Some` only under
    /// `CrossPartitionPolicy::PreferLocal`; `None` under `Never` (and for
    /// the single-partition default, where it would have no NIC-group
    /// peers anyway). Drives the `send` cap-bound borrow branch.
    borrow: Option<PeerBorrowHandle>,
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
        let borrow = self.borrow.clone();
        Box::pin(async move {
            // Local checkout. The post-checkout `poll_ready` is the
            // reactive health check: the composable pool has no proactive
            // checkout-time health check, so a popped idle connection may
            // be dead (server closed it, driver gone). `poll_ready` Err
            // identifies that; the loop discards it and pops the next,
            // until a live connection is checked out or the connect path
            // is reached. Converges because:
            //   - Cache idle set is bounded; each post-checkout `poll_ready`
            //     Err flips `is_closed` so `Cached::Drop` skips reinsertion,
            //     shrinking the set by one.
            //   - Singleton clears to `Empty` on `poll_ready` Err, forcing
            //     the next `call` to run a fresh handshake; a handshake
            //     failure surfaces as an error from `svc.call` (not the
            //     inner `poll_ready`), which we propagate.
            // Under `AcquireMode::NonBlocking`, a cache miss at the connect
            // path returns `CapBound` instead of blocking — surfaced here
            // so the caller can try a peer borrow before committing to wait.
            async fn local_checkout<S, Conn, PoolUnnameable>(
                svc: &mut S,
                ctx: ConnectCtx,
            ) -> Result<Conn, BoxError>
            where
                S: Service<ConnectCtx, Response = Conn>,
                S::Error: Into<BoxError>,
                Conn:
                    Service<http_1x::Request<SdkBody>, Response = CheckoutResponse<PoolUnnameable>>,
                Conn::Error: Into<BoxError>,
            {
                loop {
                    std::future::poll_fn(|cx| svc.poll_ready(cx))
                        .await
                        .map_err(Into::into)?;
                    let mut checkout = match svc.call(ctx.clone()).await {
                        Ok(c) => c,
                        Err(e) => {
                            let e: BoxError = e.into();
                            // A coalesced waiter whose `Singleton` maker
                            // bounced to the H1 fallback resolves `Canceled`
                            // without being reused or connected. Nothing was
                            // established, so re-enter the checkout (the
                            // retry becomes a maker or reuses the made
                            // connection) rather than fail.
                            if is_singleton_canceled(&*e) {
                                continue;
                            }
                            return Err(e);
                        }
                    };
                    if std::future::poll_fn(|cx| checkout.poll_ready(cx))
                        .await
                        .is_ok()
                    {
                        return Ok(checkout);
                    }
                    // drop `checkout` → pool cleanup (H1: discard via
                    // CachedConnection::Drop; H2: Singleton already cleared).
                }
            }

            // Erase a checkout's `CheckoutResponse` body to `SdkBody` at the
            // `dyn PoolEntry` boundary so consumers never see the internal
            // `GuardedBody<...>`. The guard inside lives on in `SdkBody`
            // until the body drains, returning the connection to its pool.
            fn erase_body<PoolUnnameable>(
                resp: CheckoutResponse<PoolUnnameable>,
            ) -> http_1x::Response<SdkBody>
            where
                PoolUnnameable: Send + Sync + 'static,
            {
                let (parts, body) = resp.into_parts();
                http_1x::Response::from_parts(parts, SdkBody::from_body_1_x(body))
            }

            match &borrow {
                // PreferLocal: probe locally without blocking; on a
                // cap-bound miss, borrow a peer's idle connection before
                // falling back to a blocking local acquire.
                Some(handle) => {
                    let probe = ctx.clone().with_mode(connection::AcquireMode::NonBlocking);
                    match local_checkout(&mut svc, probe).await {
                        Ok(mut conn) => {
                            let resp = conn.call(req).await.map_err(Into::into)?;
                            Ok(erase_body(resp))
                        }
                        // Cap-bound: the cap is full (no connect was
                        // attempted under `NonBlocking`). Borrow a peer's
                        // idle connection, else fall back to the
                        // authoritative blocking acquire. A genuine connect
                        // error (a permit was free but the connect failed)
                        // is not `CapBound` and propagates.
                        Err(err) if connection::CapBound::is(&*err) => {
                            let key = PoolKey::from_uri(&ctx.uri)
                                .ok_or("request URI must have scheme and authority")?;
                            if let Some(mut borrowed) = handle.try_borrow(&key) {
                                // Single-shot liveness check. On death,
                                // fall through to the authoritative local
                                // acquire rather than walk further peers.
                                if std::future::poll_fn(|cx| borrowed.poll_ready(cx))
                                    .await
                                    .is_ok()
                                {
                                    // Dispatch through the peer's connection;
                                    // its driver stays on the peer's runtime,
                                    // and the connection returns to the peer's
                                    // pool when the body drains.
                                    return borrowed.dispatch(req).await;
                                }
                                // Dead borrowed connection drops here:
                                // `CachedConnection::Drop` discards it from
                                // the peer's pool and balances the peer's
                                // `active`.
                            }
                            // Borrow miss → authoritative blocking local
                            // acquire (today's wait; includes reclaim).
                            let mut conn = local_checkout(&mut svc, ctx).await?;
                            let resp = conn.call(req).await.map_err(Into::into)?;
                            Ok(erase_body(resp))
                        }
                        Err(err) => Err(err),
                    }
                }
                // Never (and single-partition default): blocking local
                // acquire, unchanged.
                None => {
                    let mut conn = local_checkout(&mut svc, ctx).await?;
                    let resp = conn.call(req).await.map_err(Into::into)?;
                    Ok(erase_body(resp))
                }
            }
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

    fn try_reclaim_one(&self) -> bool {
        let retainers = self.retainers.lock().expect("retainer slot poisoned");
        // Stop at the first leg that frees a connection. The H1 cache leg
        // pops an idle connection; the H2 leg is a no-op (no
        // simply-reclaimable idle).
        retainers.iter().any(|r| r.reclaim_one())
    }

    fn try_borrow_one(&self) -> Option<Box<dyn DispatchConn>> {
        let retainers = self.retainers.lock().expect("retainer slot poisoned");
        // Return the first leg that yields a borrowable handle. The H1
        // cache leg checks out an idle connection; the H2 leg is a no-op
        // returning `None`.
        retainers.iter().find_map(|r| r.borrow_one())
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
        fn try_reclaim_one(&self) -> bool {
            false
        }
        fn try_borrow_one(&self) -> Option<Box<dyn DispatchConn>> {
            None
        }
    }

    fn pool_with_config(config: PoolConfig) -> Arc<ConnectionPool> {
        let shared = Arc::new(SharedPoolState::new(&config));
        let make_stack_for = |_partition_id: partition::PartitionId,
                              _spawner: &Arc<dyn partition::DriverSpawner>|
         -> MakeStack {
            Arc::new(|_uri, _shared| Box::new(NullPoolEntry) as Box<dyn PoolEntry>)
        };
        let partitions = partition::normalize_partitions(Vec::new(), || {
            Arc::new(partition::TokioDriverSpawner::current())
        });
        let registry = Arc::new(partition::PartitionRegistry::build(
            partitions,
            make_stack_for,
        ));
        Arc::new(ConnectionPool {
            config,
            shared,
            registry,
            cross_partition_policy: CrossPartitionPolicy::default(),
            eviction_spawned: AtomicBool::new(false),
            drop_notifier: OnceLock::new(),
        })
    }

    /// A `make_stack_for` stub that produces `NullPoolEntry` stacks — enough
    /// to exercise registry indexing without standing up real connectors.
    fn null_make_stack_for(
    ) -> impl Fn(partition::PartitionId, &Arc<dyn partition::DriverSpawner>) -> MakeStack {
        |_id, _spawner| Arc::new(|_uri, _shared| Box::new(NullPoolEntry) as Box<dyn PoolEntry>)
    }

    /// The registry's default partition is the first declared.
    #[tokio::test]
    async fn registry_default_partition_is_first_declared() {
        let partitions = vec![
            partition::Partition::new(
                partition::PartitionId::from_index(7),
                partition::TokioDriverSpawner::current(),
            ),
            partition::Partition::new(
                partition::PartitionId::from_index(3),
                partition::TokioDriverSpawner::current(),
            ),
        ];
        let registry = partition::PartitionRegistry::build(partitions, null_make_stack_for());
        assert_eq!(
            registry.default_partition().id,
            partition::PartitionId::from_index(7),
            "first declared partition is the default"
        );
        // Both declared partitions resolve.
        assert!(registry
            .partition_opt(partition::PartitionId::from_index(3))
            .is_some());
        assert!(registry
            .partition_opt(partition::PartitionId::from_index(99))
            .is_none());
    }

    /// Declaring the same `PartitionId` twice is a programming error and panics.
    #[tokio::test]
    #[should_panic(expected = "duplicate PartitionId")]
    async fn registry_build_panics_on_duplicate_partition_id() {
        let partitions = vec![
            partition::Partition::new(
                partition::PartitionId::from_index(0),
                partition::TokioDriverSpawner::current(),
            ),
            partition::Partition::new(
                partition::PartitionId::from_index(0),
                partition::TokioDriverSpawner::current(),
            ),
        ];
        let _ = partition::PartitionRegistry::build(partitions, null_make_stack_for());
    }

    /// The default (no-topology) path normalizes to a single anonymous
    /// partition that the registry indexes and resolves via `Client::new`.
    #[tokio::test]
    async fn registry_anonymous_default_when_no_partitions_declared() {
        let partitions = partition::normalize_partitions(Vec::new(), || {
            Arc::new(partition::TokioDriverSpawner::current())
        });
        let registry = partition::PartitionRegistry::build(partitions, null_make_stack_for());
        assert_eq!(
            registry.default_partition().id,
            partition::PartitionId::default(),
            "no-topology default is the anonymous partition"
        );
    }

    /// Without a `pool_idle_timeout`, `maybe_spawn_eviction_task` is a no-op.
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

    /// A zero `pool_idle_timeout` is treated the same as `None`.
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

    /// `retain_idle` drops host entries whose retainers report empty.
    /// Uses `NullPoolEntry` whose `is_empty` returns `true`, so the first
    /// tick should remove every entry.
    #[tokio::test]
    async fn retain_idle_hosts_drops_empty_entries() {
        let pool = pool_with_config(PoolConfig::default());
        let uri: http_1x::Uri = "http://example.com".parse().unwrap();
        let key = PoolKey::from_uri(&uri).unwrap();
        let partition = pool.registry().default_partition();
        partition
            .authorities
            .lock()
            .unwrap()
            .insert(key.clone(), Box::new(NullPoolEntry));
        assert_eq!(partition.authorities.lock().unwrap().len(), 1);
        pool.retain_idle(Duration::from_secs(30));
        assert_eq!(
            partition.authorities.lock().unwrap().len(),
            0,
            "empty entry should have been evicted"
        );
    }

    #[test]
    fn per_host_sem_shared_by_key() {
        let cfg = PoolConfig {
            max_connections_per_host: Some(2),
            ..PoolConfig::default()
        };
        let s = SharedPoolState::new(&cfg);
        let uri: http_1x::Uri = "https://example.com".parse().unwrap();
        let key = PoolKey::from_uri(&uri).unwrap();
        let a = s.per_host_sem(&key).unwrap();
        let b = s.per_host_sem(&key).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "same key must share one semaphore");
        let uri2: http_1x::Uri = "https://other.com".parse().unwrap();
        let key2 = PoolKey::from_uri(&uri2).unwrap();
        let c = s.per_host_sem(&key2).unwrap();
        assert!(
            !Arc::ptr_eq(&a, &c),
            "different key must get a distinct semaphore"
        );
    }

    #[tokio::test]
    async fn two_partitions_independent_storage() {
        let pool = SharedPool::builder()
            .partitions([
                partition::Partition::new(
                    partition::PartitionId::from_index(0),
                    partition::TokioDriverSpawner::current(),
                ),
                partition::Partition::new(
                    partition::PartitionId::from_index(1),
                    partition::TokioDriverSpawner::current(),
                ),
            ])
            .build_http();
        let p0 = pool
            .inner
            .pool
            .registry()
            .partition(partition::PartitionId::from_index(0));
        let p1 = pool
            .inner
            .pool
            .registry()
            .partition(partition::PartitionId::from_index(1));
        assert!(
            !Arc::ptr_eq(&p0, &p1),
            "distinct partition ids must yield distinct PartitionState arcs"
        );
    }

    /// End-to-end lifecycle: `EstablishingGuard::new` → `promote` →
    /// `ManagedConnection` holds `EstablishedGuard` → drop decrements.
    /// Verifies the wiring from handshake through connection lifetime.
    #[test]
    fn established_set_after_handshake() {
        use connection::{Authority, ConnectionInfo, ConnectionPermit, ManagedConnection};
        use stats::{ConnectionCounters, EstablishingGuard};

        let counters = Arc::new(ConnectionCounters::default());
        let authority = Authority::new("example.com:443");
        let partition_id = partition::PartitionId::from_index(0);

        // Register in StatsIndex so we can read through that path too
        let index = StatsIndex::default();
        index.register(authority.clone(), partition_id, &counters);

        // Simulate: ConnectionLimit creates guard post-permit
        let establishing = EstablishingGuard::new(counters.clone());
        assert_eq!(index.establishing_for(&authority, partition_id), 1);
        assert_eq!(index.established_for(&authority, partition_id), 0);

        // Simulate: handshake succeeds, promote
        let established = establishing.promote(stats::PROTO_H1);
        assert_eq!(index.establishing_for(&authority, partition_id), 0);
        assert_eq!(index.established_for(&authority, partition_id), 1);

        // Simulate: ManagedConnection holds the EstablishedGuard
        let permit = Arc::new(ConnectionPermit::new(None, None));
        let info = ConnectionInfo {
            remote_addr: None,
            local_addr: None,
            is_proxied: false,
            authority: authority.clone(),
        };
        let conn: ManagedConnection<()> = ManagedConnection::new(
            (),
            info,
            aws_smithy_runtime_api::client::connection::ConnectionId::new(0),
            permit,
            established,
        );

        // H2-style: clone shares the same Arc<EstablishedGuard>
        let conn2 = conn.clone();
        assert_eq!(index.established_for(&authority, partition_id), 1);

        // Drop one clone — guard still alive via the other
        drop(conn);
        assert_eq!(index.established_for(&authority, partition_id), 1);

        // Drop last clone — guard fires, established decrements
        drop(conn2);
        assert_eq!(index.established_for(&authority, partition_id), 0);
    }
}

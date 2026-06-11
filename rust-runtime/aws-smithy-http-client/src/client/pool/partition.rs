/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool partitioning.
//!
//! A partition is a label that groups connections by locality: the runtime
//! that owns their drivers and the network interface their sockets bind to.
//! Partition labels are assigned through client configuration; the pool
//! indexes connections by label and respects locality at checkout.
//!
//! # Topologies
//!
//! ## Single partition
//!
//! No partitioning. All connections pool together. The runtime is whatever
//! was current at client construction; no NIC binding.
//!
//! ```text
//! Pool
//! └── Partition (anonymous, runtime=tokio-mt, nic=none)
//!     ├── conn-1
//!     └── conn-2
//! ```
//!
//! ## Per-runtime, no NIC binding
//!
//! N partitions, one per runtime. Each partition's connections have
//! drivers on that partition's runtime.
//!
//! ```text
//! Pool
//! ├── Partition 0 (runtime=tokio-current R0, nic=none)
//! │   ├── conn-1 (driver on R0)
//! │   └── conn-2 (driver on R0)
//! └── Partition 1 (runtime=tokio-current R1, nic=none)
//!     └── conn-3 (driver on R1)
//! ```
//!
//! ## Per-runtime, per-NIC
//!
//! Partitions cluster by NIC. A socket bound to one NIC cannot serve
//! traffic on another.
//!
//! ```text
//! Pool
//! ├── Partition 0 (runtime=R0, nic=eth0) ─┐
//! ├── Partition 1 (runtime=R1, nic=eth0) ─┴─ same NIC group
//! ├── Partition 2 (runtime=R2, nic=eth1) ─┐
//! └── Partition 3 (runtime=R3, nic=eth1) ─┴─ same NIC group
//! ```
//!
//! ## NUMA-aware
//!
//! Partitions align with NUMA topology: runtimes pinned to cores on a
//! node, NIC selected to match the node. The pool does not detect NUMA
//! topology; it sees only `(PartitionId, runtime, nic)` and the
//! alignment is established when clients are configured.
//!
//! ```text
//! Pool
//! ├── NUMA node 0
//! │   ├── Partition 0 (runtime=R0 on core 0,  nic=eth0)
//! │   ├── Partition 1 (runtime=R1 on core 1,  nic=eth0)
//! │   └── Partition 2 (runtime=R2 on core 2,  nic=eth1)
//! └── NUMA node 1
//!     ├── Partition 3 (runtime=R3 on core 32, nic=eth2)
//!     └── Partition 4 (runtime=R4 on core 33, nic=eth3)
//! ```
//!
//! # Boundaries
//!
//! - **NIC (hard):** a connection's NIC binding is fixed at creation.
//!   Connections form NIC groups: one group per `Some(nic)` value plus
//!   an unbound group for `None`. The pool only returns a connection
//!   to a checkout in the same NIC group; the unbound group is not a
//!   wildcard.
//! - **Runtime (soft):** a connection's driver runs on a specific
//!   runtime. Cross-runtime checkout is feasible (the request is
//!   dispatched through the driver's runtime via a channel) but costs
//!   a cross-thread send. [`CrossPartitionPolicy`] controls whether
//!   the pool crosses this boundary at checkout.
//!
//! # Checkout
//!
//! When a request arrives on partition P for authority A:
//!
//! 1. **Local hit:** an idle connection in P for A is reused.
//! 2. **Local miss, under capacity:** a new connection is created
//!    in P.
//! 3. **Local miss, at capacity:** behavior depends on
//!    [`CrossPartitionPolicy`].
//!
//! Cross-partition borrowing is a capacity-pressure fallback. Under
//! normal load each partition creates its own connections.
//!
//! ## Policy: `Never`
//!
//! At capacity with no local idle, the request waits for a permit.
//! Peer partitions are not consulted.
//!
//! ```text
//! Capacity = 2, both in use, request arrives on P0 for A:
//!
//!   P0 (eth0): [active to B]   ← request for A queues here
//!   P1 (eth0): [idle to A]     ← not consulted
//!
//! Outcome: request blocks until a permit is available, then either
//! reuses a returning idle for A or creates a new connection on P0.
//! ```
//!
//! ## Policy: `PreferLocal`
//!
//! At capacity with no local idle, the pool checks peer partitions in
//! the same NIC group for an idle connection to the requested
//! authority. If one is found, the request borrows it. Otherwise it
//! waits for a permit.
//!
//! ```text
//! Capacity = 2, both in use, request arrives on P0 for A:
//!
//!   P0 (eth0): [active to B]   ← request for A
//!   P1 (eth0): [idle to A]     ← borrowed
//!   P2 (eth1): [idle to A]     ← different NIC, never consulted
//!
//! Outcome: P1's idle connection serves the request. The connection's
//! driver stays on P1's runtime; the request flows through P1's
//! runtime via the connection's channel.
//! ```

/// Identifier for a pool partition.
///
/// A partition's identity is opaque to the pool. The identifier is
/// assigned at client construction and groups connections that share
/// a driver spawner and network interface binding.
///
/// The default identifier denotes an anonymous partition used when
/// partitioning is not configured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartitionId(u64);

impl PartitionId {
    const ANONYMOUS: u64 = u64::MAX;

    /// Identifier from a numeric index.
    pub const fn from_index(index: usize) -> Self {
        Self(index as u64)
    }

    /// Identifier from a raw value. The value `u64::MAX` is reserved
    /// for the anonymous default partition.
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    /// Raw value of this identifier.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl Default for PartitionId {
    fn default() -> Self {
        Self(Self::ANONYMOUS)
    }
}

/// Spawner for connection driver tasks.
///
/// A driver is the task that owns an HTTP connection's I/O state machine:
/// reading frames, writing frames, and managing protocol-level events.
/// Calls through a connection's request handle flow through this driver.
/// The pool spawns one driver per established connection.
///
/// Different partitions may use different spawners, allowing each
/// partition's drivers to run on a specific runtime.
pub trait DriverSpawner: std::fmt::Debug + Send + Sync + 'static {
    /// Spawn the connection driver future on this spawner's runtime.
    fn spawn(
        &self,
        driver: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
    );
}

/// Driver spawner backed by a tokio runtime handle.
///
/// Spawns the connection driver via [`tokio::runtime::Handle::spawn`].
/// The handle is captured at construction; the driver runs on the
/// runtime the handle refers to, regardless of which runtime called
/// [`DriverSpawner::spawn`].
#[derive(Clone, Debug)]
pub struct TokioDriverSpawner {
    handle: tokio::runtime::Handle,
}

impl TokioDriverSpawner {
    /// Spawner using the current tokio runtime handle (captured eagerly).
    ///
    /// Panics if invoked outside a tokio runtime context.
    pub fn current() -> Self {
        Self::from_handle(tokio::runtime::Handle::current())
    }

    /// Spawner using a specific tokio runtime handle.
    pub fn from_handle(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }
}

impl DriverSpawner for TokioDriverSpawner {
    fn spawn(
        &self,
        driver: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
    ) {
        self.handle.spawn(driver);
    }
}

/// Policy governing checkout when the local partition has no idle
/// connection and the pool is at capacity.
///
/// Cross-partition borrowing applies within a NIC group only.
/// Connections bound to different NICs are never shared regardless
/// of policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CrossPartitionPolicy {
    /// At capacity with no local idle, the request waits for a permit.
    /// Peer partitions are not consulted.
    #[default]
    Never,
    /// At capacity with no local idle, the request borrows an idle
    /// connection from a peer partition in the same NIC group when one
    /// is available, otherwise waits for a permit.
    PreferLocal,
}

/// A declared pool partition: a driver-spawner runtime and an optional
/// NIC binding, identified by a caller-owned [`PartitionId`]. Declared on
/// the pool builder via `Builder::partitions`; the pool owns the topology
/// for its lifetime.
#[derive(Clone, Debug)]
pub struct Partition {
    id: PartitionId,
    spawner: std::sync::Arc<dyn DriverSpawner>,
    nic: Option<String>,
}

impl Partition {
    /// Declare a partition with the given id and driver spawner.
    pub fn new<S: DriverSpawner>(id: PartitionId, spawner: S) -> Self {
        Self {
            id,
            spawner: std::sync::Arc::new(spawner),
            nic: None,
        }
    }

    /// Bind this partition's connections to a network interface.
    pub fn interface(mut self, nic: impl Into<String>) -> Self {
        self.nic = Some(nic.into());
        self
    }
}

/// Pool-owned state for one declared partition. Resolved once at pool
/// build time and referenced by [`Client`](super::Client) handles.
pub(crate) struct PartitionState {
    pub(crate) id: PartitionId,
    // Captured into `make_stack` by the build factory; the field is retained
    // on the state but read through the captured closure, not directly.
    #[allow(dead_code)]
    pub(crate) spawner: std::sync::Arc<dyn DriverSpawner>,
    /// Network interface this partition's connections bind to, and the
    /// boundary for cross-partition borrow and reclaim (peers in the same
    /// NIC group only).
    pub(crate) nic: Option<String>,
    /// Per-host connection storage for this partition. Keyed by
    /// (scheme, authority); entries built lazily on first request.
    pub(crate) authorities:
        std::sync::Mutex<std::collections::HashMap<super::PoolKey, Box<dyn super::PoolEntry>>>,
    /// Builds a host entry on first touch, capturing this partition's
    /// connector; shared budget/hooks arrive via `&SharedPoolState`.
    pub(crate) make_stack: super::MakeStack,
    /// Round-robin cursor over reclaim/borrow candidate peers. Advisory
    /// (`Relaxed` `fetch_add`): rotates the starting offset into the
    /// candidate set so concurrent cap-bound reclaims from this partition
    /// do not all probe the lowest-numbered peer first. No correctness
    /// invariant rides it — `try_reclaim_one` is the authoritative gate.
    pub(crate) peer_cursor: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for PartitionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartitionState")
            .field("id", &self.id)
            .field("nic", &self.nic)
            .finish_non_exhaustive()
    }
}

/// Normalize a caller-declared partition set for pool construction: when
/// no partitions are declared, synthesize a single anonymous partition
/// (`PartitionId::default()`, no NIC binding) using `anonymous_spawner`.
/// Returns the caller's set unchanged when non-empty.
///
/// This is the one place the "no topology declared" default is decided;
/// [`PartitionRegistry::build`] then indexes whatever set it is given.
/// `anonymous_spawner` is a closure so the runtime handle is captured
/// only when actually needed (e.g. `TokioDriverSpawner::current()` panics
/// off a runtime).
pub(crate) fn normalize_partitions(
    partitions: Vec<Partition>,
    anonymous_spawner: impl FnOnce() -> std::sync::Arc<dyn DriverSpawner>,
) -> Vec<Partition> {
    if partitions.is_empty() {
        vec![Partition {
            id: PartitionId::default(),
            spawner: anonymous_spawner(),
            nic: None,
        }]
    } else {
        partitions
    }
}

/// Immutable registry of declared partitions, built once at pool
/// construction. Maps ids and NIC groups to partition state and records
/// the default partition used by [`Client::new`](super::Client::new).
#[derive(Debug)]
pub(crate) struct PartitionRegistry {
    by_id: std::collections::HashMap<PartitionId, std::sync::Arc<PartitionState>>,
    /// Partition ids grouped by NIC, for the cross-partition borrow and
    /// reclaim peer walk (candidates are drawn from the requester's NIC group).
    by_nic: std::collections::HashMap<Option<String>, Vec<PartitionId>>,
    default_partition: PartitionId,
}

impl PartitionRegistry {
    /// Build a registry from a non-empty set of declared partitions. The
    /// default partition is the first in the slice. Panics on a duplicate
    /// `PartitionId`, or if `partitions` is empty (callers normalize the
    /// no-topology case via [`normalize_partitions`] first).
    pub(crate) fn build(
        partitions: Vec<Partition>,
        make_stack_for: impl Fn(PartitionId, &std::sync::Arc<dyn DriverSpawner>) -> super::MakeStack,
    ) -> Self {
        assert!(
            !partitions.is_empty(),
            "PartitionRegistry::build requires at least one partition; \
             normalize the empty case with normalize_partitions"
        );
        let default_partition = partitions[0].id;
        let mut by_id = std::collections::HashMap::new();
        let mut by_nic: std::collections::HashMap<Option<String>, Vec<PartitionId>> =
            std::collections::HashMap::new();
        for p in partitions {
            by_nic.entry(p.nic.clone()).or_default().push(p.id);
            let make_stack = make_stack_for(p.id, &p.spawner);
            let state = std::sync::Arc::new(PartitionState {
                id: p.id,
                spawner: p.spawner,
                nic: p.nic,
                authorities: std::sync::Mutex::new(std::collections::HashMap::new()),
                make_stack,
                peer_cursor: std::sync::atomic::AtomicUsize::new(0),
            });
            if by_id.insert(p.id, state).is_some() {
                panic!("duplicate PartitionId declared: {:?}", p.id);
            }
        }
        Self {
            by_id,
            by_nic,
            default_partition,
        }
    }

    /// Resolve the default partition (first declared or anonymous).
    pub(crate) fn default_partition(&self) -> std::sync::Arc<PartitionState> {
        self.by_id
            .get(&self.default_partition)
            .expect("default partition exists")
            .clone()
    }

    /// Resolve a declared partition by id. Panics if the id was not
    /// declared (programming error: the caller declared the topology).
    pub(crate) fn partition(&self, id: PartitionId) -> std::sync::Arc<PartitionState> {
        self.by_id
            .get(&id)
            .unwrap_or_else(|| panic!("partition not declared: {:?}", id))
            .clone()
    }

    /// Iterate all declared partitions.
    pub(crate) fn partitions(&self) -> impl Iterator<Item = &std::sync::Arc<PartitionState>> {
        self.by_id.values()
    }

    /// Resolve a partition by id without panicking. `None` if not declared.
    pub(crate) fn partition_opt(&self, id: PartitionId) -> Option<&std::sync::Arc<PartitionState>> {
        self.by_id.get(&id)
    }

    /// Partition ids sharing `id`'s NIC group, excluding `id` itself.
    ///
    /// Reclaim is NIC-blind for the freed *permit* (P0 connects on its own
    /// NIC), but candidate peers are still drawn from the same NIC group:
    /// the registry only groups by NIC, and a freed permit from any
    /// same-group peer is equivalent. Empty if `id` is alone in its group
    /// (e.g. the single-partition default).
    pub(crate) fn nic_group_peers(&self, id: PartitionId) -> Vec<PartitionId> {
        let nic = match self.by_id.get(&id) {
            Some(state) => &state.nic,
            None => return Vec::new(),
        };
        self.by_nic
            .get(nic)
            .map(|ids| ids.iter().copied().filter(|p| *p != id).collect())
            .unwrap_or_default()
    }

    /// Attempt to reclaim one idle connection from `peer`'s entry for
    /// `key`, freeing its permit. Returns `true` if one was freed. The
    /// peer's `authorities` lock is held only for the `try_reclaim_one`
    /// call (which pops + drops under the cache lock, releasing it before
    /// the drop). No-op `false` if the peer or entry is absent.
    pub(crate) fn try_reclaim_on(&self, peer: PartitionId, key: &super::PoolKey) -> bool {
        let state = match self.by_id.get(&peer) {
            Some(s) => s,
            None => return false,
        };
        let auth = state.authorities.lock().expect("authorities poisoned");
        match auth.get(key) {
            Some(entry) => entry.try_reclaim_one(),
            None => false,
        }
    }

    /// Attempt to reclaim one idle connection from *any* of `peer`'s
    /// entries, freeing its permit. Returns `true` at the first entry that
    /// yields. Drives the `Global` constraint, where the freed permit is
    /// fungible across authorities — so the specific authority does not
    /// matter, and this sidesteps reconstructing a `PoolKey` (scheme +
    /// authority) from the authority-only stats index. No-op `false` if
    /// the peer is absent or holds no reclaimable idle.
    pub(crate) fn try_reclaim_any(&self, peer: PartitionId) -> bool {
        let state = match self.by_id.get(&peer) {
            Some(s) => s,
            None => return false,
        };
        let auth = state.authorities.lock().expect("authorities poisoned");
        auth.values().any(|entry| entry.try_reclaim_one())
    }

    /// Attempt to borrow one idle connection from `peer`'s entry for
    /// `key`, as a dispatchable handle that returns to `peer`'s pool on
    /// drop. Returns `None` if the peer or entry is absent, or holds no
    /// borrowable idle. The peer's `authorities` lock is held only to
    /// check out the handle (the handle re-pools on drop, independent of
    /// the lock); dispatch happens after the lock is released.
    pub(crate) fn try_borrow_on(
        &self,
        peer: PartitionId,
        key: &super::PoolKey,
    ) -> Option<Box<dyn super::DispatchConn>> {
        let state = self.by_id.get(&peer)?;
        let auth = state.authorities.lock().expect("authorities poisoned");
        auth.get(key)?.try_borrow_one()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_id_from_index() {
        assert_eq!(PartitionId::from_index(0).as_u64(), 0);
        assert_eq!(PartitionId::from_index(5).as_u64(), 5);
        assert_eq!(PartitionId::from_index(0), PartitionId::from_index(0));
        assert_ne!(PartitionId::from_index(0), PartitionId::from_index(1));
    }

    #[test]
    fn partition_id_default_is_anonymous() {
        assert_eq!(PartitionId::default().as_u64(), u64::MAX);
        assert_ne!(PartitionId::default(), PartitionId::from_index(0));
    }

    #[test]
    fn cross_partition_policy_default_is_never() {
        assert_eq!(CrossPartitionPolicy::default(), CrossPartitionPolicy::Never);
    }

    #[tokio::test]
    async fn normalize_partitions_synthesizes_anonymous_when_empty() {
        // Empty input → exactly one anonymous partition on the supplied spawner.
        let parts = normalize_partitions(Vec::new(), || {
            std::sync::Arc::new(TokioDriverSpawner::current()) as std::sync::Arc<dyn DriverSpawner>
        });
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].id, PartitionId::default());
        assert!(parts[0].nic.is_none());
    }

    #[tokio::test]
    async fn normalize_partitions_passes_declared_set_through_untouched() {
        // Non-empty input is returned unchanged, and the anonymous-spawner
        // closure is never invoked.
        let declared = vec![
            Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current()),
            Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current()),
        ];
        let parts = normalize_partitions(declared, || {
            panic!("anonymous spawner must not be called when partitions are declared")
        });
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].id, PartitionId::from_index(0));
        assert_eq!(parts[1].id, PartitionId::from_index(1));
    }

    #[tokio::test]
    async fn tokio_driver_spawner_current() {
        let _ = TokioDriverSpawner::current();
    }

    #[tokio::test]
    async fn tokio_driver_spawner_from_handle() {
        let h = tokio::runtime::Handle::current();
        let _ = TokioDriverSpawner::from_handle(h);
    }

    #[tokio::test]
    async fn tokio_driver_spawner_runs_future() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let sp = TokioDriverSpawner::current();
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        sp.spawn(Box::pin(async move {
            f.store(true, Ordering::SeqCst);
        }));
        // Yield enough times for the spawned task to run.
        for _ in 0..10 {
            tokio::task::yield_now().await;
            if flag.load(Ordering::SeqCst) {
                break;
            }
        }
        assert!(flag.load(Ordering::SeqCst), "spawned future did not run");
    }
}

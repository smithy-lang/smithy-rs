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
/// A driver is the task that owns a connection's I/O. The spawner
/// determines which runtime executes that task. Per-runtime spawners
/// allow each connection's I/O to remain on a specific runtime.
///
/// Construct via [`DriverSpawner::current_tokio`] (captures the
/// current tokio runtime handle) or [`DriverSpawner::from_tokio_handle`]
/// (uses a specific runtime handle).
#[derive(Clone)]
pub struct DriverSpawner {
    inner: SpawnerInner,
}

#[derive(Clone)]
enum SpawnerInner {
    TokioHandle(tokio::runtime::Handle),
}

impl DriverSpawner {
    /// Spawner that uses the current tokio runtime.
    ///
    /// Captures the runtime handle eagerly at the call site. Panics
    /// if invoked outside a tokio runtime context.
    pub fn current_tokio() -> Self {
        Self::from_tokio_handle(tokio::runtime::Handle::current())
    }

    /// Spawner targeting the given tokio runtime handle.
    pub fn from_tokio_handle(handle: tokio::runtime::Handle) -> Self {
        Self {
            inner: SpawnerInner::TokioHandle(handle),
        }
    }
}

impl std::fmt::Debug for DriverSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let variant = match &self.inner {
            SpawnerInner::TokioHandle(_) => "TokioHandle",
        };
        f.debug_tuple("DriverSpawner").field(&variant).finish()
    }
}

/// Policy governing checkout when the local partition has no idle
/// connection and the pool is at capacity.
///
/// Cross-partition borrowing applies within a NIC group only.
/// Connections bound to different NICs are never shared regardless
/// of policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
    async fn driver_spawner_current_tokio() {
        let _ = DriverSpawner::current_tokio();
    }

    #[tokio::test]
    async fn driver_spawner_from_handle() {
        let h = tokio::runtime::Handle::current();
        let _ = DriverSpawner::from_tokio_handle(h);
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Partition-aware HTTP connection pooling.
//!
//! [`ConnectionPool`] owns a fixed set of [`Partition`]s. A [`Client`] selects
//! one partition, and each request resolves its URI to that partition's stable
//! origin cell. The cell is the local synchronization domain for acquisition,
//! reusable protocol state, and connection return:
//!
//! ```text
//! ConnectionPool
//!   `-- PartitionRegistry
//!         |-- PartitionState(owner runtime, optional interface)
//!         |     `-- OriginCell(origin)
//!         |           |-- acquisition waiters
//!         |           `-- source-owned H1 records
//!         `-- OriginAdmission(origin, optional bound)
//!               |-- capacity and demand order
//!               `-- cross-cell H1 source claims
//! ```
//!
//! A local idle H1 sender is the request fast path. A miss registers one
//! waiter that remains authoritative while establishment races a returning
//! sender. For a bounded origin, `OriginAdmission` grants connection
//! capacity across every partition. It may also borrow an eligible peer's H1
//! sender or reclaim an ineligible peer's idle connection so the waiting
//! partition can establish without exceeding the origin-wide limit.
//!
//! One `DeliveryGuard` carries either capacity or a claimed H1 sender from
//! admission to a target cell. It materializes all fallible source state
//! before reserving the target waiter, and its drop fallback restores the
//! payload and demand fence. Pool coordination never holds two pool locks at
//! once.
//!
//! Established connections separate three lifetimes:
//!
//! ```text
//! ConnectionState
//!   |-- logical close ----------> reject dispatch and release bounded capacity
//!   |-- DispatchGuard ----------> one accepted request
//!   `-- PhysicalConnectionGuard -> root transport I/O
//! ```
//!
//! Connector execution, handshakes, protocol drivers, pending H1 return, and
//! maintenance run through the owning partition's [`DriverSpawner`]. Borrowing
//! a sender moves dispatch authority only; its socket and driver stay on the
//! source partition.

#![cfg_attr(smithy_http_client_loom, allow(dead_code))]

mod admission;
mod builder;
mod cell;
mod client;
mod connection;
mod dispatch;
mod handshake;
mod maintenance;
mod origin;
mod partition;
mod registry;

pub use builder::{BuildError, Builder};
pub use client::{Client, InvalidPartition};
pub use connection::{CloseReason, ConnectionId};
pub use origin::{InvalidOrigin, OriginKey};
#[cfg(feature = "rt-tokio")]
pub use partition::TokioDriverSpawner;
pub use partition::{ConnectionReuseScope, DriverSpawner, Partition, PartitionId};

use handshake::TransportFactory;
use registry::PartitionRegistry;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc as StdArc;
use std::time::Duration;

/// A connection pool shared by one or more partition-specific clients.
///
/// Build a pool with [`ConnectionPool::builder`], then resolve a [`Client`]
/// for its anonymous or explicitly declared partition.
#[derive(Clone)]
pub struct ConnectionPool {
    inner: StdArc<PoolInner>,
}

impl ConnectionPool {
    /// Creates a connection-pool builder.
    pub fn builder() -> Builder<super::TlsUnset> {
        Builder::default()
    }
}

impl fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("partitions", &self.inner.registry)
            .field("idle_timeout", &self.inner.config.idle_timeout)
            .field(
                "max_connections_per_host",
                &self.inner.config.max_connections_per_host,
            )
            .field("reuse_scope", &self.inner.config.reuse_scope)
            .finish_non_exhaustive()
    }
}

/// Immutable pool policy retained with the partition registry.
#[derive(Clone, Debug)]
struct PoolConfig {
    idle_timeout: Option<Duration>,
    max_connections_per_host: Option<NonZeroUsize>,
    reuse_scope: ConnectionReuseScope,
}

/// Shared implementation state behind [`ConnectionPool`].
struct PoolInner {
    config: PoolConfig,
    registry: PartitionRegistry,
    transport: StdArc<dyn TransportFactory>,
    next_connection_id: AtomicU64,
}

impl Drop for PoolInner {
    fn drop(&mut self) {
        self.registry.close_all(CloseReason::PoolDropped);
    }
}

impl fmt::Debug for PoolInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolInner")
            .field("config", &self.config)
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

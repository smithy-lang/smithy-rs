/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP connection pooling across runtime and network partitions.
//!
//! A [`ConnectionPool`] owns connection policy and a fixed set of partitions.
//! A [`Client`] is a cheap handle resolved to one of those partitions. A pool
//! built without explicit [`Partition`] values has one anonymous partition;
//! otherwise clients select an explicit partition by [`PartitionId`].
//!
//! A partition determines where connections are established, driven, and
//! maintained. [`ConnectionReuseScope`] determines which partitions may send
//! requests through each other's connections. Reuse never moves a connection's
//! socket or protocol driver between runtimes.
//!
//! # State ownership
//!
//! Pool state is divided by the scope of each decision:
//!
//! ```text
//! ConnectionPool
//!   `-- PoolInner
//!         |-- PartitionRegistry
//!         |     |-- PartitionState per partition
//!         |     |     |-- OriginCell per canonical origin
//!         |     |     `-- runtime placement and idle maintenance
//!         |     `-- OriginAdmission per bounded origin
//!         `-- immutable policy and transport construction
//! ```
//!
//! An `OriginCell` is the only lock domain that combines a partition's local
//! acquisition order with its protocol connections. For a bounded origin,
//! `OriginAdmission` separately owns the origin-wide capacity limit, demand
//! order, and cross-cell reuse decisions. Values crossing between a cell and
//! admission own their payload and cancellation fallback; pool coordination
//! never holds both locks at once.
//!
//! An HTTP/1 sender is Hyper's exclusive `SendRequest<SdkBody>` request
//! handle. It is the authority to send over one
//! HTTP/1 connection, not the socket or the protocol driver.
//!
//! # Request flow
//!
//! Each request follows the same ownership path:
//!
//! ```text
//! Client(partition)
//!   `-- resolve OriginCell(partition, origin)
//!         |-- select a local idle HTTP/1 sender
//!         `-- register one waiter
//!               |-- establish with new capacity
//!               |-- borrow an eligible partition's sender
//!               `-- reclaim capacity from an ineligible partition
//!                         |
//!                         v
//!                    H1Selection
//!                         |
//!                  commit dispatch
//!                         |
//!                    H1Exchange
//!                         |
//!               complete response body
//!                         |
//!                   H1ReturnOffer
//!                         |
//!                 connection-owning cell
//! ```
//!
//! A local idle sender is selected without origin-wide coordination. On a
//! miss, one waiter remains authoritative while capacity, a returning sender,
//! and connection establishment race to satisfy it. A bounded origin may lend
//! an eligible partition's sender or close an ineligible idle connection and
//! transfer its capacity. Active connections are not reclaimed.
//!
//! Dispatch commits against logical close before Hyper receives the request.
//! An accepted response retains the sender until Hyper proves a complete,
//! reusable HTTP/1 message boundary. Errors retire the connection. A protocol
//! upgrade logically closes the pool record before upgraded root I/O is
//! exposed to the caller.
//!
//! `ConnectionState` separates logical close, accepted-request accounting,
//! and root-I/O ownership. Logical close rejects new dispatch and releases
//! bounded capacity. `DispatchGuard` follows one accepted request.
//! `PhysicalConnectionGuard` follows the root transport until the pool no
//! longer owns that I/O; it does not describe the kernel's TCP state.
//!
//! Connector execution, handshakes, protocol drivers, pending HTTP/1 return,
//! and idle maintenance run through the connection-owning partition's
//! [`DriverSpawner`].

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
pub use client::{Client, ClientBuildError};
pub use connection::{CloseReason, ConnectionId};
pub use origin::{InvalidOrigin, OriginKey};
#[cfg(feature = "rt-tokio")]
pub use partition::TokioDriverSpawner;
pub use partition::{ConnectionReuseScope, DriverSpawner, Partition, PartitionId};

use crate::sync::Arc;
use handshake::TransportFactory;
use registry::PartitionRegistry;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc as StdArc;
use std::time::Duration;

/// Shared owner of connection topology, reusable connections, and pool policy.
///
/// Cloning a pool shares its connections. Create a [`Client`] to bind requests
/// to the anonymous partition or to an explicit [`PartitionId`]. A pool built
/// with explicit partitions does not also create an anonymous partition.
///
/// Dropping the final owner of the shared pool state logically closes retained
/// connections and stops partition maintenance.
#[derive(Clone)]
pub struct ConnectionPool {
    /// Shared pool policy, topology, connector, and connection-ID allocator.
    inner: Arc<PoolInner>,
}

impl ConnectionPool {
    /// Returns a builder for a new connection pool.
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
    /// Duration after which a reusable idle connection is retired.
    idle_timeout: Option<Duration>,
    /// Optional logical-connection bound shared by every partition for one origin.
    max_connections_per_host: Option<NonZeroUsize>,
    /// Partitions allowed to dispatch through one another's connections.
    reuse_scope: ConnectionReuseScope,
}

/// Shared implementation state behind [`ConnectionPool`].
struct PoolInner {
    /// Immutable settings shared by pool operations.
    config: PoolConfig,
    /// Fixed partitions and lazily published per-origin state.
    registry: PartitionRegistry,
    /// Type-erased construction of one partition-bound transport.
    transport: StdArc<dyn TransportFactory>,
    /// Monotonic identity source shared by every physical connection.
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

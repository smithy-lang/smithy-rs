/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pooling with explicit runtime and network placement.
//!
//! A [`ConnectionPool`] owns connection policy and a fixed set of partitions.
//! A [`Client`] binds Smithy HTTP operations to one partition. Pools without
//! explicit [`Partition`] values contain one anonymous partition; otherwise a
//! client selects a declared partition by [`PartitionId`].
//!
//! A partition owns connection establishment, protocol drivers, and idle
//! maintenance. [`ConnectionReuseScope`] controls whether another partition may
//! dispatch through those connections. Reuse transfers protocol dispatch
//! authority; it never moves the socket, driver, or capacity accounting.
//!
//! # State ownership
//!
//! ```text
//! ConnectionPool
//! `-- PoolInner
//!     |-- immutable policy and transport factory
//!     `-- PartitionRegistry
//!         |-- PartitionState per partition
//!         |   |-- runtime placement and idle maintenance
//!         |   `-- OriginCell per canonical origin
//!         |       |-- acquisition queue
//!         |       `-- protocol connection records
//!         `-- OriginAdmission per bounded origin
//!             |-- connection permits
//!             |-- cross-cell demand order
//!             `-- peer reuse indexes
//! ```
//!
//! One `OriginCell` lock owns local acquisition order and protocol residence.
//! For a bounded origin, `OriginAdmission` separately owns the origin-wide
//! connection limit and cross-cell scheduling. Delivery values carry their own
//! payload and cancellation fallback between those lock domains; cell and
//! admission locks are never held together.
//!
//! # HTTP/1 request lifecycle
//!
//! Hyper represents an HTTP/1 connection with one exclusive
//! `SendRequest<SdkBody>` handle. This module calls that handle the sender. The
//! sender authorizes request dispatch but does not own the socket or protocol
//! driver.
//!
//! ```text
//! Client(partition, request)
//! `-- OriginCell(partition, origin)
//!     |-- local idle sender --------------------------> H1Selection
//!     `-- acquisition queue
//!         |-- returned local or eligible peer sender -> H1Selection
//!         |-- capacity permit -> establish HTTP/1 ---> H1Selection
//!         `-- reclaimed peer capacity -> establish ---> H1Selection
//!
//! H1Selection -- Hyper accepts request --> H1Exchange
//! H1Exchange
//!     |-- complete response + ready --> H1ReturnOffer --> owning OriginCell
//!     `-- failure, cancellation, or upgrade ----------> retire pool record
//! ```
//!
//! A local hit touches only the cell lock. On a miss, one queued acquisition
//! remains authoritative while a returned sender and establishment race to
//! satisfy it. Bounded origins may borrow an eligible peer sender or reclaim a
//! peer connection and transfer its permit; active connections are not
//! reclaimed.
//!
//! Dispatch commits against logical close before Hyper receives the request.
//! Once accepted, the response lifecycle retains the sender until Hyper proves
//! a complete reusable message boundary. Failure retires the connection, and
//! an upgrade closes the pool record before exposing upgraded root I/O.
//!
//! `ConnectionState` separates logical close, accepted-request accounting, and
//! root-I/O ownership. Logical close rejects new dispatch and releases bounded
//! capacity. `DispatchGuard` follows an accepted request, while
//! `PhysicalConnectionGuard` follows root I/O until the pool no longer owns
//! that transport; neither describes the operating system TCP state. All
//! connection-owned work runs through the partition [`DriverSpawner`].

#![cfg_attr(smithy_http_client_loom, allow(dead_code))]

mod admission;
mod builder;
mod cell;
mod client;
mod connection;
mod dispatch;
mod establish;
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
use establish::TransportFactory;
use registry::PartitionRegistry;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc as StdArc;
use std::time::Duration;

/// Shared connection topology and pooling policy.
///
/// Construct a pool with [`ConnectionPool::builder`], then create [`Client`]
/// handles for its anonymous or explicit partitions. The pool itself owns no
/// request placement; each client supplies that partition choice. Cloning a
/// pool or client shares all retained connections and admission state.
///
/// A pool built with explicit partitions does not also create an anonymous
/// partition. Dropping the final shared owner logically closes retained
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

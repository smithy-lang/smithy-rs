/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Partition-aware HTTP connection pooling.
//!
//! A [`Partition`] fixes where connections are established and driven. Pool
//! state for one partition and one [`OriginKey`] meets in an `OriginCell`.
//! Protocol-specific selection and establishment use that cell as follows:
//!
//! ```text
//! request (partition P, URI)
//!   |
//!   | resolve canonical origin in PartitionRegistry
//!   v
//! OriginCell(P, origin)
//!   |
//!   +-- local H1 available ----------------------------> H1Selection
//!   |
//!   `-- miss -> acquisition waiter <---------- returning H1
//!               |
//!               +-- unbounded ----------------> establishment permit
//!               |
//!               `-- bounded -> DemandSnapshot -> OriginAdmission
//!                                                   |
//!                                            CapacityDelivery
//!                                                   |
//!                                            establishment permit
//!               |
//!               `-- establishment and returning H1 race
//!                            |
//!                            `----------------------> terminal H1 or error
//! ```
//!
//! `OriginAdmission` exists only when an origin has a connection limit. It is
//! not another connection cache: cells retain protocol state, while admission
//! decides which cell may consume the next connection slot. A
//! [`ConnectionReuseScope`] determines which cells are eligible to relieve
//! each other's demand without moving connection I/O or its protocol driver.
//! Capacity starts establishment but does not complete the acquisition waiter;
//! that same waiter remains until a returned H1 or the attempt result wins.
//!
//! An installed connection tracks reuse eligibility, committed requests, and
//! root I/O independently:
//!
//! ```text
//! ConnectionState
//!   |-- logical_close ------> stop accepting work + return bounded capacity
//!   |-- try_commit_dispatch -> DispatchGuard -------> request completion
//!   `-- PhysicalConnectionGuard --------------------> root I/O is gone
//! ```
//!
//! Logical close releases the connection slot without waiting for committed
//! requests or transport teardown. Those remaining lifetimes retain the
//! shared `ConnectionState` until their own terminal transitions.

// TODO(pool): Revisit this overview once protocol records and public construction are present.

#![allow(
    dead_code,
    reason = "TODO(pool): remove when protocol paths consume the pool internals"
)]

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

use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_async::time::SharedTimeSource;
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
    time_source: SharedTimeSource,
    sleep_impl: Option<SharedAsyncSleep>,
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

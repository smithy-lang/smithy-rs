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
//!   +-- local connection available --------------------> ConnectionState -> dispatch
//!   |
//!   +-- unbounded miss --------------------------------> establish -> ConnectionState
//!   |
//!   `-- bounded miss -> local waiter -> DemandSnapshot
//!                                      |
//!                                      v
//!                               OriginAdmission
//!                          (capacity and cell order)
//!                                      |
//!                               CapacityDelivery
//!                                      |
//!                                      v
//!                         waiter receives CapacityLease
//!                                      |
//!                               establish -> ConnectionState
//! ```
//!
//! `OriginAdmission` exists only when an origin has a connection limit. It is
//! not another connection cache: cells retain protocol state, while admission
//! decides which cell may consume the next connection slot. A
//! [`ConnectionReuseScope`] determines which cells are eligible to relieve
//! each other's demand without moving connection I/O or its protocol driver.
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
mod cell;
mod connection;
mod origin;
mod partition;
mod registry;

pub use connection::{CloseReason, ConnectionId};
pub use origin::{InvalidOrigin, OriginKey};
#[cfg(feature = "rt-tokio")]
pub use partition::TokioDriverSpawner;
pub use partition::{ConnectionReuseScope, DriverSpawner, Partition, PartitionId};

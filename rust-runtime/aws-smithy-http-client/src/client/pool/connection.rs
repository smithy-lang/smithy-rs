/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol-independent connection lifetime.
//!
//! [`ConnectionState`] serializes dispatch commitment with logical close.
//! [`DispatchGuard`] accounts for accepted requests, while
//! [`PhysicalConnectionGuard`] follows root I/O until the transport is gone.
//! Logical close returns bounded capacity without waiting for either lifetime
//! to finish.

use super::admission::CapacityLease;
use super::partition::PartitionId;
use crate::sync::{Arc, Mutex};
pub use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::fmt;

/// Why a connection stopped accepting new work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CloseReason {
    /// The connection exceeded its configured idle timeout.
    IdleTimeout,
    /// The connection was explicitly marked unsafe for reuse.
    Poisoned,
    /// The protocol driver or peer closed the connection.
    ProtocolClosed,
    /// HTTP/1 did not prove a complete reusable message boundary.
    IncompleteH1Exchange,
    /// The transport left HTTP/1 pool ownership through an upgrade.
    Upgraded,
    /// The connection closed to move bounded capacity to another cell.
    Reclaimed,
    /// The connection pool was dropped.
    PoolDropped,
    /// The runtime driving the connection shut down.
    OwnerRuntimeShutdown,
}

/// Shared protocol-independent ownership for one installed connection.
pub(super) struct ConnectionState {
    /// Stable identity shared with metadata and lifecycle events.
    id: ConnectionId,
    /// Partition that retains the connection's I/O and protocol driver.
    owner_partition: PartitionId,
    /// Dispatch, logical-close, and root-I/O completion state.
    lifecycle: Mutex<LifecycleState>,
}

impl ConnectionState {
    /// Creates a connection whose origin has no admission bound.
    ///
    /// The returned guard is the unique physical-lifetime owner and must move
    /// with the root I/O task.
    pub(super) fn unbounded(
        id: ConnectionId,
        owner_partition: PartitionId,
    ) -> (Arc<Self>, PhysicalConnectionGuard) {
        Self::new(id, owner_partition, None)
    }

    /// Creates a connection that takes ownership of one bounded-origin slot.
    ///
    /// The returned guard is the unique physical-lifetime owner and must move
    /// with the root I/O task. Logical close returns `lease` independently of
    /// that guard.
    pub(super) fn bounded(
        id: ConnectionId,
        owner_partition: PartitionId,
        lease: CapacityLease,
    ) -> (Arc<Self>, PhysicalConnectionGuard) {
        Self::new(id, owner_partition, Some(lease))
    }

    /// Builds shared connection state and its unique physical-lifetime guard.
    fn new(
        id: ConnectionId,
        owner_partition: PartitionId,
        lease: Option<CapacityLease>,
    ) -> (Arc<Self>, PhysicalConnectionGuard) {
        let connection = Arc::new(Self {
            id,
            owner_partition,
            lifecycle: Mutex::new(LifecycleState {
                logical: LogicalState::Open { lease },
                in_flight: 0,
                physical_complete: false,
            }),
        });
        let physical = PhysicalConnectionGuard {
            connection: connection.clone(),
            active: true,
        };
        (connection, physical)
    }

    /// Returns this connection's stable identity.
    pub(super) fn id(&self) -> ConnectionId {
        self.id
    }

    /// Returns the partition that owns this connection's I/O and driver.
    pub(super) fn owner_partition(&self) -> PartitionId {
        self.owner_partition
    }

    /// Attempts to commit one request against logical close.
    ///
    /// Returns a guard and increments the in-flight count while the connection
    /// is open. Returns `None` without changing state after logical close.
    pub(super) fn try_commit_dispatch(connection: &Arc<Self>) -> Option<DispatchGuard> {
        let mut lifecycle = connection.lifecycle.lock();
        if !matches!(lifecycle.logical, LogicalState::Open { .. }) {
            return None;
        }
        lifecycle.in_flight = lifecycle
            .in_flight
            .checked_add(1)
            .expect("in-flight dispatch count exhausted");
        drop(lifecycle);

        Some(DispatchGuard {
            connection: connection.clone(),
            active: true,
        })
    }

    /// Performs the first logical-close transition.
    ///
    /// Returns `true` when this call closes the connection and records
    /// `reason`. Returns `false` when another close already won; the original
    /// reason remains unchanged.
    ///
    /// The detached lease is dropped only after the connection lock is
    /// released, so admission and connection locks are never nested.
    pub(super) fn logical_close(&self, reason: CloseReason) -> bool {
        let lease = {
            let mut lifecycle = self.lifecycle.lock();
            let LogicalState::Open { lease } = &mut lifecycle.logical else {
                return false;
            };
            let lease = lease.take();
            lifecycle.logical = LogicalState::Closed { reason };
            lease
        };
        drop(lease);
        true
    }

    /// Removes one dispatch previously committed by [`Self::try_commit_dispatch`].
    fn finish_dispatch(&self) {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.in_flight = lifecycle
            .in_flight
            .checked_sub(1)
            .expect("completed a dispatch that was not in flight");
    }

    /// Records that the connection's root I/O is no longer live.
    ///
    /// # Panics
    ///
    /// Panics if physical ownership completes more than once.
    fn finish_physical(&self) {
        let mut lifecycle = self.lifecycle.lock();
        assert!(
            !lifecycle.physical_complete,
            "physical connection ownership completed more than once"
        );
        lifecycle.physical_complete = true;
    }

    /// Returns a consistent lifecycle snapshot.
    pub(super) fn snapshot(&self) -> ConnectionSnapshot {
        let lifecycle = self.lifecycle.lock();
        ConnectionSnapshot {
            close_reason: match lifecycle.logical {
                LogicalState::Open { .. } => None,
                LogicalState::Closed { reason } => Some(reason),
            },
            in_flight: lifecycle.in_flight,
            physical_complete: lifecycle.physical_complete,
        }
    }
}

impl fmt::Debug for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionState")
            .field("id", &self.id)
            .field("owner_partition", &self.owner_partition)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

/// Connection lifetime state serialized with dispatch commitment and close.
#[derive(Debug)]
struct LifecycleState {
    /// Dispatch eligibility and ownership of bounded capacity.
    logical: LogicalState,
    /// Requests that committed before logical close.
    in_flight: usize,
    /// Whether ownership of root transport I/O has ended.
    physical_complete: bool,
}

/// Whether a connection may accept dispatch and still owns bounded capacity.
#[derive(Debug)]
enum LogicalState {
    /// Dispatch may commit; the optional lease is released by logical close.
    Open { lease: Option<CapacityLease> },
    /// New dispatch is rejected while existing work may still drain.
    Closed { reason: CloseReason },
}

/// Observable lifecycle state used by protocol coordination and focused tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConnectionSnapshot {
    /// Reason logical close won, or `None` while dispatch is accepted.
    pub(super) close_reason: Option<CloseReason>,
    /// Number of dispatches that have not completed.
    pub(super) in_flight: usize,
    /// Whether root-I/O ownership has completed.
    pub(super) physical_complete: bool,
}

/// One dispatch that committed before logical close.
///
/// Dropping the guard records request completion exactly once.
#[derive(Debug)]
pub(super) struct DispatchGuard {
    /// Shared state whose in-flight count this guard owns.
    connection: Arc<ConnectionState>,
    /// Whether `Drop` still owes request completion.
    active: bool,
}

impl DispatchGuard {
    /// Returns the identity of the connection carrying this dispatch.
    pub(super) fn connection_id(&self) -> ConnectionId {
        self.connection.id
    }

    /// Consumes the guard and records request completion immediately.
    ///
    /// Dropping an uncompleted guard performs the same accounting as a
    /// cancellation fallback.
    pub(super) fn complete(mut self) {
        self.active = false;
        self.connection.finish_dispatch();
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        if self.active {
            self.connection.finish_dispatch();
        }
    }
}

/// Unique root-I/O ownership whose drop records physical completion.
///
/// The guard is created with the connection and moves with root I/O through
/// protocol drain or upgrade.
#[derive(Debug)]
pub(super) struct PhysicalConnectionGuard {
    /// Shared state whose physical lifetime this guard owns.
    connection: Arc<ConnectionState>,
    /// Whether `Drop` still owes physical completion.
    active: bool,
}

impl PhysicalConnectionGuard {
    /// Consumes the guard and records root-I/O completion immediately.
    ///
    /// Dropping an uncompleted guard performs the same transition during task
    /// cancellation or runtime shutdown.
    pub(super) fn complete(mut self) {
        self.active = false;
        self.connection.finish_physical();
    }
}

impl Drop for PhysicalConnectionGuard {
    fn drop(&mut self) {
        if self.active {
            self.connection.finish_physical();
        }
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::OriginAdmission;
    use std::num::NonZeroUsize;

    #[test]
    fn logical_close_releases_capacity_before_physical_completion() {
        let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
        let lease = OriginAdmission::lease_for_test(&origin);
        let (connection, physical) =
            ConnectionState::bounded(ConnectionId::new(1), PartitionId::from_index(0), lease);

        assert!(connection.logical_close(CloseReason::Reclaimed));
        assert!(!connection.logical_close(CloseReason::PoolDropped));
        assert_eq!(1, origin.available_capacity_for_test());
        assert!(!connection.snapshot().physical_complete);
        assert_eq!(
            Some(CloseReason::Reclaimed),
            connection.snapshot().close_reason
        );

        drop(physical);
        assert!(connection.snapshot().physical_complete);
    }

    #[test]
    fn committed_dispatch_drains_after_logical_close() {
        let (connection, _physical) =
            ConnectionState::unbounded(ConnectionId::new(1), PartitionId::from_index(0));
        let dispatch = ConnectionState::try_commit_dispatch(&connection).unwrap();
        assert_eq!(ConnectionId::new(1), dispatch.connection_id());

        assert!(connection.logical_close(CloseReason::ProtocolClosed));
        assert!(ConnectionState::try_commit_dispatch(&connection).is_none());
        assert_eq!(1, connection.snapshot().in_flight);

        dispatch.complete();
        assert_eq!(0, connection.snapshot().in_flight);
    }

    #[test]
    fn physical_guard_is_created_once_with_the_connection() {
        let (connection, physical) =
            ConnectionState::unbounded(ConnectionId::new(1), PartitionId::from_index(0));
        assert_eq!(ConnectionId::new(1), connection.id());
        assert_eq!(PartitionId::from_index(0), connection.owner_partition());
        assert!(!connection.snapshot().physical_complete);
        physical.complete();
        assert!(connection.snapshot().physical_complete);
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use crate::client::pool::admission::OriginAdmission;
    use std::num::NonZeroUsize;

    #[test]
    fn dispatch_commit_linearizes_against_close() {
        loom::model(|| {
            let (connection, _physical) =
                ConnectionState::unbounded(ConnectionId::new(1), PartitionId::from_index(0));

            let dispatch_connection = connection.clone();
            let dispatch = loom::thread::spawn(move || {
                ConnectionState::try_commit_dispatch(&dispatch_connection)
            });
            let close_connection = connection.clone();
            let close = loom::thread::spawn(move || {
                close_connection.logical_close(CloseReason::ProtocolClosed)
            });

            let dispatch = dispatch.join().unwrap();
            assert!(close.join().unwrap());
            let snapshot = connection.snapshot();
            assert_eq!(Some(CloseReason::ProtocolClosed), snapshot.close_reason);
            assert!(ConnectionState::try_commit_dispatch(&connection).is_none());
            match dispatch {
                Some(dispatch) => {
                    assert_eq!(1, snapshot.in_flight);
                    drop(dispatch);
                    assert_eq!(0, connection.snapshot().in_flight);
                }
                None => assert_eq!(0, snapshot.in_flight),
            }
        });
    }

    #[test]
    fn concurrent_logical_close_releases_one_capacity_lease() {
        loom::model(|| {
            let origin = OriginAdmission::new(NonZeroUsize::new(1).unwrap());
            let lease = OriginAdmission::lease_for_test(&origin);
            let (connection, _physical) =
                ConnectionState::bounded(ConnectionId::new(1), PartitionId::from_index(0), lease);
            let first_connection = connection.clone();
            let first =
                loom::thread::spawn(move || first_connection.logical_close(CloseReason::Poisoned));
            let second_connection = connection.clone();
            let second = loom::thread::spawn(move || {
                second_connection.logical_close(CloseReason::PoolDropped)
            });

            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert_ne!(first, second);
            assert_eq!(1, origin.available_capacity_for_test());
            let reason = connection.snapshot().close_reason.unwrap();
            assert!(matches!(
                reason,
                CloseReason::Poisoned | CloseReason::PoolDropped
            ));
        });
    }
}

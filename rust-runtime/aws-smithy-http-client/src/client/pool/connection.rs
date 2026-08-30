/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol-independent connection lifetime.
//!
//! [`ConnectionState`] serializes dispatch commitment with logical close.
//! [`DispatchGuard`] accounts for accepted requests.
//! [`PhysicalConnectionGuard`] follows root I/O until the pool no longer owns
//! it, including transfer through a protocol upgrade. This physical transition
//! does not assert that the peer or kernel has completed the underlying TCP
//! close. Logical close returns bounded capacity without waiting for either
//! lifetime to finish.

use super::admission::CapacityLease;
use super::origin::OriginKey;
use super::partition::PartitionId;
use crate::sync::{Arc, Mutex};
pub use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
use http_1x::Extensions;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection, HttpInfo};
use pin_project_lite::pin_project;
use std::fmt;
use std::io::{self, IoSlice};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

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

/// Protocol selected for one installed physical connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NegotiatedProtocol {
    /// HTTP/1.1 with one exclusive request sender.
    Http1,
    /// HTTP/2 with a multiplexed request sender.
    #[allow(
        dead_code,
        reason = "constructed once HTTP/2 generations are implemented"
    )]
    Http2,
}

/// Immutable identity and transport facts shared by a connection's owners.
///
/// The protocol record, request metadata, tracing, and future lifecycle events
/// all retain this one allocation rather than reconstructing origin or address
/// data at each transition.
#[derive(Debug)]
pub(super) struct ConnectionInfo {
    /// Stable identity assigned by the owning pool.
    id: ConnectionId,
    /// Canonical origin this connection may serve.
    origin: OriginKey,
    /// Partition that retains the transport and protocol driver.
    owner_partition: PartitionId,
    /// Protocol selected by configuration or ALPN.
    #[allow(
        dead_code,
        reason = "read when HTTP/2 generation selection is implemented"
    )]
    protocol: NegotiatedProtocol,
    /// Local socket address reported by the connector, when available.
    local_addr: Option<SocketAddr>,
    /// Remote socket address reported by the connector, when available.
    remote_addr: Option<SocketAddr>,
    /// Whether the transport reaches the origin through a proxy.
    proxied: bool,
    /// Connector metadata copied into every response on this connection.
    connected: Connected,
}

impl ConnectionInfo {
    /// Captures immutable facts when protocol establishment succeeds.
    pub(super) fn new(
        id: ConnectionId,
        origin: OriginKey,
        owner_partition: PartitionId,
        protocol: NegotiatedProtocol,
        connected: Connected,
    ) -> Arc<Self> {
        let mut extras = Extensions::new();
        connected.get_extras(&mut extras);
        let http_info = extras.get::<HttpInfo>();
        Arc::new(Self {
            id,
            origin,
            owner_partition,
            protocol,
            local_addr: http_info.map(HttpInfo::local_addr),
            remote_addr: http_info.map(HttpInfo::remote_addr),
            proxied: connected.is_proxied(),
            connected,
        })
    }

    /// Returns the pool-assigned physical connection identity.
    pub(super) fn id(&self) -> ConnectionId {
        self.id
    }

    /// Returns the canonical origin this connection may serve.
    pub(super) fn origin(&self) -> &OriginKey {
        &self.origin
    }

    /// Returns the partition that owns this connection's I/O and driver.
    pub(super) fn owner_partition(&self) -> PartitionId {
        self.owner_partition
    }

    /// Returns the established HTTP protocol.
    #[cfg(test)]
    pub(super) fn protocol(&self) -> NegotiatedProtocol {
        self.protocol
    }

    /// Returns the connector-reported local socket address.
    #[cfg(test)]
    pub(super) fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Returns the connector-reported remote socket address.
    #[cfg(test)]
    pub(super) fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Returns whether the origin is reached through a proxy.
    pub(super) fn is_proxied(&self) -> bool {
        self.proxied
    }

    /// Copies connector-provided values into a response extension map.
    pub(super) fn apply_connector_extras(&self, extensions: &mut Extensions) {
        self.connected.get_extras(extensions);
    }

    /// Builds Smithy metadata with close authority for this H1 generation.
    pub(super) fn metadata(&self, close: super::cell::h1::H1CloseHandle) -> ConnectionMetadata {
        let mut builder = ConnectionMetadata::builder()
            .proxied(self.proxied)
            .connection_id(self.id)
            .poison_fn(move || {
                close.close(CloseReason::Poisoned);
            });
        builder
            .set_local_addr(self.local_addr)
            .set_remote_addr(self.remote_addr);
        builder.build()
    }

    /// Creates synthetic HTTP/1 information for state-machine tests.
    #[cfg(test)]
    pub(super) fn for_test(id: ConnectionId, owner_partition: PartitionId) -> Arc<Self> {
        Self::new(
            id,
            OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "example.com", None)
                .expect("synthetic test origin is valid"),
            owner_partition,
            NegotiatedProtocol::Http1,
            Connected::new(),
        )
    }
}

/// Shared protocol-independent ownership for one installed connection.
pub(super) struct ConnectionState {
    /// Identity and transport facts shared with metadata and lifecycle events.
    info: Arc<ConnectionInfo>,
    /// Dispatch, logical-close, and root-I/O completion state.
    lifecycle: Mutex<LifecycleState>,
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
    /// Dispatch may commit.
    Open {
        /// Bounded-origin slot released by logical close, when configured.
        lease: Option<CapacityLease>,
    },
    /// New dispatch is rejected while accepted work may still drain.
    Closed {
        /// Reason recorded by the first logical-close transition.
        reason: CloseReason,
    },
}

impl ConnectionState {
    /// Creates a connection whose origin has no admission bound.
    ///
    /// The returned guard is the unique physical-lifetime owner and must move
    /// with the root I/O task.
    pub(super) fn unbounded(info: Arc<ConnectionInfo>) -> (Arc<Self>, PhysicalConnectionGuard) {
        Self::new(info, None)
    }

    /// Creates a connection that takes ownership of one bounded-origin slot.
    ///
    /// The returned guard is the unique physical-lifetime owner and must move
    /// with the root I/O task. Logical close returns `lease` independently of
    /// that guard.
    pub(super) fn bounded(
        info: Arc<ConnectionInfo>,
        lease: CapacityLease,
    ) -> (Arc<Self>, PhysicalConnectionGuard) {
        Self::new(info, Some(lease))
    }

    /// Builds shared connection state and its unique physical-lifetime guard.
    fn new(
        info: Arc<ConnectionInfo>,
        lease: Option<CapacityLease>,
    ) -> (Arc<Self>, PhysicalConnectionGuard) {
        let connection = Arc::new(Self {
            info,
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
        self.info.id()
    }

    /// Returns the partition that owns this connection's I/O and driver.
    pub(super) fn owner_partition(&self) -> PartitionId {
        self.info.owner_partition()
    }

    /// Returns immutable identity and transport facts for this connection.
    pub(super) fn info(&self) -> &Arc<ConnectionInfo> {
        &self.info
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
        tracing::debug!(
            connection_id = %self.id(),
            connection_partition = ?self.owner_partition(),
            origin_scheme = %self.info.origin().scheme(),
            origin_host = self.info.origin().host(),
            origin_port = ?self.info.origin().port(),
            close_reason = ?reason,
            "connection logically closed"
        );
        true
    }

    /// Refines a driver-observed close after Hyper confirms an upgrade.
    ///
    /// Hyper may transfer upgraded I/O and complete its HTTP/1 driver in one
    /// poll. The driver can therefore record [`CloseReason::ProtocolClosed`]
    /// before the request task observes the upgrading response. This method
    /// changes that stored reason to [`CloseReason::Upgraded`].
    ///
    /// Returns `true` only when that refinement was applied. It does not close
    /// the connection again, change I/O ownership, or release capacity a second
    /// time.
    pub(super) fn refine_protocol_close_as_upgrade(&self) -> bool {
        let refined = {
            let mut lifecycle = self.lifecycle.lock();
            match &mut lifecycle.logical {
                LogicalState::Closed { reason } if *reason == CloseReason::ProtocolClosed => {
                    *reason = CloseReason::Upgraded;
                    true
                }
                _ => false,
            }
        };
        if refined {
            tracing::debug!(
                connection_id = %self.id(),
                connection_partition = ?self.owner_partition(),
                origin_scheme = %self.info.origin().scheme(),
                origin_host = self.info.origin().host(),
                origin_port = ?self.info.origin().port(),
                previous_close_reason = ?CloseReason::ProtocolClosed,
                close_reason = ?CloseReason::Upgraded,
                "refined connection close reason after HTTP/1 upgrade"
            );
        }
        refined
    }

    /// Verifies a close reason at a protocol handoff boundary in debug builds.
    #[cfg(debug_assertions)]
    pub(super) fn debug_assert_close_reason(&self, expected: CloseReason) {
        let lifecycle = self.lifecycle.lock();
        let actual = match lifecycle.logical {
            LogicalState::Open { .. } => None,
            LogicalState::Closed { reason } => Some(reason),
        };
        debug_assert_eq!(Some(expected), actual);
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
        {
            let mut lifecycle = self.lifecycle.lock();
            assert!(
                !lifecycle.physical_complete,
                "physical connection ownership completed more than once"
            );
            lifecycle.physical_complete = true;
        }
        tracing::debug!(
            connection_id = %self.id(),
            connection_partition = ?self.owner_partition(),
            origin_scheme = %self.info.origin().scheme(),
            origin_host = self.info.origin().host(),
            origin_port = ?self.info.origin().port(),
            "connection root I/O ownership ended"
        );
    }

    /// Returns a consistent lifecycle snapshot.
    #[cfg(test)]
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
            .field("info", &self.info)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

#[cfg(test)]
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
    #[cfg(test)]
    pub(super) fn connection_id(&self) -> ConnectionId {
        self.connection.id()
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

/// Unique root-I/O ownership whose drop records the end of pool ownership.
///
/// The guard is created with the connection and moves with root I/O through
/// protocol drain or upgrade. Completion says only that the pool no longer
/// owns the root transport; the operating system may continue TCP teardown.
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
    #[cfg(test)]
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

pin_project! {
    /// Root transport wrapper that tracks how long the pool owns the I/O.
    ///
    /// The wrapper moves intact through Hyper's driver and H1 upgrade path.
    /// Logical close may happen earlier. Dropping the wrapper records that pool
    /// ownership ended after the wrapped I/O was destroyed; it does not observe
    /// peer or kernel TCP completion.
    pub(super) struct ConnectionIo<T> {
        #[pin]
        inner: T,
        // Declared after `inner` so transport destruction precedes the
        // physical-completion signal.
        physical: PhysicalConnectionGuard,
    }
}

impl<T> ConnectionIo<T> {
    /// Attaches the unique physical-lifetime guard to root transport I/O.
    pub(super) fn new(inner: T, physical: PhysicalConnectionGuard) -> Self {
        Self { inner, physical }
    }

    /// Returns the wrapped transport.
    #[cfg(test)]
    pub(super) fn get_ref(&self) -> &T {
        &self.inner
    }
}

impl<T> fmt::Debug for ConnectionIo<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionIo")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<T> Connection for ConnectionIo<T>
where
    T: Connection,
{
    fn connected(&self) -> Connected {
        self.inner.connected()
    }
}

impl<T> Read for ConnectionIo<T>
where
    T: Read,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        Read::poll_read(self.project().inner, cx, buf)
    }
}

impl<T> Write for ConnectionIo<T>
where
    T: Write,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Write::poll_write(self.project().inner, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Write::poll_flush(self.project().inner, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Write::poll_shutdown(self.project().inner, cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Write::poll_write_vectored(self.project().inner, cx, bufs)
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::OriginAdmission;
    use std::num::NonZeroUsize;

    fn test_info(id: u64) -> Arc<ConnectionInfo> {
        ConnectionInfo::for_test(ConnectionId::new(id), PartitionId::from_index(0))
    }

    #[test]
    fn logical_close_releases_capacity_before_physical_completion() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let lease = OriginAdmission::lease_for_test(&origin);
        let (connection, physical) = ConnectionState::bounded(test_info(1), lease);

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
    fn upgrade_refines_only_a_driver_observed_protocol_close() {
        let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let lease = OriginAdmission::lease_for_test(&origin);
        let (connection, _physical) = ConnectionState::bounded(test_info(1), lease);

        assert!(connection.logical_close(CloseReason::ProtocolClosed));
        assert!(connection.refine_protocol_close_as_upgrade());
        assert_eq!(
            Some(CloseReason::Upgraded),
            connection.snapshot().close_reason
        );
        assert_eq!(1, origin.available_capacity_for_test());
        assert!(!connection.refine_protocol_close_as_upgrade());

        let (poisoned, _physical) = ConnectionState::unbounded(test_info(2));
        assert!(poisoned.logical_close(CloseReason::Poisoned));
        assert!(!poisoned.refine_protocol_close_as_upgrade());
        assert_eq!(
            Some(CloseReason::Poisoned),
            poisoned.snapshot().close_reason
        );
    }

    #[test]
    fn committed_dispatch_drains_after_logical_close() {
        let (connection, _physical) = ConnectionState::unbounded(test_info(1));
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
        let (connection, physical) = ConnectionState::unbounded(test_info(1));
        assert_eq!(ConnectionId::new(1), connection.id());
        assert_eq!(PartitionId::from_index(0), connection.owner_partition());
        assert!(!connection.snapshot().physical_complete);
        physical.complete();
        assert!(connection.snapshot().physical_complete);
    }

    #[test]
    fn connection_state_retains_immutable_connection_info() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct ConnectorMarker(&'static str);

        let origin =
            OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "example.com", None).unwrap();
        let info = ConnectionInfo::new(
            ConnectionId::new(7),
            origin.clone(),
            PartitionId::from_index(2),
            NegotiatedProtocol::Http1,
            Connected::new()
                .proxy(true)
                .extra(ConnectorMarker("connector-extra")),
        );
        let (connection, _physical) = ConnectionState::unbounded(info);

        assert_eq!(ConnectionId::new(7), connection.info().id());
        assert_eq!(&origin, connection.info().origin());
        assert_eq!(PartitionId::from_index(2), connection.owner_partition());
        assert_eq!(NegotiatedProtocol::Http1, connection.info().protocol());
        assert_eq!(None, connection.info().local_addr());
        assert_eq!(None, connection.info().remote_addr());
        assert!(connection.info().is_proxied());
        let mut extensions = Extensions::new();
        connection.info().apply_connector_extras(&mut extensions);
        assert_eq!(
            Some(&ConnectorMarker("connector-extra")),
            extensions.get::<ConnectorMarker>()
        );
    }

    #[test]
    fn root_io_drop_completes_physical_lifetime() {
        let (connection, physical) = ConnectionState::unbounded(test_info(1));
        let io = ConnectionIo::new("transport", physical);
        assert_eq!(&"transport", io.get_ref());
        assert!(!connection.snapshot().physical_complete);

        drop(io);

        assert!(connection.snapshot().physical_complete);
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use crate::client::pool::admission::OriginAdmission;
    use std::num::NonZeroUsize;

    fn test_info(id: u64) -> Arc<ConnectionInfo> {
        ConnectionInfo::for_test(ConnectionId::new(id), PartitionId::from_index(0))
    }

    #[test]
    fn dispatch_commit_linearizes_against_close() {
        loom::model(|| {
            let (connection, _physical) = ConnectionState::unbounded(test_info(1));

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
            let origin = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
            let lease = OriginAdmission::lease_for_test(&origin);
            let (connection, _physical) = ConnectionState::bounded(test_info(1), lease);
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

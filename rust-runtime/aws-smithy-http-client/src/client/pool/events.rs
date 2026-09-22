/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection establishment and installed-lifetime observations.
//!
//! Event payloads borrow transition-owned values for one synchronous callback.
//! Listener invocation occurs after pool state transitions release their locks.

use super::connection::{CloseReason, ConnectionInfo, ConnectionProtocol, ConnectionState};
use super::origin::OriginKey;
use super::partition::PartitionId;
use crate::sync::Arc as PoolArc;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Receives connection lifecycle observations from one pool.
///
/// Callbacks run synchronously on the task that performs the observed
/// transition. They must not block on work that requires progress from the
/// same pool.
pub trait ConnectionEventListener: Send + Sync + 'static {
    /// Observes one completed lifecycle transition.
    fn on_event(&self, event: &ConnectionEvent<'_>);
}

impl<F> ConnectionEventListener for F
where
    F: for<'a> Fn(&ConnectionEvent<'a>) + Send + Sync + 'static,
{
    fn on_event(&self, event: &ConnectionEvent<'_>) {
        self(event);
    }
}

/// Cloneable, type-erased connection event listener.
#[derive(Clone)]
pub struct SharedConnectionEventListener(Arc<dyn ConnectionEventListener>);

impl SharedConnectionEventListener {
    /// Creates a shared listener from one concrete implementation.
    pub fn new(listener: impl ConnectionEventListener) -> Self {
        Self(Arc::new(listener))
    }

    /// Invokes the listener without allowing observer panics to alter pool state.
    fn notify(&self, event: &ConnectionEvent<'_>) {
        #[cfg(panic = "unwind")]
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.on_event(event))).is_err()
        {
            tracing::warn!("connection event listener panicked");
        }

        #[cfg(not(panic = "unwind"))]
        self.on_event(event);
    }

    /// Reports the first transition that rejects new dispatch.
    pub(super) fn logical_close(
        &self,
        connection: &PoolArc<ConnectionInfo>,
        cause: LogicalCloseCause,
    ) {
        self.notify(&ConnectionEvent::LogicalClose(ConnectionLogicalClose {
            connection,
            cause,
        }));
    }

    /// Reports release of the client's root transport ownership.
    pub(super) fn physical_close(&self, connection: &PoolArc<ConnectionInfo>, reason: CloseReason) {
        self.notify(&ConnectionEvent::PhysicalClose(ConnectionPhysicalClose {
            connection,
            reason,
        }));
    }
}

impl ConnectionEventListener for SharedConnectionEventListener {
    fn on_event(&self, event: &ConnectionEvent<'_>) {
        self.0.on_event(event);
    }
}

impl fmt::Debug for SharedConnectionEventListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedConnectionEventListener")
    }
}

/// One connection establishment or installed-lifetime observation.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConnectionEvent<'a> {
    /// An establishment ended without installing a connection.
    EstablishmentFailed(ConnectionEstablishmentFailed<'a>),
    /// An establishment installed a connection for dispatch.
    Opened(ConnectionOpened<'a>),
    /// An installed connection stopped accepting new dispatch.
    LogicalClose(ConnectionLogicalClose<'a>),
    /// The client released its root transport ownership.
    PhysicalClose(ConnectionPhysicalClose<'a>),
}

/// Opaque identity for one connection establishment.
///
/// Establishment IDs are unique within their assigning pool. A retry that
/// starts another establishment receives another ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionEstablishmentId(u64);

impl fmt::Display for ConnectionEstablishmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Stable identity and routing context for one establishment.
#[derive(Debug)]
pub struct ConnectionEstablishmentInfo {
    id: ConnectionEstablishmentId,
    origin: OriginKey,
    partition: PartitionId,
}

impl ConnectionEstablishmentInfo {
    /// Returns this establishment's pool-assigned identity.
    pub fn id(&self) -> ConnectionEstablishmentId {
        self.id
    }

    /// Returns the canonical origin being connected.
    pub fn origin(&self) -> &OriginKey {
        &self.origin
    }

    /// Returns the partition that owns the establishment task.
    pub fn partition(&self) -> PartitionId {
        self.partition
    }
}

/// Measurements from one complete connection establishment.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ConnectionEstablishmentStats {
    total_duration: Duration,
    transport_duration: Duration,
    protocol_handshake_duration: Option<Duration>,
}

impl ConnectionEstablishmentStats {
    /// Returns elapsed time from establishment start through its terminal event.
    pub fn total_duration(&self) -> Duration {
        self.total_duration
    }

    /// Returns elapsed time spent establishing the connected transport.
    pub fn transport_duration(&self) -> Duration {
        self.transport_duration
    }

    /// Returns elapsed time spent in the Hyper protocol handshake, when started.
    pub fn protocol_handshake_duration(&self) -> Option<Duration> {
        self.protocol_handshake_duration
    }
}

/// Stage that terminated one failed connection establishment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConnectionEstablishmentStage {
    /// DNS, socket, proxy, or TLS transport establishment.
    Transport,
    /// Selection of an HTTP protocol compatible with the request.
    ProtocolSelection,
    /// Hyper's HTTP/1 or HTTP/2 client handshake.
    ProtocolHandshake,
    /// Transfer of a handshaken connection into pool-owned protocol state.
    PoolInstallation,
}

/// A terminal failure before a connection became pool supply.
#[derive(Debug)]
pub struct ConnectionEstablishmentFailed<'a> {
    establishment: &'a ConnectionEstablishmentInfo,
    stats: &'a ConnectionEstablishmentStats,
    stage: ConnectionEstablishmentStage,
    remote_addr: Option<SocketAddr>,
    protocol: Option<ConnectionProtocol>,
    error: &'a (dyn Error + Send + Sync),
}

impl<'a> ConnectionEstablishmentFailed<'a> {
    /// Returns the failed establishment's identity and routing context.
    pub fn establishment(&self) -> &'a ConnectionEstablishmentInfo {
        self.establishment
    }

    /// Returns measurements collected before the failure.
    pub fn stats(&self) -> &'a ConnectionEstablishmentStats {
        self.stats
    }

    /// Returns the stage that terminated the establishment.
    pub fn stage(&self) -> ConnectionEstablishmentStage {
        self.stage
    }

    /// Returns the connector-reported remote address, when transport completed.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Returns the selected HTTP protocol, when selection completed.
    pub fn protocol(&self) -> Option<ConnectionProtocol> {
        self.protocol
    }

    /// Returns the classified connector error delivered to acquisition.
    pub fn error(&self) -> &'a (dyn Error + Send + Sync) {
        self.error
    }
}

/// A connection installed for pool dispatch.
#[derive(Debug)]
pub struct ConnectionOpened<'a> {
    establishment: &'a ConnectionEstablishmentInfo,
    stats: &'a ConnectionEstablishmentStats,
    connection: &'a PoolArc<ConnectionInfo>,
}

impl<'a> ConnectionOpened<'a> {
    /// Returns the establishment that created this connection.
    pub fn establishment(&self) -> &'a ConnectionEstablishmentInfo {
        self.establishment
    }

    /// Returns measurements from the successful establishment.
    pub fn stats(&self) -> &'a ConnectionEstablishmentStats {
        self.stats
    }

    /// Returns immutable identity and transport facts for the connection.
    pub fn connection(&self) -> &'a PoolArc<ConnectionInfo> {
        self.connection
    }
}

/// Stable cause recorded when pool dispatch ownership ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LogicalCloseCause {
    /// The connection exceeded its configured idle timeout.
    IdleTimeout,
    /// The connection was explicitly marked unsafe for reuse.
    Poisoned,
    /// Hyper's protocol dispatch ownership ended.
    ProtocolEnded,
    /// HTTP/1 did not prove a complete reusable message boundary.
    IncompleteH1Exchange,
    /// The connection closed to move bounded capacity to another cell.
    Reclaimed,
    /// The connection pool was dropped.
    PoolDropped,
    /// The runtime driving the connection shut down.
    OwnerRuntimeShutdown,
}

impl LogicalCloseCause {
    /// Maps the first pool close classification to its stable logical cause.
    pub(super) fn from_reason(reason: CloseReason) -> Self {
        match reason {
            CloseReason::IdleTimeout => Self::IdleTimeout,
            CloseReason::Poisoned => Self::Poisoned,
            CloseReason::ProtocolClosed | CloseReason::Upgraded => Self::ProtocolEnded,
            CloseReason::IncompleteH1Exchange => Self::IncompleteH1Exchange,
            CloseReason::Reclaimed => Self::Reclaimed,
            CloseReason::PoolDropped => Self::PoolDropped,
            CloseReason::OwnerRuntimeShutdown => Self::OwnerRuntimeShutdown,
        }
    }
}

/// Observation that an installed connection stopped accepting new dispatch.
#[derive(Debug)]
pub struct ConnectionLogicalClose<'a> {
    connection: &'a PoolArc<ConnectionInfo>,
    cause: LogicalCloseCause,
}

impl<'a> ConnectionLogicalClose<'a> {
    /// Returns immutable identity and transport facts for the connection.
    pub fn connection(&self) -> &'a PoolArc<ConnectionInfo> {
        self.connection
    }

    /// Returns the first cause that ended pool dispatch ownership.
    pub fn cause(&self) -> LogicalCloseCause {
        self.cause
    }
}

/// Observation that the client released root transport ownership.
#[derive(Debug)]
pub struct ConnectionPhysicalClose<'a> {
    connection: &'a PoolArc<ConnectionInfo>,
    reason: CloseReason,
}

impl<'a> ConnectionPhysicalClose<'a> {
    /// Returns immutable identity and transport facts for the connection.
    pub fn connection(&self) -> &'a PoolArc<ConnectionInfo> {
        self.connection
    }

    /// Returns the final connection close classification.
    pub fn reason(&self) -> CloseReason {
        self.reason
    }
}

/// Pool-owned connection event source.
///
/// This owner retains callback delivery, lifecycle timing, and establishment
/// identity allocation. Starting an establishment performs no timing or
/// identity work when no listener is configured.
#[derive(Debug)]
pub(super) struct ConnectionEvents {
    listener: Option<SharedConnectionEventListener>,
    time_source: SharedTimeSource,
    next_establishment_id: AtomicU64,
}

impl ConnectionEvents {
    /// Creates the event source retained by one connection pool.
    pub(super) fn new(
        listener: Option<SharedConnectionEventListener>,
        time_source: SharedTimeSource,
    ) -> Self {
        Self {
            listener,
            time_source,
            next_establishment_id: AtomicU64::new(0),
        }
    }

    /// Records the start of an establishment when observation is enabled.
    pub(super) fn establishment_started(
        &self,
        origin: &OriginKey,
        partition: PartitionId,
    ) -> ConnectionEstablishment {
        let observation = self
            .listener
            .as_ref()
            .map(|listener| EstablishmentObservation {
                listener: listener.clone(),
                info: ConnectionEstablishmentInfo {
                    id: ConnectionEstablishmentId(
                        self.next_establishment_id.fetch_add(1, Ordering::Relaxed),
                    ),
                    origin: origin.clone(),
                    partition,
                },
                time_source: self.time_source.clone(),
                started_at: self.time_source.now(),
                protocol_handshake_started_at: None,
                transport_duration: None,
                protocol_handshake_duration: None,
                stage: ConnectionEstablishmentStage::Transport,
                remote_addr: None,
                protocol: None,
            });
        ConnectionEstablishment { observation }
    }
}

/// Tracks observations collected during one connection establishment.
pub(super) struct ConnectionEstablishment {
    observation: Option<EstablishmentObservation>,
}

/// Data collected while one observed establishment is active.
struct EstablishmentObservation {
    listener: SharedConnectionEventListener,
    info: ConnectionEstablishmentInfo,
    time_source: SharedTimeSource,
    started_at: SystemTime,
    protocol_handshake_started_at: Option<SystemTime>,
    transport_duration: Option<Duration>,
    protocol_handshake_duration: Option<Duration>,
    stage: ConnectionEstablishmentStage,
    remote_addr: Option<SocketAddr>,
    protocol: Option<ConnectionProtocol>,
}

impl ConnectionEstablishment {
    /// Records completion of the transport stage.
    pub(super) fn transport_completed(&mut self, remote_addr: Option<SocketAddr>) {
        if let Some(observation) = &mut self.observation {
            observation.transport_duration =
                Some(observation.elapsed_since(observation.started_at));
            observation.remote_addr = remote_addr;
            observation.stage = ConnectionEstablishmentStage::ProtocolSelection;
        }
    }

    /// Records the protocol selected from connector metadata.
    pub(super) fn protocol_selected(&mut self, protocol: ConnectionProtocol) {
        if let Some(observation) = &mut self.observation {
            observation.protocol = Some(protocol);
        }
    }

    /// Records entry into Hyper's protocol handshake.
    pub(super) fn protocol_handshake_started(&mut self) {
        if let Some(observation) = &mut self.observation {
            observation.protocol_handshake_started_at = Some(observation.time_source.now());
            observation.stage = ConnectionEstablishmentStage::ProtocolHandshake;
        }
    }

    /// Records a failed Hyper handshake before emitting its terminal event.
    pub(super) fn protocol_handshake_failed(&mut self) {
        if let Some(observation) = &mut self.observation {
            observation.finish_protocol_handshake();
        }
    }

    /// Records successful Hyper handshake before pool installation.
    pub(super) fn protocol_handshake_completed(&mut self) {
        if let Some(observation) = &mut self.observation {
            observation.finish_protocol_handshake();
            observation.stage = ConnectionEstablishmentStage::PoolInstallation;
        }
    }

    /// Emits the terminal failure for this establishment.
    pub(super) fn failed(mut self, error: &ConnectorError) {
        let Some(observation) = self.observation.take() else {
            return;
        };
        observation.notify_failure(error);
    }

    /// Reports successful installation and enables ordered close observations.
    pub(super) fn opened(mut self, connection: &PoolArc<ConnectionState>) {
        let listener = self.observation.take().map(|observation| {
            let stats = observation.stats();
            observation
                .listener
                .notify(&ConnectionEvent::Opened(ConnectionOpened {
                    establishment: &observation.info,
                    stats: &stats,
                    connection: connection.info(),
                }));
            observation.listener
        });
        connection.complete_opened_event(listener.as_ref());
    }

    /// Ends an establishment whose transport lost to existing HTTP/2 supply.
    pub(super) fn superseded(mut self) {
        self.observation.take();
    }
}

impl Drop for ConnectionEstablishment {
    fn drop(&mut self) {
        let Some(observation) = self.observation.take() else {
            return;
        };
        let error = ConnectorError::io("connection establishment task was dropped".into());
        observation.notify_failure(&error);
    }
}

impl EstablishmentObservation {
    fn notify_failure(&self, error: &ConnectorError) {
        let stats = self.stats();
        self.listener.notify(&ConnectionEvent::EstablishmentFailed(
            ConnectionEstablishmentFailed {
                establishment: &self.info,
                stats: &stats,
                stage: self.stage,
                remote_addr: self.remote_addr,
                protocol: self.protocol,
                error,
            },
        ));
    }

    fn stats(&self) -> ConnectionEstablishmentStats {
        let total_duration = self.elapsed_since(self.started_at);
        ConnectionEstablishmentStats {
            total_duration,
            transport_duration: self.transport_duration.unwrap_or(total_duration),
            protocol_handshake_duration: self.protocol_handshake_duration.or_else(|| {
                self.protocol_handshake_started_at
                    .map(|started_at| self.elapsed_since(started_at))
            }),
        }
    }

    fn finish_protocol_handshake(&mut self) {
        self.protocol_handshake_duration = self
            .protocol_handshake_started_at
            .take()
            .map(|started_at| self.elapsed_since(started_at));
    }

    fn elapsed_since(&self, started_at: SystemTime) -> Duration {
        self.time_source
            .now()
            .duration_since(started_at)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    fn origin() -> OriginKey {
        OriginKey::from_parts(http_1x::uri::Scheme::HTTPS, "example.com", None).unwrap()
    }

    fn events(listener: Option<SharedConnectionEventListener>) -> ConnectionEvents {
        ConnectionEvents::new(listener, SharedTimeSource::default())
    }

    fn establishment(events: &ConnectionEvents) -> ConnectionEstablishment {
        events.establishment_started(&origin(), PartitionId::from_index(1))
    }

    #[test]
    fn disabled_callbacks_do_not_allocate_establishment_ids() {
        let events = events(None);
        events.next_establishment_id.store(7, Ordering::Relaxed);

        establishment(&events).superseded();

        assert_eq!(7, events.next_establishment_id.load(Ordering::Relaxed));
    }

    #[test]
    fn listener_panic_is_isolated() {
        let events = events(Some(SharedConnectionEventListener::new(
            |_: &ConnectionEvent<'_>| panic!("listener failed"),
        )));
        let error = ConnectorError::io("synthetic transport failure".into());

        establishment(&events).failed(&error);
    }

    #[test]
    fn dropping_active_establishment_emits_transport_failure() {
        let observed = Arc::new(Mutex::new(None));
        let events = events(Some(SharedConnectionEventListener::new({
            let observed = observed.clone();
            move |event: &ConnectionEvent<'_>| {
                let ConnectionEvent::EstablishmentFailed(failed) = event else {
                    panic!("unexpected event: {event:?}");
                };
                *observed.lock().unwrap() = Some((failed.establishment().id(), failed.stage()));
            }
        })));

        drop(establishment(&events));

        assert_eq!(
            Some((
                ConnectionEstablishmentId(0),
                ConnectionEstablishmentStage::Transport
            )),
            *observed.lock().unwrap()
        );
    }

    #[test]
    fn failed_event_carries_recorded_stage_and_protocol() {
        let observed = Arc::new(Mutex::new(None));
        let events = events(Some(SharedConnectionEventListener::new({
            let observed = observed.clone();
            move |event: &ConnectionEvent<'_>| {
                let ConnectionEvent::EstablishmentFailed(failed) = event else {
                    panic!("unexpected event: {event:?}");
                };
                *observed.lock().unwrap() = Some((
                    failed.establishment().id(),
                    failed.stage(),
                    failed.protocol(),
                    failed.stats().protocol_handshake_duration().is_some(),
                ));
            }
        })));
        let mut establishment = establishment(&events);
        establishment.transport_completed(None);
        establishment.protocol_selected(ConnectionProtocol::Http2);
        establishment.protocol_handshake_started();
        establishment.protocol_handshake_failed();
        let error = ConnectorError::io("synthetic handshake failure".into());

        establishment.failed(&error);

        assert_eq!(
            Some((
                ConnectionEstablishmentId(0),
                ConnectionEstablishmentStage::ProtocolHandshake,
                Some(ConnectionProtocol::Http2),
                true,
            )),
            *observed.lock().unwrap()
        );
    }

    #[test]
    fn superseded_establishment_emits_no_public_event() {
        let observed = Arc::new(AtomicUsize::new(0));
        let events = events(Some(SharedConnectionEventListener::new({
            let observed = observed.clone();
            move |_: &ConnectionEvent<'_>| {
                observed.fetch_add(1, Ordering::Relaxed);
            }
        })));

        establishment(&events).superseded();

        assert_eq!(0, observed.load(Ordering::Relaxed));
        assert_eq!(1, events.next_establishment_id.load(Ordering::Relaxed));
    }

    #[test]
    fn closure_listener_receives_one_terminal_event() {
        let observed = Arc::new(AtomicUsize::new(0));
        let events = events(Some(SharedConnectionEventListener::new({
            let observed = observed.clone();
            move |event: &ConnectionEvent<'_>| {
                assert!(matches!(event, ConnectionEvent::EstablishmentFailed(_)));
                observed.fetch_add(1, Ordering::Relaxed);
            }
        })));
        let error = ConnectorError::io("synthetic transport failure".into());

        establishment(&events).failed(&error);

        assert_eq!(1, observed.load(Ordering::Relaxed));
    }

    #[test]
    fn logical_close_causes_cover_every_close_reason() {
        assert_eq!(
            [
                LogicalCloseCause::IdleTimeout,
                LogicalCloseCause::Poisoned,
                LogicalCloseCause::ProtocolEnded,
                LogicalCloseCause::ProtocolEnded,
                LogicalCloseCause::IncompleteH1Exchange,
                LogicalCloseCause::Reclaimed,
                LogicalCloseCause::PoolDropped,
                LogicalCloseCause::OwnerRuntimeShutdown,
            ],
            [
                CloseReason::IdleTimeout,
                CloseReason::Poisoned,
                CloseReason::ProtocolClosed,
                CloseReason::Upgraded,
                CloseReason::IncompleteH1Exchange,
                CloseReason::Reclaimed,
                CloseReason::PoolDropped,
                CloseReason::OwnerRuntimeShutdown,
            ]
            .map(LogicalCloseCause::from_reason)
        );
    }
}

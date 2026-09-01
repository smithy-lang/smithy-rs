/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/2 flight, generation, route, and request-lease ownership.
//!
//! One connection-owning cell stores the authoritative HTTP/2 generation.
//! Other cells store only [`H2Route`] identities and revalidate them at the
//! connection-owning cell before dispatch. A generation may stop accepting
//! new requests while prospective and accepted requests still drain.
//!
//! [`H2Records`] is the invariant owner under the cell lock. It stores one
//! post-ALPN flight, the accepting and draining generations, a local generation
//! gate, and at most one peer route. A route carries only connection partition
//! and generation identity. [`H2Activation`] is the unlocked value that carries
//! a prospective lease and transient sender from selection to Hyper acceptance.
//!
//! ```text
//! requesting cell                        connection-owning cell
//! peer route + local gate --identity---> exact accepting generation
//!                                      |-- increment prospective count
//!                                      `-- clone transient sender
//!                                               |
//!                                               `--> H2Activation
//! ```
//!
//! ```text
//! no flight -- post-ALPN owner task ----------------------> Flight
//! Flight -- successful handshake and install ------------> Accepting
//! Flight -- failure, stale completion, or task drop ------> no flight
//! Accepting -- close with retained request leases --------> Draining
//! Accepting -- close without retained request leases -----> removed
//! Draining -- last prospective or accepted lease --------> removed
//! ```
//!
//! ```text
//! no peer route -- publication ---------------------------> PeerRoute(generation)
//! PeerRoute(A) -- replacement publication ---------------> PeerRoute(B)
//! PeerRoute -- stale activation or local generation -----> no peer route
//! ```
//!
//! Generation installation makes the sender visible before the owner task
//! submits the Hyper driver. An activation in that interval is accepted by
//! Hyper's dispatch channel and remains pending until the driver is polled.
//!
//! Activation reserves a prospective lease before a sender clone leaves the
//! cell lock. Hyper acceptance converts that reservation to one accepted
//! lease. The accepted lease is released only after both request-send and
//! response-receive endpoints terminate.

use super::super::connection::{ConnectionInfo, ConnectionState, DispatchGuard};
use super::super::partition::PartitionId;
use super::waiters::{AcquisitionQueue, WaiterInstall};
use super::{AcquisitionEvent, AcquisitionResult, CellState, OriginCell, WaiterId};
use crate::sync::{Arc, Mutex, Weak};
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_types::body::SdkBody;
use std::collections::{BTreeSet, HashMap};
use std::time::SystemTime;

/// Identity of one post-ALPN HTTP/2 establishment flight.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::client::pool) struct H2FlightId(u64);

/// Identity of one installed HTTP/2 generation within a cell.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::client::pool) struct H2GenerationId(u64);

#[cfg(test)]
impl H2GenerationId {
    /// Creates a generation identity for admission-index tests.
    pub(in crate::client::pool) fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Cloneable Hyper request sender retained only by its owning generation.
///
/// The test-only variant exercises generation and lease transitions without a
/// parallel fake generation implementation.
#[derive(Clone)]
pub(in crate::client::pool) enum H2Sender {
    /// Sender returned by a successful Hyper HTTP/2 handshake.
    Hyper(hyper::client::conn::http2::SendRequest<SdkBody>),
    /// Synthetic sender identity used by state-machine tests.
    #[cfg(test)]
    Test(u64),
}

impl H2Sender {
    /// Wraps a sender produced by a successful Hyper HTTP/2 handshake.
    pub(in crate::client::pool) fn from_hyper(
        inner: hyper::client::conn::http2::SendRequest<SdkBody>,
    ) -> Self {
        Self::Hyper(inner)
    }

    /// Returns whether Hyper has observed connection closure.
    pub(in crate::client::pool) fn is_closed(&self) -> bool {
        match self {
            Self::Hyper(sender) => sender.is_closed(),
            #[cfg(test)]
            Self::Test(_) => false,
        }
    }

    /// Returns mutable access to a transient sender clone for dispatch.
    ///
    /// # Panics
    ///
    /// Panics when a test-only sender reaches real dispatch.
    pub(in crate::client::pool) fn hyper_mut(
        &mut self,
    ) -> &mut hyper::client::conn::http2::SendRequest<SdkBody> {
        match self {
            Self::Hyper(sender) => sender,
            #[cfg(test)]
            Self::Test(_) => panic!("test HTTP/2 sender reached Hyper dispatch"),
        }
    }

    /// Creates a synthetic sender for state-machine tests.
    #[cfg(test)]
    fn test(id: u64) -> Self {
        Self::Test(id)
    }
}

impl std::fmt::Debug for H2Sender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hyper(_) => f.write_str("H2Sender::Hyper"),
            #[cfg(test)]
            Self::Test(id) => f.debug_tuple("H2Sender::Test").field(id).finish(),
        }
    }
}

/// Identity-only reference to one accepting generation.
///
/// The route owns no sender, connection capacity, driver, or socket. Every
/// activation upgrades the cell reference and checks the generation identity.
#[derive(Clone, Debug)]
pub(in crate::client::pool) struct H2Route {
    id: H2RouteId,
    connection_cell: Weak<OriginCell>,
}

/// Exact connection-owning generation named by one peer route.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct H2RouteId {
    connection_partition: PartitionId,
    generation: H2GenerationId,
}

impl H2Route {
    /// Creates a route after its generation has been installed.
    pub(in crate::client::pool) fn new(
        connection_cell: &Arc<OriginCell>,
        generation: H2GenerationId,
    ) -> Self {
        Self {
            connection_cell: Weak::from_arc(connection_cell),
            id: H2RouteId {
                connection_partition: connection_cell.id().partition(),
                generation,
            },
        }
    }

    /// Returns the exact connection-owning generation named by this route.
    fn id(&self) -> H2RouteId {
        self.id
    }

    /// Returns the partition that owns the advertised generation.
    pub(super) fn connection_partition(&self) -> PartitionId {
        self.id.connection_partition
    }

    /// Returns the advertised generation identity.
    #[cfg(any(debug_assertions, test))]
    pub(in crate::client::pool) fn generation(&self) -> H2GenerationId {
        self.id.generation
    }

    /// Attempts to reserve a prospective request lease for one requesting partition.
    pub(super) fn activate(&self, request_partition: PartitionId) -> Option<H2Activation> {
        let cell = self.connection_cell.upgrade()?;
        let mut activation = OriginCell::activate_h2(&cell, self.id.generation)?;
        activation.request_partition = request_partition;
        Some(activation)
    }
}

/// Cell-local ownership of one HTTP/2 flight and installed generations.
///
/// At every completed transition:
///
/// - at most one flight and one accepting generation exist;
/// - the accepting identity names an `Accepting` record;
/// - every other generation is `Draining`;
/// - prospective and accepted request counts are checked and non-wrapping;
/// - a draining record remains until both counts reach zero; and
/// - each flight participant identity appears at most once.
#[derive(Debug, Default)]
pub(super) struct H2Records {
    /// Post-ALPN convergence in progress for this cell.
    flight: Option<H2Flight>,
    /// Installed accepting and draining generations by exact identity.
    generations: HashMap<H2GenerationId, H2Generation>,
    /// Sole generation permitted to issue new activations.
    accepting: Option<H2GenerationId>,
    /// Identity-only route to one peer cell's accepting generation.
    peer_route: Option<PeerH2Route>,
    /// Local activation order for the accepting generation.
    gate: GenerationGate,
    /// Next cell-local flight identity.
    next_flight: u64,
    /// Next cell-local generation identity.
    next_generation: u64,
}

/// One post-ALPN flight and the waiters awaiting its result.
#[derive(Debug)]
struct H2Flight {
    /// Identity checked by the owner task at completion.
    id: H2FlightId,
    /// Waiters that receive the shared handshake result.
    participants: BTreeSet<WaiterId>,
}

/// One installed multiplexed connection generation.
#[derive(Debug)]
struct H2Generation {
    /// Establishment waiters transferred to this generation but not yet served.
    pending_waiters: BTreeSet<WaiterId>,
    /// Protocol-neutral connection and capacity owner.
    connection: Arc<ConnectionState>,
    /// Authoritative Hyper sender cloned only after generation validation.
    sender: H2Sender,
    /// Whether new activations may be issued.
    residence: H2Residence,
    /// Activations selected but not yet accepted by Hyper.
    prospective: usize,
    /// Whether Hyper has accepted a request on this generation.
    has_dispatched: bool,
    /// Accepted requests whose two lease endpoints have not both terminated.
    accepted: usize,
    /// Expiration deadline while the generation remains accepting.
    idle_deadline: Option<SystemTime>,
}

/// Whether an installed generation may accept new requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H2Residence {
    /// New requests may reserve prospective leases.
    Accepting,
    /// No new requests are admitted; retained leases may finish.
    Draining,
}

/// Peer route visibility and requesting-cell activation order.
#[derive(Debug)]
struct PeerH2Route {
    /// Exact connection-cell generation visible to this requesting cell.
    route: H2Route,
    /// Local waiter priority for uses of this route.
    gate: GenerationGate,
    /// Waiter whose route activation is crossing the connection-cell lock.
    crossing: Option<WaiterId>,
}

/// Local priority state for one accepting generation.
#[derive(Debug, Default)]
enum GenerationGate {
    /// No accepting generation is visible.
    #[default]
    Closed,
    /// Waiters committed through `cutoff` precede later arrivals.
    Prioritizing {
        /// Exact generation governed by this gate.
        generation: H2GenerationId,
        /// Newest waiter committed before generation visibility.
        cutoff: WaiterId,
        /// Prioritized waiter whose activation has not accepted or cancelled.
        activating: Option<WaiterId>,
    },
    /// The publication cutoff drained; queued work still precedes direct arrivals.
    Open { generation: H2GenerationId },
}

/// One activation opportunity returned by a generation gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GateTurn {
    /// The gate cannot issue another activation.
    Unavailable,
    /// Any oldest compatible acquisition may activate.
    Open,
    /// Only an acquisition at or before this waiter may activate.
    Through(WaiterId),
}

impl GenerationGate {
    /// Creates a gate over every waiter committed through `cutoff`.
    fn for_generation(generation: H2GenerationId, cutoff: Option<WaiterId>) -> Self {
        match cutoff {
            Some(cutoff) => Self::Prioritizing {
                generation,
                cutoff,
                activating: None,
            },
            None => Self::Open { generation },
        }
    }

    fn generation(&self) -> Option<H2GenerationId> {
        match self {
            Self::Closed => None,
            Self::Prioritizing { generation, .. } | Self::Open { generation } => Some(*generation),
        }
    }

    /// Returns the next activation opportunity and opens a drained priority gate.
    fn next_turn(&mut self, has_prioritized: bool) -> GateTurn {
        match self {
            Self::Closed => GateTurn::Unavailable,
            Self::Prioritizing {
                cutoff,
                activating,
                generation,
            } => {
                if activating.is_some() {
                    return GateTurn::Unavailable;
                }
                if !has_prioritized {
                    let generation = *generation;
                    *self = Self::Open { generation };
                    GateTurn::Open
                } else {
                    GateTurn::Through(*cutoff)
                }
            }
            Self::Open { .. } => GateTurn::Open,
        }
    }

    /// Records a prioritized activation until it accepts or cancels.
    fn begin_gate_activation(&mut self, waiter: WaiterId) -> bool {
        let activating = match self {
            Self::Closed => {
                unreachable!("started an HTTP/2 activation while its gate was closed")
            }
            Self::Prioritizing { activating, .. } => activating,
            Self::Open { .. } => return false,
        };
        assert!(
            activating.replace(waiter).is_none(),
            "HTTP/2 generation gate admitted two activation opportunities"
        );
        true
    }

    /// Discharges one exact activation opportunity.
    fn finish_gate_activation(&mut self, generation: H2GenerationId, waiter: WaiterId) -> bool {
        if self.generation() != Some(generation) {
            return false;
        }
        let activating = match self {
            Self::Closed => return false,
            Self::Prioritizing { activating, .. } => activating,
            Self::Open { .. } => return false,
        };
        if *activating != Some(waiter) {
            return false;
        }
        *activating = None;
        true
    }

    fn priority_cutoff(&self) -> Option<WaiterId> {
        match self {
            Self::Prioritizing { cutoff, .. } => Some(*cutoff),
            Self::Closed | Self::Open { .. } => None,
        }
    }

    fn activating(&self) -> Option<WaiterId> {
        match self {
            Self::Prioritizing { activating, .. } => *activating,
            Self::Closed | Self::Open { .. } => None,
        }
    }

    fn is_open_for(&self, generation: H2GenerationId) -> bool {
        matches!(self, Self::Open { generation: current } if *current == generation)
    }
}

/// Result of atomically converging one post-ALPN attempt.
#[derive(Debug)]
pub(in crate::client::pool) enum H2FlightInstall {
    /// An installed generation can serve this waiter.
    Accepting(H2GenerationId),
    /// The waiter joined the current flight as a result participant.
    Joined,
    /// The caller owns the task that must drive this new flight.
    Driver(H2FlightId),
}

/// Result of joining an accepting generation after ALPN selected HTTP/2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) enum H2GenerationJoin {
    /// The waiter was transferred to the named generation.
    Joined,
    /// The named generation stopped accepting before the transfer.
    GenerationChanged,
    /// Another transition already installed the waiter's result.
    WaiterCompleted,
}

impl H2Records {
    /// Installs or joins the one current flight after ALPN selected HTTP/2.
    pub(super) fn install_or_join_flight(&mut self, waiter: WaiterId) -> H2FlightInstall {
        if let Some(generation) = self.accepting {
            return H2FlightInstall::Accepting(generation);
        }
        if let Some(flight) = &mut self.flight {
            assert!(
                flight.participants.insert(waiter),
                "HTTP/2 waiter joined one flight more than once"
            );
            self.assert_consistent();
            return H2FlightInstall::Joined;
        }

        let id = self.take_flight_id();
        self.flight = Some(H2Flight {
            id,
            participants: BTreeSet::from([waiter]),
        });
        self.assert_consistent();
        H2FlightInstall::Driver(id)
    }

    /// Removes one cancelled waiter from its current flight, when present.
    pub(super) fn cancel_flight_participant(&mut self, waiter: WaiterId) {
        if let Some(flight) = &mut self.flight {
            flight.participants.remove(&waiter);
        }
        self.assert_consistent();
    }

    /// Removes a waiter that no longer awaits an accepting generation.
    pub(super) fn cancel_pending_waiter(&mut self, waiter: WaiterId) {
        if let Some(generation) = self.accepting {
            self.generations
                .get_mut(&generation)
                .expect("accepting HTTP/2 generation disappeared")
                .pending_waiters
                .remove(&waiter);
        }
        self.assert_consistent();
    }

    /// Installs the generation produced by the named flight.
    pub(in crate::client::pool) fn complete_flight(
        &mut self,
        flight: H2FlightId,
        connection: Arc<ConnectionState>,
        sender: H2Sender,
        idle_deadline: Option<SystemTime>,
    ) -> Result<H2GenerationId, (Arc<ConnectionState>, H2Sender)> {
        if self.flight.as_ref().map(|current| current.id) != Some(flight)
            || self.accepting.is_some()
        {
            return Err((connection, sender));
        }
        let flight = self
            .flight
            .take()
            .expect("validated HTTP/2 flight disappeared");
        let generation =
            self.install_generation(flight.participants, connection, sender, idle_deadline);
        self.assert_consistent();
        Ok(generation)
    }

    /// Installs one accepting generation with its transferred waiters.
    fn install_generation(
        &mut self,
        pending_waiters: BTreeSet<WaiterId>,
        connection: Arc<ConnectionState>,
        sender: H2Sender,
        idle_deadline: Option<SystemTime>,
    ) -> H2GenerationId {
        let generation = self.take_generation_id();
        let replaced = self.generations.insert(
            generation,
            H2Generation {
                pending_waiters,
                connection,
                sender,
                residence: H2Residence::Accepting,
                prospective: 0,
                has_dispatched: false,
                accepted: 0,
                idle_deadline,
            },
        );
        assert!(replaced.is_none(), "HTTP/2 generation identity was reused");
        self.accepting = Some(generation);
        self.peer_route = None;
        debug_assert!(matches!(self.gate, GenerationGate::Closed));
        self.gate = GenerationGate::Open { generation };
        generation
    }

    /// Retires a failed or dropped flight and returns its participants.
    pub(super) fn fail_flight(&mut self, flight: H2FlightId) -> Option<Vec<WaiterId>> {
        if self.flight.as_ref().map(|current| current.id) != Some(flight) {
            return None;
        }
        let participants = self
            .flight
            .take()
            .expect("validated HTTP/2 flight disappeared")
            .participants
            .into_iter()
            .collect();
        self.assert_consistent();
        Some(participants)
    }

    /// Closes the gate over waiters committed when a generation became visible.
    pub(super) fn prioritize_through(&mut self, cutoff: Option<WaiterId>) {
        let generation = self
            .accepting
            .expect("HTTP/2 generation gate opened without an accepting generation");
        self.gate = GenerationGate::for_generation(generation, cutoff);
        self.assert_consistent();
    }

    /// Extends priority to a post-ALPN waiter that found this generation.
    pub(super) fn prioritize_waiter(
        &mut self,
        generation: H2GenerationId,
        waiter: WaiterId,
    ) -> bool {
        if self.accepting != Some(generation) {
            return false;
        }
        match &mut self.gate {
            GenerationGate::Closed => {
                unreachable!("accepting HTTP/2 generation had no generation gate")
            }
            GenerationGate::Prioritizing { cutoff, .. } => {
                *cutoff = (*cutoff).max(waiter);
            }
            GenerationGate::Open { .. } => {
                self.gate = GenerationGate::Prioritizing {
                    generation,
                    cutoff: waiter,
                    activating: None,
                };
            }
        }
        self.generations
            .get_mut(&generation)
            .expect("accepting HTTP/2 generation disappeared")
            .pending_waiters
            .insert(waiter);
        self.assert_consistent();
        true
    }

    /// Returns the next queued activation opportunity.
    fn next_gate_turn(&mut self, has_prioritized: bool) -> GateTurn {
        self.gate.next_turn(has_prioritized)
    }

    /// Records a prioritized activation and releases its transferred waiter.
    pub(super) fn begin_gate_activation(&mut self, waiter: WaiterId) -> bool {
        let gated = self.gate.begin_gate_activation(waiter);
        if let Some(generation) = self.accepting {
            self.generations
                .get_mut(&generation)
                .expect("accepting HTTP/2 generation disappeared")
                .pending_waiters
                .remove(&waiter);
        }
        self.assert_consistent();
        gated
    }

    /// Discharges one accepted or cancelled activation opportunity.
    pub(super) fn finish_gate_activation(
        &mut self,
        generation: H2GenerationId,
        waiter: WaiterId,
    ) -> bool {
        let finished = self.gate.finish_gate_activation(generation, waiter);
        self.assert_consistent();
        finished
    }

    /// Returns whether a direct arrival may use the accepting generation.
    pub(super) fn direct_is_allowed(&self, queued: bool) -> bool {
        !queued && matches!(self.gate, GenerationGate::Open { .. })
    }

    /// Returns the publication cutoff while older waiters remain prioritized.
    fn priority_cutoff(&self) -> Option<WaiterId> {
        self.gate.priority_cutoff()
    }

    /// Reserves one prospective lease against an accepting generation.
    fn activate(&mut self, generation: H2GenerationId) -> Option<H2ActivationParts> {
        if self.accepting != Some(generation) {
            return None;
        }
        let record = self.generations.get_mut(&generation)?;
        if record.residence != H2Residence::Accepting {
            return None;
        }
        record.prospective = record
            .prospective
            .checked_add(1)
            .expect("HTTP/2 prospective request count exhausted");
        let parts = H2ActivationParts {
            sender: record.sender.clone(),
            reused: record.has_dispatched,
            connection: record.connection.clone(),
        };
        self.assert_consistent();
        Some(parts)
    }

    /// Converts one prospective reservation to an accepted request lease.
    fn accept(&mut self, generation: H2GenerationId) -> bool {
        let Some(record) = self.generations.get_mut(&generation) else {
            return false;
        };
        if record.prospective == 0 {
            return false;
        }
        record.prospective -= 1;
        record.accepted = record
            .accepted
            .checked_add(1)
            .expect("HTTP/2 accepted request count exhausted");
        record.has_dispatched = true;
        self.assert_consistent();
        true
    }

    /// Cancels one prospective reservation and detaches an empty drain record.
    ///
    /// # Panics
    ///
    /// Panics if the activation's exact generation or prospective count is
    /// missing.
    #[must_use]
    fn cancel(&mut self, generation: H2GenerationId) -> Option<H2Generation> {
        let record = self
            .generations
            .get_mut(&generation)
            .expect("HTTP/2 activation generation disappeared before cancellation");
        assert!(
            record.prospective > 0,
            "HTTP/2 prospective request count underflowed"
        );
        record.prospective -= 1;
        self.remove_finished_drain(generation)
    }

    /// Releases one accepted request and detaches an empty drain record.
    ///
    /// # Panics
    ///
    /// Panics if the lease's exact generation or accepted count is missing.
    #[must_use]
    fn complete_request(&mut self, generation: H2GenerationId) -> Option<H2Generation> {
        let record = self
            .generations
            .get_mut(&generation)
            .expect("HTTP/2 request generation disappeared before lease completion");
        assert!(
            record.accepted > 0,
            "HTTP/2 accepted request count underflowed"
        );
        record.accepted -= 1;
        self.remove_finished_drain(generation)
    }

    /// Moves one exact accepting generation to draining.
    #[must_use]
    fn begin_close(&mut self, generation: H2GenerationId) -> Option<H2CloseTransition> {
        if self.accepting != Some(generation) {
            return None;
        }
        let record = self.generations.get_mut(&generation)?;
        if record.residence != H2Residence::Accepting {
            return None;
        }
        record.residence = H2Residence::Draining;
        self.accepting = None;
        self.gate = GenerationGate::Closed;
        let pending_waiters = std::mem::take(&mut record.pending_waiters);
        let remove_record = record.prospective == 0 && record.accepted == 0;
        let connection = record.connection.clone();
        let removed_generation = remove_record
            .then(|| self.generations.remove(&generation))
            .flatten();
        Some(H2CloseTransition {
            connection,
            pending_waiters,
            removed_generation,
        })
    }

    /// Detaches an exact accepting generation only while it has no request work.
    #[must_use]
    fn begin_idle_reclaim(&mut self, generation: H2GenerationId) -> Option<H2CloseTransition> {
        self.is_idle(generation)
            .then(|| self.begin_close(generation))
            .flatten()
    }

    /// Returns the accepting generation when one is locally reusable.
    pub(in crate::client::pool) fn accepting(&self) -> Option<H2GenerationId> {
        self.accepting
    }

    /// Returns whether an exact generation remains accepting.
    pub(super) fn is_accepting(&self, generation: H2GenerationId) -> bool {
        self.accepting == Some(generation)
    }

    /// Returns the generation peers may discover after the local cutoff drains.
    pub(super) fn publishable_generation(&self) -> Option<H2GenerationId> {
        let generation = self.accepting?;
        matches!(
            self.gate,
            GenerationGate::Open { generation: gate_generation } if gate_generation == generation
        )
        .then_some(generation)
    }

    /// Returns whether an exact publishable generation has no request work.
    pub(super) fn is_idle(&self, generation: H2GenerationId) -> bool {
        if self.publishable_generation() != Some(generation) {
            return false;
        }
        self.generations.get(&generation).is_some_and(|record| {
            record.pending_waiters.is_empty() && record.prospective == 0 && record.accepted == 0
        })
    }

    /// Returns whether a local generation or peer route suppresses admission demand.
    pub(super) fn has_visible_h2(&self) -> bool {
        self.accepting.is_some() || self.peer_route.is_some()
    }

    /// Installs or refreshes one identity-only peer route.
    pub(super) fn install_peer_route(&mut self, route: H2Route, cutoff: Option<WaiterId>) {
        if self.accepting.is_some() {
            self.peer_route = None;
            self.assert_consistent();
            return;
        }
        let id = route.id();
        match &mut self.peer_route {
            Some(current) if current.route.id() == id => {
                if let Some(cutoff) = cutoff {
                    match &mut current.gate {
                        GenerationGate::Closed => {
                            unreachable!("visible peer HTTP/2 route had a closed gate")
                        }
                        GenerationGate::Prioritizing {
                            cutoff: current, ..
                        } => *current = (*current).max(cutoff),
                        GenerationGate::Open { generation } => {
                            current.gate = GenerationGate::Prioritizing {
                                generation: *generation,
                                cutoff,
                                activating: None,
                            };
                        }
                    }
                }
            }
            _ => {
                self.peer_route = Some(PeerH2Route {
                    gate: GenerationGate::for_generation(id.generation, cutoff),
                    route,
                    crossing: None,
                });
            }
        }
        self.assert_consistent();
    }

    /// Marks one requesting waiter as the route's current activation opportunity.
    fn prepare_peer_activation(
        &mut self,
        waiters: &AcquisitionQueue,
    ) -> Option<PreparedPeerActivation> {
        let peer = self.peer_route.as_mut()?;
        if peer.crossing.is_some() {
            return None;
        }
        let has_prioritized = peer
            .gate
            .priority_cutoff()
            .is_some_and(|cutoff| waiters.has_h2_candidate_through(cutoff));
        let cutoff = match peer.gate.next_turn(has_prioritized) {
            GateTurn::Unavailable => return None,
            GateTurn::Open => None,
            GateTurn::Through(cutoff) => Some(cutoff),
        };
        let waiter = waiters.oldest_h2_candidate()?;
        if cutoff.is_some_and(|cutoff| waiter > cutoff) {
            return None;
        }
        let gated = peer.gate.begin_gate_activation(waiter);
        peer.crossing = Some(waiter);
        Some(PreparedPeerActivation {
            route: peer.route.clone(),
            waiter,
            cutoff,
            gated,
        })
    }

    /// Revalidates one peer activation after the connection-cell crossing.
    fn peer_activation_is_current(
        &self,
        prepared: &PreparedPeerActivation,
        waiters: &AcquisitionQueue,
    ) -> bool {
        self.peer_route.as_ref().is_some_and(|peer| {
            peer.route.id() == prepared.route.id()
                && peer.crossing == Some(prepared.waiter)
                && (!prepared.gated || peer.gate.activating() == Some(prepared.waiter))
                && waiters.is_oldest_h2_candidate(prepared.waiter)
        })
    }

    /// Ends one route crossing after its result is installed or rejected.
    fn finish_peer_crossing(&mut self, route: H2RouteId, waiter: WaiterId) -> bool {
        let Some(peer) = &mut self.peer_route else {
            return false;
        };
        if peer.route.id() != route || peer.crossing != Some(waiter) {
            return false;
        }
        peer.crossing = None;
        self.assert_consistent();
        true
    }

    /// Discharges one prioritized peer-route activation opportunity.
    fn finish_peer_gate(&mut self, route: H2RouteId, waiter: WaiterId) -> bool {
        let Some(peer) = &mut self.peer_route else {
            return false;
        };
        if peer.route.id() != route {
            return false;
        }
        let finished = peer.gate.finish_gate_activation(route.generation, waiter);
        self.assert_consistent();
        finished
    }

    /// Clears a peer activation marker when its requesting waiter is cancelled.
    pub(super) fn cancel_peer_activation(&mut self, waiter: WaiterId) -> bool {
        let Some(route) = self.peer_route.as_ref().map(|peer| peer.route.id()) else {
            return false;
        };
        if self
            .peer_route
            .as_ref()
            .and_then(|peer| peer.gate.activating())
            != Some(waiter)
        {
            return false;
        }
        self.finish_peer_gate(route, waiter)
    }

    /// Returns an open peer route when no queued acquisition precedes it.
    fn open_peer_route(&self, queued: bool) -> Option<H2Route> {
        let peer = self.peer_route.as_ref()?;
        (!queued && peer.gate.is_open_for(peer.route.id().generation)).then(|| peer.route.clone())
    }

    /// Revalidates a direct peer route after its connection-cell crossing.
    fn direct_peer_route_is_current(&self, route: H2RouteId, queued: bool) -> bool {
        !queued
            && self.peer_route.as_ref().is_some_and(|peer| {
                peer.route.id() == route && peer.gate.is_open_for(route.generation)
            })
    }

    /// Removes one stale exact peer route.
    fn clear_peer_route(&mut self, route: H2RouteId) -> bool {
        if self.peer_route.as_ref().map(|peer| peer.route.id()) != Some(route) {
            return false;
        }
        self.peer_route = None;
        self.assert_consistent();
        true
    }

    /// Returns all installed generation identities for shutdown.
    pub(super) fn generation_ids(&self) -> Vec<H2GenerationId> {
        self.generations.keys().copied().collect()
    }

    /// Returns the accepting generation's idle deadline.
    pub(super) fn nearest_idle_deadline(&self) -> Option<SystemTime> {
        self.accepting
            .and_then(|generation| self.generations.get(&generation))
            .and_then(|record| record.idle_deadline)
    }

    /// Returns an accepting generation whose idle deadline elapsed.
    pub(super) fn expired(&self, now: SystemTime) -> Option<H2GenerationId> {
        let generation = self.accepting?;
        let deadline = self.generations.get(&generation)?.idle_deadline?;
        (deadline <= now).then_some(generation)
    }

    /// Replaces the accepting generation's idle deadline after dispatch.
    pub(super) fn reset_idle_deadline(
        &mut self,
        generation: H2GenerationId,
        deadline: Option<SystemTime>,
    ) -> bool {
        if self.accepting != Some(generation) {
            return false;
        }
        let record = self
            .generations
            .get_mut(&generation)
            .expect("accepting HTTP/2 generation disappeared");
        record.idle_deadline = deadline;
        self.assert_consistent();
        true
    }

    /// Detaches a draining generation after its last retained request ends.
    #[must_use]
    fn remove_finished_drain(&mut self, generation: H2GenerationId) -> Option<H2Generation> {
        let remove = self.generations.get(&generation).is_some_and(|record| {
            record.residence == H2Residence::Draining
                && record.prospective == 0
                && record.accepted == 0
        });
        remove
            .then(|| self.generations.remove(&generation))
            .flatten()
    }

    fn take_flight_id(&mut self) -> H2FlightId {
        let value = self.next_flight;
        self.next_flight = value
            .checked_add(1)
            .expect("HTTP/2 flight identity exhausted");
        H2FlightId(value)
    }

    fn take_generation_id(&mut self) -> H2GenerationId {
        let value = self.next_generation;
        self.next_generation = value
            .checked_add(1)
            .expect("HTTP/2 generation identity exhausted");
        H2GenerationId(value)
    }

    /// Checks flight, generation, residence, and request-count relationships.
    pub(super) fn assert_consistent(&self) {
        #[cfg(any(debug_assertions, test))]
        {
            if std::thread::panicking() {
                return;
            }
            let accepting_records = self
                .generations
                .iter()
                .filter(|(_, record)| record.residence == H2Residence::Accepting)
                .map(|(generation, _)| *generation)
                .collect::<Vec<_>>();
            match self.accepting {
                Some(generation) => assert_eq!(
                    vec![generation],
                    accepting_records,
                    "HTTP/2 accepting identity did not match generation residence"
                ),
                None => assert!(
                    accepting_records.is_empty(),
                    "HTTP/2 accepting generation lacked an accepting identity"
                ),
            }
            assert_eq!(
                self.accepting,
                self.gate.generation(),
                "HTTP/2 generation gate did not name the accepting generation"
            );
            assert!(
                self.flight.is_none() || self.accepting.is_none(),
                "HTTP/2 flight coexisted with an accepting generation"
            );
            if let Some(peer) = &self.peer_route {
                assert!(
                    self.accepting.is_none(),
                    "local accepting generation retained a peer route"
                );
                assert_eq!(
                    Some(peer.route.generation()),
                    peer.gate.generation(),
                    "peer route gate did not name the advertised generation"
                );
            }

            for record in self.generations.values() {
                if record.residence == H2Residence::Draining {
                    assert!(
                        record.prospective > 0 || record.accepted > 0,
                        "empty HTTP/2 draining generation was retained"
                    );
                    assert!(
                        record.pending_waiters.is_empty(),
                        "draining HTTP/2 generation retained unserved waiters"
                    );
                }
            }
        }
    }

    /// Checks transferred waiter identities against the cell's acquisition state.
    #[cfg(any(debug_assertions, test))]
    pub(super) fn assert_pending_waiters(&self, waiters: &AcquisitionQueue) {
        for record in self.generations.values() {
            for waiter in &record.pending_waiters {
                assert!(
                    waiters.is_launching_h2_candidate(*waiter),
                    "HTTP/2 generation retained a waiter that was no longer launchable"
                );
            }
        }
    }
}

/// One requesting-cell activation prepared before crossing to its connection cell.
struct PreparedPeerActivation {
    /// Route validated in the requesting cell before the lock crossing.
    route: H2Route,
    /// Oldest waiter selected for this activation.
    waiter: WaiterId,
    /// Publication cutoff that this waiter must satisfy.
    cutoff: Option<WaiterId>,
    /// Whether acceptance or cancellation must discharge a priority turn.
    gated: bool,
}

/// State detached when an accepting generation begins draining.
struct H2CloseTransition {
    /// Connection whose logical close follows cell unlock.
    connection: Arc<ConnectionState>,
    /// Unserved transferred waiters that must re-enter acquisition.
    pending_waiters: BTreeSet<WaiterId>,
    /// Removed record retained until the cell lock is released.
    removed_generation: Option<H2Generation>,
}

/// Sender and connection cloned while a prospective lease is state-owned.
struct H2ActivationParts {
    /// Transient sender clone for one activation.
    sender: H2Sender,
    /// Whether the generation has accepted an earlier request.
    reused: bool,
    /// Connection retained through prospective dispatch.
    connection: Arc<ConnectionState>,
}

/// Values transferred together when an activation begins Hyper dispatch.
pub(in crate::client::pool) struct H2DispatchParts {
    /// Transient sender clone for one dispatch attempt.
    pub(in crate::client::pool) sender: H2Sender,
    /// Request-send endpoint retained by the request body.
    pub(in crate::client::pool) send_endpoint: H2LeaseEndpoint,
    /// Response-receive endpoint retained by the response body.
    pub(in crate::client::pool) receive_endpoint: H2LeaseEndpoint,
}

/// Prospective request reservation against one exact generation.
///
/// Dropping the activation before Hyper accepts the request cancels the
/// prospective reservation. Acceptance creates endpoint guards and transfers
/// dispatch accounting to the response lifecycle.
pub(in crate::client::pool) struct H2Activation {
    /// Cell that owns the selected generation.
    cell: Arc<OriginCell>,
    /// Partition issuing this request.
    request_partition: PartitionId,
    /// Exact generation that owns the prospective count.
    generation: H2GenerationId,
    /// Transient sender transferred at most once into dispatch.
    sender: Option<H2Sender>,
    /// Whether the generation has accepted an earlier request.
    reused: bool,
    /// Connection retained until acceptance or cancellation.
    connection: Arc<ConnectionState>,
    /// Shared state for the request's send and receive endpoints.
    lease: Arc<H2LeaseCore>,
    /// Requesting-cell priority discharged only at acceptance or cancellation.
    gate: Option<H2GateToken>,
    /// Whether drop must cancel the prospective generation count.
    active: bool,
}

impl std::fmt::Debug for H2Activation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2Activation")
            .field("generation", &self.generation)
            .field("connection_id", &self.connection.id())
            .field("active", &self.active)
            .finish()
    }
}

impl H2Activation {
    /// Builds an activation after the generation records its prospective lease.
    fn new(
        cell: Arc<OriginCell>,
        generation: H2GenerationId,
        parts: H2ActivationParts,
        gate: Option<H2GateToken>,
    ) -> Self {
        let request_partition = cell.id().partition();
        let lease = Arc::new(H2LeaseCore {
            cell: Weak::from_arc(&cell),
            generation,
            state: Mutex::new(H2LeaseState::Prospective {
                send_complete: false,
                receive_complete: false,
            }),
        });
        Self {
            cell,
            request_partition,
            generation,
            sender: Some(parts.sender),
            reused: parts.reused,
            connection: parts.connection,
            lease,
            gate,
            active: true,
        }
    }

    /// Returns the exact selected generation.
    #[cfg(test)]
    pub(in crate::client::pool) fn generation(&self) -> H2GenerationId {
        self.generation
    }

    /// Returns whether Hyper previously accepted a request on this generation.
    pub(in crate::client::pool) fn is_reused(&self) -> bool {
        self.reused
    }

    /// Returns the selected protocol-neutral connection.
    pub(in crate::client::pool) fn connection(&self) -> &Arc<ConnectionState> {
        &self.connection
    }

    /// Transfers the sender and both lease endpoints into one dispatch attempt.
    ///
    /// # Panics
    ///
    /// Panics if dispatch parts were already taken.
    pub(in crate::client::pool) fn take_dispatch_parts(&mut self) -> H2DispatchParts {
        let sender = self
            .sender
            .take()
            .expect("HTTP/2 activation dispatch parts already taken");
        let trace = H2EndpointTrace::new(self);
        H2DispatchParts {
            sender,
            send_endpoint: H2LeaseEndpoint::new(
                self.lease.clone(),
                H2Endpoint::Send,
                Some(trace.clone()),
            ),
            receive_endpoint: H2LeaseEndpoint::new(
                self.lease.clone(),
                H2Endpoint::Receive,
                Some(trace),
            ),
        }
    }

    /// Retains a requesting-cell route opportunity until acceptance or cancellation.
    fn attach_peer_gate(
        &mut self,
        requesting_cell: &Arc<OriginCell>,
        route: H2RouteId,
        waiter: WaiterId,
    ) {
        debug_assert_eq!(
            self.request_partition,
            requesting_cell.id().partition(),
            "peer HTTP/2 activation changed requesting partition"
        );
        assert!(
            self.gate.is_none(),
            "HTTP/2 activation acquired two requesting-cell gates"
        );
        self.gate = Some(H2GateToken::peer(requesting_cell, route, waiter));
    }

    /// Returns close authority for the connection-owning generation.
    pub(in crate::client::pool) fn close_handle(&self) -> H2CloseHandle {
        H2CloseHandle::new(&self.cell, self.generation)
    }

    /// Converts the prospective reservation after Hyper accepts the request.
    ///
    /// # Panics
    ///
    /// Panics if dispatch parts were not transferred or the prospective
    /// reservation disappeared before acceptance.
    pub(in crate::client::pool) fn accept(mut self, dispatch: DispatchGuard) {
        assert!(
            self.sender.is_none(),
            "HTTP/2 activation accepted before dispatch parts were taken"
        );
        assert!(
            OriginCell::accept_h2_activation(&self.cell, self.generation),
            "prospective HTTP/2 activation disappeared before acceptance"
        );
        self.active = false;
        self.lease.accept(dispatch);
        if let Some(gate) = self.gate.take() {
            gate.finish();
        }
        tracing::trace!(
            connection_id = %self.connection.id(),
            request_partition = ?self.request_partition,
            connection_partition = ?self.connection.owner_partition(),
            origin_scheme = %self.connection.info().origin().scheme(),
            origin_host = self.connection.info().origin().host(),
            origin_port = ?self.connection.info().origin().port(),
            h2_generation = ?self.generation,
            "HTTP/2 request accepted"
        );
    }
}

impl Drop for H2Activation {
    fn drop(&mut self) {
        if self.active {
            OriginCell::cancel_h2_activation(&self.cell, self.generation);
            self.lease.cancel();
            if let Some(gate) = self.gate.take() {
                gate.finish();
            }
            tracing::trace!(
                connection_id = %self.connection.id(),
                request_partition = ?self.request_partition,
                connection_partition = ?self.connection.owner_partition(),
                origin_scheme = %self.connection.info().origin().scheme(),
                origin_host = self.connection.info().origin().host(),
                origin_port = ?self.connection.info().origin().port(),
                h2_generation = ?self.generation,
                "HTTP/2 activation cancelled before request acceptance"
            );
        }
    }
}

/// One activation opportunity retained until Hyper acceptance or cancellation.
struct H2GateToken {
    /// Requesting cell whose gate owns this turn.
    cell: Weak<OriginCell>,
    /// Waiter that received the activation opportunity.
    waiter: WaiterId,
    /// Local-generation or peer-route gate identity.
    kind: H2GateKind,
}

/// Cell transition run when an activation accepts or cancels.
enum H2GateKind {
    /// Gate attached to the connection cell's local generation.
    Local { generation: H2GenerationId },
    /// Gate attached to a requesting cell's peer route.
    Peer { route: H2RouteId },
}

impl H2GateToken {
    fn local(cell: &Arc<OriginCell>, generation: H2GenerationId, waiter: WaiterId) -> Self {
        Self {
            cell: Weak::from_arc(cell),
            waiter,
            kind: H2GateKind::Local { generation },
        }
    }

    fn peer(cell: &Arc<OriginCell>, route: H2RouteId, waiter: WaiterId) -> Self {
        Self {
            cell: Weak::from_arc(cell),
            waiter,
            kind: H2GateKind::Peer { route },
        }
    }

    fn finish(self) {
        if let Some(cell) = self.cell.upgrade() {
            match self.kind {
                H2GateKind::Local { generation } => {
                    OriginCell::finish_h2_gate(&cell, generation, self.waiter)
                }
                H2GateKind::Peer { route } => {
                    OriginCell::finish_peer_h2_gate(&cell, route, self.waiter)
                }
            }
        }
    }
}

/// One terminal side of an accepted HTTP/2 request lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H2Endpoint {
    /// Request-body upload lifetime.
    Send,
    /// Response future and response-body lifetime.
    Receive,
}

/// Shared request-lease phase and endpoint bits.
enum H2LeaseState {
    /// Generation count reserved before Hyper accepts the request.
    Prospective {
        /// Whether upload ownership already terminated.
        send_complete: bool,
        /// Whether response ownership already terminated.
        receive_complete: bool,
    },
    /// Hyper accepted the request and connection dispatch is retained.
    Accepted {
        /// Whether upload ownership already terminated.
        send_complete: bool,
        /// Whether response ownership already terminated.
        receive_complete: bool,
        /// Accepted-dispatch accounting released with the second endpoint.
        dispatch: DispatchGuard,
    },
    /// Both endpoints terminated or prospective dispatch was cancelled.
    Complete,
}

/// Shared endpoint state for one prospective or accepted request.
struct H2LeaseCore {
    /// Connection cell updated after both accepted endpoints terminate.
    cell: Weak<OriginCell>,
    /// Exact generation whose request count this lease owns.
    generation: H2GenerationId,
    /// Endpoint phase; never held while locking the cell or connection state.
    state: Mutex<H2LeaseState>,
}

impl H2LeaseCore {
    /// Converts prospective endpoint state to accepted state.
    fn accept(&self, dispatch: DispatchGuard) {
        let mut dispatch = Some(dispatch);
        let complete = {
            let mut state = self.state.lock();
            let (send_complete, receive_complete) = match &*state {
                H2LeaseState::Prospective {
                    send_complete,
                    receive_complete,
                } => (*send_complete, *receive_complete),
                H2LeaseState::Accepted { .. } | H2LeaseState::Complete => {
                    drop(state);
                    panic!("HTTP/2 request lease accepted outside prospective state");
                }
            };
            if send_complete && receive_complete {
                *state = H2LeaseState::Complete;
                true
            } else {
                *state = H2LeaseState::Accepted {
                    send_complete,
                    receive_complete,
                    dispatch: dispatch
                        .take()
                        .expect("HTTP/2 dispatch guard disappeared before acceptance"),
                };
                false
            }
        };
        drop(dispatch);
        if complete {
            self.complete_generation();
        }
    }

    /// Cancels endpoint state after the prospective reservation ends.
    fn cancel(&self) {
        let mut state = self.state.lock();
        if matches!(*state, H2LeaseState::Prospective { .. }) {
            *state = H2LeaseState::Complete;
        }
    }

    /// Marks one endpoint terminal and releases the accepted lease on the second.
    fn complete_endpoint(&self, endpoint: H2Endpoint) -> bool {
        let dispatch = {
            let mut state = self.state.lock();
            match &mut *state {
                H2LeaseState::Prospective {
                    send_complete,
                    receive_complete,
                } => {
                    match endpoint {
                        H2Endpoint::Send => *send_complete = true,
                        H2Endpoint::Receive => *receive_complete = true,
                    }
                    None
                }
                H2LeaseState::Accepted {
                    send_complete,
                    receive_complete,
                    ..
                } => {
                    match endpoint {
                        H2Endpoint::Send => *send_complete = true,
                        H2Endpoint::Receive => *receive_complete = true,
                    }
                    if *send_complete && *receive_complete {
                        let previous = std::mem::replace(&mut *state, H2LeaseState::Complete);
                        let H2LeaseState::Accepted { dispatch, .. } = previous else {
                            unreachable!("completed HTTP/2 lease changed state under its lock");
                        };
                        Some(dispatch)
                    } else {
                        None
                    }
                }
                H2LeaseState::Complete => None,
            }
        };
        let request_complete = dispatch.is_some();
        if let Some(dispatch) = dispatch {
            // Dispatch completion takes the connection lifecycle lock. Keep it
            // outside the request-lease lock so endpoint completion cannot
            // nest pool synchronization.
            drop(dispatch);
            self.complete_generation();
        }
        request_complete
    }

    /// Releases the generation count after the lease lock is released.
    fn complete_generation(&self) {
        if let Some(cell) = self.cell.upgrade() {
            OriginCell::complete_h2_request(&cell, self.generation);
        }
    }
}

/// Structured identity retained by one request-lease endpoint.
#[derive(Clone)]
struct H2EndpointTrace {
    /// Partition that issued the request.
    request_partition: PartitionId,
    /// Stable connection identity and origin metadata.
    connection: Arc<ConnectionInfo>,
    /// Generation that owns the endpoint.
    generation: H2GenerationId,
}

impl H2EndpointTrace {
    fn new(activation: &H2Activation) -> Self {
        Self {
            request_partition: activation.request_partition,
            connection: activation.connection.info().clone(),
            generation: activation.generation,
        }
    }
}

/// Linear guard for one request-lease endpoint.
pub(in crate::client::pool) struct H2LeaseEndpoint {
    /// Shared request-lease state.
    core: Arc<H2LeaseCore>,
    /// Send or receive side owned by this guard.
    endpoint: H2Endpoint,
    /// Structured fields emitted on terminal completion.
    trace: Option<H2EndpointTrace>,
    /// Whether drop must finish this endpoint.
    active: bool,
}

impl H2LeaseEndpoint {
    fn new(core: Arc<H2LeaseCore>, endpoint: H2Endpoint, trace: Option<H2EndpointTrace>) -> Self {
        Self {
            core,
            endpoint,
            trace,
            active: true,
        }
    }

    /// Creates a detached endpoint and observation handle for body tests.
    #[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
    fn for_test(cell: &Arc<OriginCell>, endpoint: H2Endpoint) -> (Self, H2LeaseProbe) {
        let core = Arc::new(H2LeaseCore {
            cell: Weak::from_arc(cell),
            generation: H2GenerationId(0),
            state: Mutex::new(H2LeaseState::Prospective {
                send_complete: false,
                receive_complete: false,
            }),
        });
        (
            Self::new(core.clone(), endpoint, None),
            H2LeaseProbe { core },
        )
    }

    /// Creates a detached send endpoint and observation handle for body tests.
    #[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
    pub(in crate::client::pool) fn send_for_test(cell: &Arc<OriginCell>) -> (Self, H2LeaseProbe) {
        Self::for_test(cell, H2Endpoint::Send)
    }

    /// Creates a detached receive endpoint and observation handle for body tests.
    #[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
    pub(in crate::client::pool) fn receive_for_test(
        cell: &Arc<OriginCell>,
    ) -> (Self, H2LeaseProbe) {
        Self::for_test(cell, H2Endpoint::Receive)
    }

    /// Completes this endpoint before dropping the guard.
    pub(in crate::client::pool) fn complete(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        if self.active {
            self.active = false;
            let request_complete = self.core.complete_endpoint(self.endpoint);
            if let Some(trace) = self.trace.take() {
                tracing::trace!(
                    connection_id = %trace.connection.id(),
                    request_partition = ?trace.request_partition,
                    connection_partition = ?trace.connection.owner_partition(),
                    origin_scheme = %trace.connection.origin().scheme(),
                    origin_host = trace.connection.origin().host(),
                    origin_port = ?trace.connection.origin().port(),
                    h2_generation = ?trace.generation,
                    endpoint = ?self.endpoint,
                    request_complete,
                    "HTTP/2 request endpoint completed"
                );
            }
        }
    }
}

impl Drop for H2LeaseEndpoint {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Test observation of a request-lease endpoint's production state.
#[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
pub(in crate::client::pool) struct H2LeaseProbe {
    /// Production lease state observed by the test.
    core: Arc<H2LeaseCore>,
}

#[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
impl H2LeaseProbe {
    /// Returns whether the send endpoint reached its terminal transition.
    pub(in crate::client::pool) fn send_complete(&self) -> bool {
        matches!(
            &*self.core.state.lock(),
            H2LeaseState::Prospective {
                send_complete: true,
                ..
            } | H2LeaseState::Accepted {
                send_complete: true,
                ..
            } | H2LeaseState::Complete
        )
    }

    /// Returns whether the receive endpoint reached its terminal transition.
    pub(in crate::client::pool) fn receive_complete(&self) -> bool {
        matches!(
            &*self.core.state.lock(),
            H2LeaseState::Prospective {
                receive_complete: true,
                ..
            } | H2LeaseState::Accepted {
                receive_complete: true,
                ..
            } | H2LeaseState::Complete
        )
    }
}

impl OriginCell {
    /// Reserves a prospective request lease from one exact local generation.
    pub(in crate::client::pool) fn activate_h2(
        cell: &Arc<Self>,
        generation: H2GenerationId,
    ) -> Option<H2Activation> {
        let (parts, advertisement) = {
            let mut state = cell.state.lock();
            let parts = state.h2.activate(generation)?;
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (parts, advertisement)
        };
        Self::publish_h2_advertisement(cell, advertisement);
        Some(H2Activation::new(cell.clone(), generation, parts, None))
    }

    /// Converts a prospective activation to an accepted generation lease.
    fn accept_h2_activation(cell: &Arc<Self>, generation: H2GenerationId) -> bool {
        let deadline = cell.idle_deadline();
        let (accepted, reset_deadline) = {
            let mut state = cell.state.lock();
            let accepted = state.h2.accept(generation);
            let reset_deadline = accepted && state.h2.reset_idle_deadline(generation, deadline);
            state.assert_consistent();
            (accepted, reset_deadline)
        };
        if reset_deadline {
            cell.notify_maintenance(deadline);
        }
        accepted
    }

    /// Cancels one prospective activation after sender rejection or task drop.
    fn cancel_h2_activation(cell: &Arc<Self>, generation: H2GenerationId) {
        // Keep the detached generation outside the lock's unwind scope. Its
        // sender drop may wake Hyper's driver.
        let (removed, advertisement) = {
            let mut state = cell.state.lock();
            let removed = state.h2.cancel(generation);
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (removed, advertisement)
        };
        drop(removed);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::service_h2_waiters(cell);
    }

    /// Releases one accepted lease after its second endpoint terminates.
    fn complete_h2_request(cell: &Arc<Self>, generation: H2GenerationId) {
        // Keep the detached generation outside the lock's unwind scope. Its
        // sender drop may wake Hyper's driver.
        let (removed, advertisement) = {
            let mut state = cell.state.lock();
            let removed = state.h2.complete_request(generation);
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (removed, advertisement)
        };
        drop(removed);
        Self::publish_h2_advertisement(cell, advertisement);
    }

    /// Offers one local activation while preserving the generation cutoff.
    fn service_h2_gate_locked(
        cell: &Arc<Self>,
        state: &mut CellState,
        returned_event: &mut Option<AcquisitionEvent>,
    ) -> Option<WaiterInstall> {
        let generation = state.h2.accepting()?;
        let has_prioritized = state
            .h2
            .priority_cutoff()
            .is_some_and(|cutoff| state.waiters.has_h2_candidate_through(cutoff));
        let cutoff = match state.h2.next_gate_turn(has_prioritized) {
            GateTurn::Unavailable => return None,
            GateTurn::Open => None,
            GateTurn::Through(cutoff) => Some(cutoff),
        };
        state.waiters.oldest_h2_candidate()?;

        let h2 = &mut state.h2;
        let waiters = &mut state.waiters;
        let mut install = waiters.install_h2(
            cutoff,
            |waiter| {
                let parts = h2
                    .activate(generation)
                    .expect("accepting HTTP/2 generation could not create a gated activation");
                let gated = h2.begin_gate_activation(waiter);
                AcquisitionResult::H2(H2Activation::new(
                    cell.clone(),
                    generation,
                    parts,
                    gated.then(|| H2GateToken::local(cell, generation, waiter)),
                ))
            },
            &cell.eligibility_group,
        );
        *returned_event = install.returned_event.take();
        install.demand_updates = state.publishable_demand_updates(install.demand_updates);
        state.assert_consistent();
        install.waiter.is_some().then_some(install)
    }

    /// Runs publication, fallback, and wake work after the cell lock is released.
    fn finish_h2_install(cell: &Arc<Self>, install: Option<WaiterInstall>) {
        let Some(install) = install else {
            return;
        };
        if let Some(admission) = &cell.admission {
            for snapshot in install.demand_updates.into_iter().flatten() {
                super::super::admission::OriginAdmission::publish_demand(
                    admission,
                    cell.id.partition(),
                    snapshot,
                );
            }
        }
        drop(install.returned_event);
        if let Some(waker) = install.waker {
            waker.wake();
        }
    }

    /// Publishes one complete local-generation report after cell unlock.
    fn publish_h2_advertisement(
        cell: &Arc<Self>,
        snapshot: Option<super::super::admission::H2AdvertisementSnapshot>,
    ) {
        if let (Some(admission), Some(snapshot)) = (&cell.admission, snapshot) {
            super::super::admission::OriginAdmission::update_h2_advertisement(
                admission,
                cell.id.partition(),
                cell.eligibility_group.clone(),
                snapshot,
            );
        }
    }

    /// Publishes the current local demand after its last H2 route disappears.
    fn publish_current_demand(
        cell: &Arc<Self>,
        snapshot: Option<super::super::admission::DemandSnapshot>,
    ) {
        if let (Some(admission), Some(snapshot)) = (&cell.admission, snapshot) {
            super::super::admission::OriginAdmission::publish_demand(
                admission,
                cell.id.partition(),
                snapshot,
            );
        }
    }

    /// Advances a generation gate after one activation accepts or cancels.
    fn finish_h2_gate(cell: &Arc<Self>, generation: H2GenerationId, waiter: WaiterId) {
        let mut returned_event = None;
        let (install, advertisement) = {
            let mut state = cell.state.lock();
            if !state.h2.finish_gate_activation(generation, waiter) {
                return;
            }
            let install = Self::service_h2_gate_locked(cell, &mut state, &mut returned_event);
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (install, advertisement)
        };
        drop(returned_event);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::finish_h2_install(cell, install);
    }

    /// Offers the accepting local generation to one queued acquisition.
    pub(in crate::client::pool) fn service_h2_waiters(cell: &Arc<Self>) {
        let mut returned_event = None;
        let (install, advertisement) = {
            let mut state = cell.state.lock();
            let install = Self::service_h2_gate_locked(cell, &mut state, &mut returned_event);
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (install, advertisement)
        };
        drop(returned_event);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::finish_h2_install(cell, install);
    }

    /// Returns whether one exact connection generation still accepts activations.
    pub(in crate::client::pool) fn h2_generation_is_accepting(
        cell: &Arc<Self>,
        generation: H2GenerationId,
    ) -> bool {
        cell.state.lock().h2.is_accepting(generation)
    }

    /// Installs requesting-cell visibility for one peer generation publication.
    ///
    /// The named local demand remains queued behind the route gate. Advancing
    /// its snapshot version makes admission acknowledgement authoritative
    /// without losing the demand if this exact route later becomes stale.
    pub(in crate::client::pool) fn install_h2_route(
        cell: &Arc<Self>,
        route: H2Route,
        group: &super::super::partition::EligibilityGroup,
        demand: super::super::admission::DemandId,
    ) -> bool {
        if &cell.eligibility_group != group || route.connection_partition() == cell.id.partition() {
            return false;
        }
        let mut state = cell.state.lock();
        if !state.waiters.suppress_published_demand(demand) {
            return false;
        }
        let cutoff = state.waiters.publication_cutoff();
        state.h2.install_peer_route(route, cutoff);
        state.assert_consistent();
        true
    }

    /// Activates queued requests through the currently visible peer route.
    ///
    /// Preparing the requesting-cell opportunity, activating the connection
    /// generation, and committing the result each use a separate lock scope.
    pub(in crate::client::pool) fn service_peer_h2_waiters(cell: &Arc<Self>) {
        loop {
            let prepared = {
                let mut state = cell.state.lock();
                let CellState { h2, waiters, .. } = &mut *state;
                let prepared = h2.prepare_peer_activation(waiters);
                state.assert_consistent();
                prepared
            };
            let Some(prepared) = prepared else {
                return;
            };
            let route_id = prepared.route.id();
            let Some(activation) = prepared.route.activate(cell.id.partition()) else {
                let snapshot = {
                    let mut state = cell.state.lock();
                    if !state.h2.clear_peer_route(route_id) {
                        return;
                    }
                    let snapshot = state
                        .waiters
                        .current_demand_snapshot(&cell.eligibility_group);
                    state.assert_consistent();
                    snapshot
                };
                if let (Some(admission), Some(snapshot)) = (&cell.admission, snapshot) {
                    super::super::admission::OriginAdmission::publish_demand(
                        admission,
                        cell.id.partition(),
                        snapshot,
                    );
                }
                return;
            };

            let mut activation = Some(activation);
            let mut returned_event = None;
            let install = {
                let mut state = cell.state.lock();
                let current = state
                    .h2
                    .peer_activation_is_current(&prepared, &state.waiters);
                if !current {
                    state.h2.finish_peer_crossing(route_id, prepared.waiter);
                    if prepared.gated {
                        state.h2.finish_peer_gate(route_id, prepared.waiter);
                    }
                    state.assert_consistent();
                    None
                } else {
                    if prepared.gated {
                        activation
                            .as_mut()
                            .expect("peer HTTP/2 activation disappeared")
                            .attach_peer_gate(cell, route_id, prepared.waiter);
                    }
                    let mut install = state.waiters.install_h2(
                        prepared.cutoff,
                        |_| {
                            AcquisitionResult::H2(
                                activation
                                    .take()
                                    .expect("peer HTTP/2 activation was installed twice"),
                            )
                        },
                        &cell.eligibility_group,
                    );
                    returned_event = install.returned_event.take();
                    install.demand_updates =
                        state.publishable_demand_updates(install.demand_updates);
                    state.h2.finish_peer_crossing(route_id, prepared.waiter);
                    if install.waiter.is_none() && prepared.gated {
                        state.h2.finish_peer_gate(route_id, prepared.waiter);
                    }
                    state.assert_consistent();
                    install.waiter.is_some().then_some(install)
                }
            };
            drop(activation);
            drop(returned_event);
            if install.is_some() {
                Self::finish_h2_install(cell, install);
                return;
            }
        }
    }

    /// Advances an exact peer-route gate after acceptance or cancellation.
    fn finish_peer_h2_gate(cell: &Arc<Self>, route: H2RouteId, waiter: WaiterId) {
        let finished = {
            let mut state = cell.state.lock();
            let finished = state.h2.finish_peer_gate(route, waiter);
            state.assert_consistent();
            finished
        };
        if finished {
            Self::service_peer_h2_waiters(cell);
        }
    }
}

/// Generation-specific close authority that does not retain the cell.
#[derive(Clone, Debug)]
pub(in crate::client::pool) struct H2CloseHandle {
    /// Cell weak reference so driver lifetime does not retain the pool.
    cell: Weak<OriginCell>,
    /// Exact generation this handle may close.
    generation: H2GenerationId,
}

impl H2CloseHandle {
    /// Creates close authority for one installed generation.
    pub(in crate::client::pool) fn new(cell: &Arc<OriginCell>, generation: H2GenerationId) -> Self {
        Self {
            cell: Weak::from_arc(cell),
            generation,
        }
    }

    /// Begins drain when the cell still contains this generation.
    pub(in crate::client::pool) fn close(
        &self,
        reason: super::super::connection::CloseReason,
    ) -> bool {
        self.cell
            .upgrade()
            .is_some_and(|cell| OriginCell::close_h2(&cell, self.generation, reason))
    }
}

/// Closes an H2 generation if its owner-runtime driver ends or is dropped.
pub(in crate::client::pool) struct H2DriverGuard {
    /// Exact-generation close authority.
    close: H2CloseHandle,
    /// Whether drop must report owner-runtime shutdown.
    active: bool,
}

impl H2DriverGuard {
    /// Arms generation cleanup before driver submission.
    pub(in crate::client::pool) fn new(close: H2CloseHandle) -> Self {
        Self {
            close,
            active: true,
        }
    }

    /// Records ordinary driver completion.
    pub(in crate::client::pool) fn protocol_closed(mut self) {
        self.active = false;
        self.close
            .close(super::super::connection::CloseReason::ProtocolClosed);
    }
}

impl Drop for H2DriverGuard {
    fn drop(&mut self) {
        if self.active {
            self.close
                .close(super::super::connection::CloseReason::OwnerRuntimeShutdown);
        }
    }
}

impl OriginCell {
    /// Selects an accepting local generation before consulting a peer route.
    pub(in crate::client::pool) fn select_h2(cell: &Arc<Self>) -> Option<H2Activation> {
        let local = {
            let mut state = cell.state.lock();
            let queued = state.waiters.has_h2_candidate();
            if state.h2.direct_is_allowed(queued) {
                let generation = state.h2.accepting()?;
                let parts = state.h2.activate(generation)?;
                state.assert_consistent();
                let advertisement = state.take_h2_advertisement_update();
                Some((generation, parts, advertisement))
            } else {
                None
            }
        };
        if let Some((generation, parts, advertisement)) = local {
            Self::publish_h2_advertisement(cell, advertisement);
            return Some(H2Activation::new(cell.clone(), generation, parts, None));
        }

        let route = {
            let state = cell.state.lock();
            state.h2.open_peer_route(state.waiters.has_h2_candidate())?
        };
        let route_id = route.id();
        let Some(activation) = route.activate(cell.id.partition()) else {
            Self::clear_stale_peer_route(cell, route_id);
            return None;
        };
        let current = {
            let state = cell.state.lock();
            state
                .h2
                .direct_peer_route_is_current(route_id, state.waiters.has_h2_candidate())
        };
        current.then_some(activation)
    }

    /// Removes one exact stale route and republishes the requesting demand.
    fn clear_stale_peer_route(cell: &Arc<Self>, route: H2RouteId) {
        let snapshot = {
            let mut state = cell.state.lock();
            if !state.h2.clear_peer_route(route) {
                return;
            }
            let snapshot = state
                .waiters
                .current_demand_snapshot(&cell.eligibility_group);
            state.assert_consistent();
            snapshot
        };
        Self::publish_current_demand(cell, snapshot);
    }

    /// Places a post-ALPN waiter behind an already accepting generation.
    pub(in crate::client::pool) fn join_h2_generation(
        cell: &Arc<Self>,
        waiter: WaiterId,
        generation: H2GenerationId,
    ) -> H2GenerationJoin {
        let mut returned_event = None;
        let (install, advertisement) = {
            let mut state = cell.state.lock();
            if !state.waiters.is_launching_h2_candidate(waiter) {
                return H2GenerationJoin::WaiterCompleted;
            }
            if !state.h2.prioritize_waiter(generation, waiter) {
                return H2GenerationJoin::GenerationChanged;
            }
            let install = Self::service_h2_gate_locked(cell, &mut state, &mut returned_event);
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (install, advertisement)
        };
        drop(returned_event);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::finish_h2_install(cell, install);
        H2GenerationJoin::Joined
    }

    /// Atomically selects, joins, or installs the cell's post-ALPN flight.
    pub(in crate::client::pool) fn install_or_join_h2_flight(
        &self,
        waiter: WaiterId,
    ) -> H2FlightInstall {
        let mut state = self.state.lock();
        let result = state.h2.install_or_join_flight(waiter);
        state.assert_consistent();
        result
    }

    /// Installs one successful flight as the accepting generation.
    pub(in crate::client::pool) fn complete_h2_flight(
        cell: &Arc<Self>,
        flight: H2FlightId,
        connection: Arc<ConnectionState>,
        sender: H2Sender,
        idle_deadline: Option<SystemTime>,
    ) -> Result<H2GenerationId, (Arc<ConnectionState>, H2Sender)> {
        let mut returned_event = None;
        let (completion, install, advertisement) = {
            let mut state = cell.state.lock();
            let completion = state
                .h2
                .complete_flight(flight, connection, sender, idle_deadline);
            let install = if completion.is_ok() {
                let cutoff = state.waiters.publication_cutoff();
                state.h2.prioritize_through(cutoff);
                Self::service_h2_gate_locked(cell, &mut state, &mut returned_event)
            } else {
                None
            };
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (completion, install, advertisement)
        };
        drop(returned_event);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::finish_h2_install(cell, install);
        if completion.is_ok() {
            cell.notify_maintenance(idle_deadline);
        }
        completion
    }

    /// Retires one failed flight and returns its retained participants.
    pub(in crate::client::pool) fn fail_h2_flight(
        &self,
        flight: H2FlightId,
    ) -> Option<Vec<WaiterId>> {
        let mut state = self.state.lock();
        let participants = state.h2.fail_flight(flight);
        state.assert_consistent();
        participants
    }

    /// Returns the complete current advertisement for an unlocked crossing.
    pub(in crate::client::pool) fn report_h2_advertisement(
        &self,
    ) -> super::super::admission::H2AdvertisementSnapshot {
        let mut state = self.state.lock();
        let snapshot = state.report_h2_advertisement();
        state.assert_consistent();
        snapshot
    }

    /// Reclaims one exact idle generation and reports its resulting availability.
    pub(in crate::client::pool) fn reclaim_idle_h2(
        cell: &Arc<Self>,
        generation: H2GenerationId,
    ) -> (
        super::super::admission::H2AdvertisementSnapshot,
        Option<ConnectionId>,
    ) {
        // A zero-request generation may hold the last capacity-owning
        // connection reference. Keep it outside the cell-lock unwind scope.
        let detached;
        let (advertisement, demand) = {
            let mut state = cell.state.lock();
            detached = state.h2.begin_idle_reclaim(generation);
            let advertisement = state.report_h2_advertisement();
            let demand = detached.as_ref().and_then(|_| {
                state
                    .waiters
                    .current_demand_snapshot(&cell.eligibility_group)
            });
            state.assert_consistent();
            (advertisement, demand)
        };
        let Some(detached) = detached else {
            return (advertisement, None);
        };
        let H2CloseTransition {
            connection,
            pending_waiters,
            removed_generation,
        } = detached;
        assert!(
            pending_waiters.is_empty(),
            "idle HTTP/2 reclaim detached pending generation waiters"
        );
        let connection_id = connection.id();
        drop(removed_generation);
        Self::publish_current_demand(cell, demand);
        let reclaimed = connection.logical_close(super::super::connection::CloseReason::Reclaimed);
        (advertisement, reclaimed.then_some(connection_id))
    }

    /// Moves one exact generation to draining and closes its connection.
    pub(super) fn close_h2(
        cell: &Arc<Self>,
        generation: H2GenerationId,
        reason: super::super::connection::CloseReason,
    ) -> bool {
        // A zero-request generation may hold the last capacity-owning
        // connection reference. Keep it outside the cell-lock unwind scope.
        let detached;
        let (advertisement, demand) = {
            let mut state = cell.state.lock();
            detached = state.h2.begin_close(generation);
            let advertisement = detached
                .as_ref()
                .and_then(|_| state.take_h2_advertisement_update());
            let demand = detached.as_ref().and_then(|_| {
                state
                    .waiters
                    .current_demand_snapshot(&cell.eligibility_group)
            });
            state.assert_consistent();
            (advertisement, demand)
        };
        let Some(detached) = detached else {
            return false;
        };
        let H2CloseTransition {
            connection,
            pending_waiters,
            removed_generation,
        } = detached;
        drop(removed_generation);
        Self::publish_h2_advertisement(cell, advertisement);
        Self::publish_current_demand(cell, demand);
        for waiter in pending_waiters {
            cell.complete_establishment(waiter, AcquisitionResult::Reacquire);
        }
        connection.logical_close(reason)
    }

    /// Returns the exact accepting generation for publication.
    #[cfg(test)]
    pub(in crate::client::pool) fn accepting_h2_generation(&self) -> Option<H2GenerationId> {
        self.state.lock().h2.accepting()
    }

    /// Returns the prospective and accepted request counts for one generation.
    #[cfg(test)]
    pub(in crate::client::pool) fn h2_request_counts(
        &self,
        generation: H2GenerationId,
    ) -> Option<(usize, usize)> {
        self.state
            .lock()
            .h2
            .generations
            .get(&generation)
            .map(|record| (record.prospective, record.accepted))
    }

    /// Installs an accepting generation without a Hyper handshake.
    #[cfg(test)]
    pub(in crate::client::pool) fn install_h2_for_test(
        cell: &Arc<Self>,
        connection: Arc<ConnectionState>,
        sender_id: u64,
        idle_deadline: Option<SystemTime>,
    ) -> H2GenerationId {
        let (generation, advertisement) = {
            let mut state = cell.state.lock();
            let generation = state.h2.install_generation(
                BTreeSet::new(),
                connection,
                H2Sender::test(sender_id),
                idle_deadline,
            );
            state.assert_consistent();
            let advertisement = state.take_h2_advertisement_update();
            (generation, advertisement)
        };
        Self::publish_h2_advertisement(cell, advertisement);
        cell.notify_maintenance(idle_deadline);
        generation
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::ProtocolRequirement;
    use crate::client::pool::connection::{CloseReason, ConnectionInfo};
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::EligibilityGroup;
    use aws_smithy_runtime_api::client::connection::ConnectionId;
    use http_1x::uri::Scheme;

    fn cell() -> Arc<OriginCell> {
        Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    fn bounded_cell(
        admission: &Arc<crate::client::pool::admission::OriginAdmission>,
        partition: usize,
    ) -> Arc<OriginCell> {
        crate::client::pool::admission::OriginAdmission::register_cell(
            admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(partition),
                OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
                EligibilityGroup::Pool,
                Some(admission.clone()),
                None,
            )),
        )
    }

    fn connection(
        id: u64,
    ) -> (
        Arc<ConnectionState>,
        super::super::super::connection::PhysicalConnectionGuard,
    ) {
        ConnectionState::unbounded(ConnectionInfo::for_test(
            ConnectionId::new(id),
            PartitionId::from_index(1),
        ))
    }

    fn begin_waiter(cell: &Arc<OriginCell>) -> WaiterId {
        let waiter = OriginCell::register_waiter(cell, ProtocolRequirement::H2Required);
        let event = cell
            .take_ready_event(waiter)
            .expect("unbounded H2 waiter did not receive establishment authority");
        let super::super::AcquisitionEvent::Establish(permit) = event else {
            panic!("new H2 waiter completed before establishment");
        };
        assert!(cell.start_establishment(waiter));
        drop(permit);
        waiter
    }

    fn install_generation(
        cell: &Arc<OriginCell>,
        connection_id: u64,
    ) -> (
        H2GenerationId,
        Arc<ConnectionState>,
        super::super::super::connection::PhysicalConnectionGuard,
    ) {
        let waiter = begin_waiter(cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(waiter) else {
            panic!("fresh cell did not create an HTTP/2 flight");
        };
        let (connection, physical) = connection(connection_id);
        let generation = OriginCell::complete_h2_flight(
            cell,
            flight,
            connection.clone(),
            H2Sender::test(connection_id),
            None,
        )
        .expect("fresh flight did not install");
        let event = cell
            .take_ready_event(waiter)
            .expect("flight completion did not satisfy its waiter");
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            activation,
        )) = event
        else {
            panic!("flight completion produced a non-H2 result");
        };
        drop(activation);
        (generation, connection, physical)
    }

    fn generation_counts(cell: &OriginCell, generation: H2GenerationId) -> (usize, usize) {
        cell.h2_request_counts(generation)
            .expect("generation was not installed")
    }

    #[test]
    fn flight_converges_participants_and_cancellation() {
        let mut records = H2Records::default();
        let first = WaiterId(1);
        let second = WaiterId(2);

        let H2FlightInstall::Driver(flight) = records.install_or_join_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            records.install_or_join_flight(second),
            H2FlightInstall::Joined
        ));
        records.cancel_flight_participant(second);
        assert_eq!(Some(vec![first]), records.fail_flight(flight));
        assert!(records.flight.is_none());
    }

    #[test]
    fn accepting_generation_prevents_a_second_flight() {
        let mut records = H2Records::default();
        let (connection, _physical) = connection(1);
        let generation =
            records.install_generation(BTreeSet::new(), connection, H2Sender::test(1), None);

        assert!(matches!(
            records.install_or_join_flight(WaiterId(1)),
            H2FlightInstall::Accepting(current) if current == generation
        ));
        assert!(records.flight.is_none());
    }

    #[test]
    fn generation_join_does_not_retain_a_waiter_already_served_by_the_gate() {
        let cell = cell();
        let first = begin_waiter(&cell);
        let second = begin_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first waiter did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));

        let (connection, _physical) = connection(1);
        let generation =
            OriginCell::complete_h2_flight(&cell, flight, connection, H2Sender::test(1), None)
                .expect("flight did not install");

        assert_eq!(
            H2GenerationJoin::WaiterCompleted,
            OriginCell::join_h2_generation(&cell, first, generation)
        );
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            activation,
        )) = cell
            .take_ready_event(first)
            .expect("generation gate did not serve the first waiter")
        else {
            panic!("first waiter received a non-H2 result");
        };

        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::PoolDropped,
        ));
        drop(activation);
        assert!(matches!(
            cell.take_ready_event(second),
            Some(super::super::AcquisitionEvent::Complete(
                super::super::AcquisitionResult::Reacquire
            ))
        ));
        assert_eq!(0, cell.retained_waiters_for_test());
    }

    #[test]
    fn generation_gate_services_committed_waiters_before_direct_arrivals() {
        let cell = cell();
        let first = begin_waiter(&cell);
        let second = begin_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));
        let (connection, _physical) = connection(1);
        OriginCell::complete_h2_flight(&cell, flight, connection, H2Sender::test(1), None)
            .expect("flight did not install");

        assert!(
            OriginCell::select_h2(&cell).is_none(),
            "direct arrival bypassed committed waiters"
        );
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first waiter was not served")
        else {
            panic!("first waiter received a non-H2 result");
        };
        drop(first_activation);

        assert!(
            OriginCell::select_h2(&cell).is_none(),
            "direct arrival bypassed the second committed waiter"
        );
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            second_activation,
        )) = cell
            .take_ready_event(second)
            .expect("second waiter was not served")
        else {
            panic!("second waiter received a non-H2 result");
        };
        drop(second_activation);

        assert!(
            OriginCell::select_h2(&cell).is_some(),
            "gate did not open after committed waiters drained"
        );
    }

    #[test]
    fn cancelling_an_open_gate_activation_services_the_next_waiter() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test(NonZeroUsize::new(1).unwrap());
        let cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
                EligibilityGroup::Pool,
                Some(admission.clone()),
                None,
            )),
        );
        let (connection, _physical) = ConnectionState::bounded(
            ConnectionInfo::for_test(ConnectionId::new(1), cell.id().partition()),
            OriginAdmission::lease_for_test(&admission),
        );
        let generation = OriginCell::install_h2_for_test(&cell, connection, 1, None);
        cell.state.lock().h2.prioritize_through(Some(WaiterId(0)));

        let first = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first waiter was not served")
        else {
            panic!("first waiter received a non-H2 result");
        };
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        let third = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        assert_eq!((1, 0), generation_counts(&cell, generation));

        drop(first_activation);
        assert_eq!((1, 0), generation_counts(&cell, generation));
        assert!(OriginCell::cancel_waiter(&cell, second));
        assert_eq!(
            (1, 0),
            generation_counts(&cell, generation),
            "cancelling an open-gate activation stranded its successor"
        );

        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            third_activation,
        )) = cell
            .take_ready_event(third)
            .expect("successor was not served after cancellation")
        else {
            panic!("successor received a non-H2 result");
        };
        drop(third_activation);
        assert_eq!((0, 0), generation_counts(&cell, generation));
        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }

    #[test]
    fn cancelling_an_unserved_flight_participant_prunes_the_generation() {
        let cell = cell();
        let first = begin_waiter(&cell);
        let second = begin_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));
        let (connection, _physical) = connection(1);
        let generation =
            OriginCell::complete_h2_flight(&cell, flight, connection, H2Sender::test(1), None)
                .expect("flight did not install");
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first participant was not served")
        else {
            panic!("first participant received a non-H2 result");
        };

        assert!(OriginCell::cancel_waiter(&cell, second));
        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::PoolDropped,
        ));
        drop(first_activation);
        assert_eq!(0, cell.retained_waiters_for_test());
    }

    #[test]
    fn returned_h1_prunes_a_transferred_h2_waiter() {
        let cell = cell();
        let first = begin_waiter(&cell);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let super::super::AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(second)
            .expect("second participant did not receive establishment authority")
        else {
            panic!("second participant completed before establishment");
        };
        assert!(cell.start_establishment(second));
        drop(permit);

        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));
        let (h2_connection, _h2_physical) = connection(1);
        let generation =
            OriginCell::complete_h2_flight(&cell, flight, h2_connection, H2Sender::test(1), None)
                .expect("flight did not install");
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first participant was not served")
        else {
            panic!("first participant received a non-H2 result");
        };

        let (h1_connection, _h1_physical) = connection(2);
        let returning = OriginCell::install_selected_h1(
            &cell,
            h1_connection,
            super::super::h1::H1Sender::test(2),
        );
        drop(returning);
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H1(
            selection,
        )) = cell
            .take_ready_event(second)
            .expect("returned HTTP/1 sender did not serve the compatible waiter")
        else {
            panic!("compatible waiter received a non-H1 result");
        };
        drop(selection);

        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::PoolDropped,
        ));
        drop(first_activation);
        assert_eq!(0, cell.retained_waiters_for_test());
    }

    #[test]
    fn later_local_activation_does_not_hide_an_open_generation_from_peers() {
        let mut records = H2Records::default();
        let committed = WaiterId(1);
        let later = WaiterId(2);
        let H2FlightInstall::Driver(flight) = records.install_or_join_flight(committed) else {
            panic!("first participant did not become the flight driver");
        };
        let (connection, _physical) = connection(1);
        let generation = records
            .complete_flight(flight, connection, H2Sender::test(1), None)
            .expect("flight did not install");
        records.prioritize_through(Some(committed));

        assert_eq!(
            GateTurn::Through(committed),
            records.next_gate_turn(true),
            "committed waiter did not retain initial priority"
        );
        records.begin_gate_activation(committed);
        assert_eq!(None, records.publishable_generation());
        assert!(records.finish_gate_activation(generation, committed));

        assert_eq!(GateTurn::Open, records.next_gate_turn(false));
        records.begin_gate_activation(later);
        assert_eq!(
            Some(generation),
            records.publishable_generation(),
            "a post-cutoff local activation hid the generation from peers"
        );
    }

    #[test]
    fn generation_close_reacquires_unserved_flight_participant() {
        let cell = cell();
        let first = begin_waiter(&cell);
        let second = begin_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));
        let (connection, _physical) = connection(1);
        let generation =
            OriginCell::complete_h2_flight(&cell, flight, connection, H2Sender::test(1), None)
                .expect("flight did not install");

        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first participant was not served")
        else {
            panic!("first participant received a non-H2 result");
        };
        assert!(cell.take_ready_event(second).is_none());

        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::Poisoned
        ));
        assert!(matches!(
            cell.take_ready_event(second),
            Some(super::super::AcquisitionEvent::Complete(
                super::super::AcquisitionResult::Reacquire
            ))
        ));
        drop(first_activation);
        assert_eq!(0, cell.retained_waiters_for_test());
    }

    #[test]
    fn bounded_generation_close_returns_capacity_and_reacquires_participant() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test(NonZeroUsize::new(2).unwrap());
        let cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
                EligibilityGroup::Pool,
                Some(admission.clone()),
                None,
            )),
        );
        let mut participants = Vec::new();
        for _ in 0..2 {
            let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
            let super::super::AcquisitionEvent::Establish(permit) = cell
                .take_ready_event(waiter)
                .expect("bounded participant did not receive capacity")
            else {
                panic!("bounded participant completed before establishment");
            };
            assert!(cell.start_establishment(waiter));
            participants.push((waiter, permit));
        }
        let (first, first_permit) = participants.remove(0);
        let (second, second_permit) = participants.remove(0);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));
        drop(second_permit);
        let (connection, _physical) = ConnectionState::bounded(
            ConnectionInfo::for_test(ConnectionId::new(1), cell.id().partition()),
            first_permit
                .into_lease()
                .expect("bounded permit had no lease"),
        );
        let generation =
            OriginCell::complete_h2_flight(&cell, flight, connection, H2Sender::test(1), None)
                .expect("bounded flight did not install");
        let super::super::AcquisitionEvent::Complete(super::super::AcquisitionResult::H2(
            first_activation,
        )) = cell
            .take_ready_event(first)
            .expect("first participant was not served")
        else {
            panic!("first participant received a non-H2 result");
        };

        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::Poisoned
        ));
        assert!(matches!(
            cell.take_ready_event(second),
            Some(super::super::AcquisitionEvent::Complete(
                super::super::AcquisitionResult::Reacquire
            ))
        ));
        drop(first_activation);
        assert_eq!(2, admission.available_capacity_for_test());
        assert_eq!(0, cell.retained_waiters_for_test());
    }

    #[test]
    fn generation_is_reused_only_after_hyper_acceptance() {
        let cell = cell();
        let (_generation, connection, _physical) = install_generation(&cell, 1);

        let mut activation =
            OriginCell::select_h2(&cell).expect("accepting generation was not selected");
        assert!(!activation.is_reused());
        let H2DispatchParts {
            sender: _sender,
            send_endpoint: send,
            receive_endpoint: receive,
        } = activation.take_dispatch_parts();
        let dispatch = ConnectionState::try_commit_dispatch(&connection)
            .expect("open connection rejected dispatch");
        activation.accept(dispatch);
        drop(send);
        drop(receive);

        let reused = OriginCell::select_h2(&cell).expect("accepted generation was not reusable");
        assert!(reused.is_reused());
        drop(reused);
    }

    #[test]
    #[should_panic(expected = "HTTP/2 activation dispatch parts already taken")]
    fn activation_transfers_dispatch_parts_once() {
        let cell = cell();
        let (_generation, _connection, _physical) = install_generation(&cell, 1);
        let mut activation =
            OriginCell::select_h2(&cell).expect("accepting generation was not selected");

        let _first = activation.take_dispatch_parts();
        let _second = activation.take_dispatch_parts();
    }

    #[test]
    fn prospective_activation_cancellation_returns_its_generation_count() {
        let cell = cell();
        let (generation, _connection, _physical) = install_generation(&cell, 1);

        let activation =
            OriginCell::select_h2(&cell).expect("accepting generation was not selected");
        assert_eq!((1, 0), generation_counts(&cell, generation));
        drop(activation);
        assert_eq!((0, 0), generation_counts(&cell, generation));
    }

    #[test]
    fn accepted_lease_waits_for_both_endpoints_in_either_order() {
        for send_first in [true, false] {
            let cell = cell();
            let (generation, connection, _physical) = install_generation(&cell, 1);
            let mut activation =
                OriginCell::select_h2(&cell).expect("accepting generation was not selected");
            let H2DispatchParts {
                sender: _sender,
                send_endpoint: send,
                receive_endpoint: receive,
            } = activation.take_dispatch_parts();
            let dispatch = ConnectionState::try_commit_dispatch(&connection)
                .expect("open connection rejected dispatch");
            activation.accept(dispatch);
            assert_eq!((0, 1), generation_counts(&cell, generation));
            assert_eq!(1, connection.snapshot().in_flight);

            if send_first {
                drop(send);
                assert_eq!((0, 1), generation_counts(&cell, generation));
                receive.complete();
            } else {
                receive.complete();
                assert_eq!((0, 1), generation_counts(&cell, generation));
                drop(send);
            }

            assert_eq!((0, 0), generation_counts(&cell, generation));
            assert_eq!(0, connection.snapshot().in_flight);
        }
    }

    #[test]
    fn stale_route_cannot_activate_a_replacement_or_retained_drain() {
        let cell = cell();
        let (first, _first_connection, _first_physical) = install_generation(&cell, 1);
        let stale_route = H2Route::new(&cell, first);
        let retained = OriginCell::select_h2(&cell).expect("first generation was not selectable");
        assert!(OriginCell::close_h2(
            &cell,
            first,
            CloseReason::ProtocolClosed
        ));

        let (second, _second_connection, _second_physical) = install_generation(&cell, 2);
        assert_ne!(first, second);
        assert!(
            stale_route.activate(PartitionId::from_index(2)).is_none(),
            "stale route activated a draining or replacement generation"
        );
        drop(retained);
    }

    #[test]
    fn accepting_identity_rejects_a_retained_draining_generation() {
        let cell = cell();
        let (first, _first_connection, _first_physical) = install_generation(&cell, 1);
        let retained = OriginCell::select_h2(&cell).expect("first generation was not selectable");
        assert!(OriginCell::close_h2(
            &cell,
            first,
            CloseReason::ProtocolClosed
        ));
        let (second, _second_connection, _second_physical) = install_generation(&cell, 2);

        assert!(cell.state.lock().h2.is_accepting(second));
        assert!(!cell.state.lock().h2.is_accepting(first));
        drop(retained);
    }

    #[test]
    fn activation_requires_the_exact_accepting_generation() {
        let cell = cell();
        let (first, _first_connection, _first_physical) = install_generation(&cell, 1);
        let retained = OriginCell::select_h2(&cell).expect("first generation was not selectable");
        assert!(OriginCell::close_h2(
            &cell,
            first,
            CloseReason::ProtocolClosed
        ));
        let (second, _second_connection, _second_physical) = install_generation(&cell, 2);

        {
            let mut state = cell.state.lock();
            let first_record = state
                .h2
                .generations
                .get_mut(&first)
                .expect("retained generation disappeared");
            assert_eq!(H2Residence::Draining, first_record.residence);
            first_record.residence = H2Residence::Accepting;
            assert!(
                state.h2.activate(first).is_none(),
                "non-current generation accepted an activation"
            );
            state
                .h2
                .generations
                .get_mut(&first)
                .expect("retained generation disappeared")
                .residence = H2Residence::Draining;
            state.assert_consistent();
        }

        assert!(cell.state.lock().h2.is_accepting(second));
        drop(retained);
    }

    #[test]
    fn activation_requires_accepting_residence() {
        let cell = cell();
        let (first, _first_connection, _first_physical) = install_generation(&cell, 1);
        let retained = OriginCell::select_h2(&cell).expect("first generation was not selectable");
        assert!(OriginCell::close_h2(
            &cell,
            first,
            CloseReason::ProtocolClosed
        ));
        let (second, _second_connection, _second_physical) = install_generation(&cell, 2);

        {
            let mut state = cell.state.lock();
            assert_eq!(
                H2Residence::Draining,
                state.h2.generations[&first].residence
            );
            state.h2.accepting = Some(first);
            assert!(
                state.h2.activate(first).is_none(),
                "draining generation accepted an activation"
            );
            state.h2.accepting = Some(second);
            state.assert_consistent();
        }

        drop(retained);
    }

    #[test]
    fn local_generation_excludes_a_peer_route() {
        let local_cell = cell();
        let (generation, _connection, _physical) = install_generation(&local_cell, 1);
        let peer_cell = Arc::new(OriginCell::new(
            PartitionId::from_index(2),
            local_cell.id().origin().clone(),
            EligibilityGroup::Pool,
            None,
            None,
        ));
        let route = H2Route::new(&peer_cell, H2GenerationId::for_test(99));

        let mut state = local_cell.state.lock();
        state.h2.install_peer_route(route, None);
        assert!(state.h2.peer_route.is_none());
        assert_eq!(Some(generation), state.h2.accepting());
    }

    #[test]
    fn open_generation_allows_concurrent_prospective_activations() {
        let cell = cell();
        let (generation, _connection, _physical) = install_generation(&cell, 1);

        let first = OriginCell::select_h2(&cell).expect("first activation was not selected");
        let second =
            OriginCell::select_h2(&cell).expect("open generation serialized the second activation");
        assert_eq!((2, 0), generation_counts(&cell, generation));
        drop(first);
        drop(second);
        assert_eq!((0, 0), generation_counts(&cell, generation));
    }

    #[test]
    fn consistency_checks_do_not_repanic_during_unwind() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let cell = cell();
        let (generation, _connection, _physical) = install_generation(&cell, 1);
        let activation =
            OriginCell::select_h2(&cell).expect("accepting generation did not activate");
        let (corrupt_connection, corrupt_physical) = connection(2);
        {
            let mut state = cell.state.lock();
            state.h2.generations.insert(
                H2GenerationId(99),
                H2Generation {
                    pending_waiters: BTreeSet::new(),
                    connection: corrupt_connection,
                    sender: H2Sender::test(2),
                    residence: H2Residence::Draining,
                    prospective: 0,
                    has_dispatched: false,
                    accepted: 0,
                    idle_deadline: None,
                },
            );
        }

        let result = catch_unwind(AssertUnwindSafe(move || {
            let _activation = activation;
            panic!("primary test panic");
        }));
        assert!(result.is_err(), "primary panic was not observed");

        let removed = {
            let mut state = cell.state.lock();
            state.h2.generations.remove(&H2GenerationId(99))
        };
        drop(removed);
        drop(corrupt_physical);
        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }

    #[test]
    fn removed_generation_id_cannot_activate_or_close_the_current_generation() {
        let cell = cell();
        let (first, first_connection, _first_physical) = install_generation(&cell, 1);
        let stale = H2CloseHandle::new(&cell, first);
        assert!(stale.close(CloseReason::ProtocolClosed));
        assert_eq!(
            Some(CloseReason::ProtocolClosed),
            first_connection.snapshot().close_reason
        );

        let (second, second_connection, _second_physical) = install_generation(&cell, 2);
        assert_ne!(first, second);
        assert!(OriginCell::activate_h2(&cell, first).is_none());
        assert!(!stale.close(CloseReason::Poisoned));
        assert_eq!(None, second_connection.snapshot().close_reason);
        assert_eq!(Some(second), cell.accepting_h2_generation());
    }

    #[test]
    fn close_drops_detached_capacity_only_after_cell_unlock() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let admission = OriginAdmission::for_test(NonZeroUsize::MIN);
        let cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
                EligibilityGroup::Pool,
                Some(admission.clone()),
                None,
            )),
        );
        let info = ConnectionInfo::for_test(ConnectionId::new(1), PartitionId::from_index(1));
        let (bounded_connection, physical) = ConnectionState::pending_open(info);
        bounded_connection
            .open(Some(OriginAdmission::lease_for_test(&admission)))
            .expect("bounded test connection did not open");
        let generation =
            OriginCell::install_h2_for_test(&cell, bounded_connection.clone(), 1, None);
        drop(bounded_connection);
        drop(physical);

        // Keep an unrelated empty drain so the close transition's final
        // consistency check panics after detaching the bounded connection.
        let (corrupt_connection, corrupt_physical) = connection(2);
        {
            let mut state = cell.state.lock();
            state.h2.generations.insert(
                H2GenerationId(99),
                H2Generation {
                    pending_waiters: BTreeSet::new(),
                    connection: corrupt_connection,
                    sender: H2Sender::test(2),
                    residence: H2Residence::Draining,
                    prospective: 0,
                    has_dispatched: false,
                    accepted: 0,
                    idle_deadline: None,
                },
            );
        }
        drop(corrupt_physical);

        let result = catch_unwind(AssertUnwindSafe(|| {
            OriginCell::close_h2(&cell, generation, CloseReason::ProtocolClosed);
        }));
        assert!(
            result.is_err(),
            "corrupt H2 state did not reach its consistency assertion"
        );

        // This second lease exists only if unwinding dropped the connection
        // after the cell guard, allowing capacity to return through admission.
        drop(OriginAdmission::lease_for_test(&admission));
    }

    #[test]
    fn close_retains_an_accepted_generation_until_both_endpoints_finish() {
        let cell = cell();
        let (generation, connection, _physical) = install_generation(&cell, 1);
        let mut activation =
            OriginCell::select_h2(&cell).expect("accepting generation was not selected");
        let H2DispatchParts {
            sender: _sender,
            send_endpoint: send,
            receive_endpoint: receive,
        } = activation.take_dispatch_parts();
        let dispatch = ConnectionState::try_commit_dispatch(&connection)
            .expect("open connection rejected dispatch");
        activation.accept(dispatch);

        assert!(OriginCell::close_h2(
            &cell,
            generation,
            CloseReason::ProtocolClosed
        ));
        assert!(cell.state.lock().h2.generations.contains_key(&generation));
        drop(send);
        assert!(cell.state.lock().h2.generations.contains_key(&generation));
        drop(receive);
        assert!(!cell.state.lock().h2.generations.contains_key(&generation));
        assert_eq!(0, connection.snapshot().in_flight);
    }

    #[test]
    fn driver_task_drop_closes_its_exact_generation() {
        let cell = cell();
        let (generation, connection, _physical) = install_generation(&cell, 1);
        let guard = H2DriverGuard::new(H2CloseHandle::new(&cell, generation));

        drop(guard);

        assert_eq!(
            Some(CloseReason::OwnerRuntimeShutdown),
            connection.snapshot().close_reason
        );
        assert_eq!(None, cell.accepting_h2_generation());
    }

    #[test]
    fn local_selection_withdraws_idle_h2_from_reclaim() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test(NonZeroUsize::MIN);
        let connection_cell = bounded_cell(&admission, 1);
        let requesting_cell = bounded_cell(&admission, 2);
        let info = ConnectionInfo::for_test(ConnectionId::new(1), connection_cell.id().partition());
        let (connection, _physical) = ConnectionState::pending_open(info);
        connection
            .open(Some(OriginAdmission::lease_for_test(&admission)))
            .expect("bounded HTTP/2 connection did not open");
        let generation =
            OriginCell::install_h2_for_test(&connection_cell, connection.clone(), 1, None);
        let activation = OriginCell::select_h2(&connection_cell)
            .expect("local accepting generation did not activate");

        let (waiter, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H1Required);
        let action = OriginAdmission::publish_action_without_driving(
            &admission,
            requesting_cell.id().partition(),
            demand,
        );
        assert!(
            action.is_none(),
            "busy local HTTP/2 generation remained eligible for reclaim"
        );

        assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
        drop(activation);
        assert_eq!(None, connection.snapshot().close_reason);
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }

    #[test]
    fn h1_required_demand_preserves_h2_without_an_http1_guarantee() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test_with_h2_reclaim(NonZeroUsize::MIN, false);
        let connection_cell = bounded_cell(&admission, 1);
        let requesting_cell = bounded_cell(&admission, 2);
        let info = ConnectionInfo::for_test(ConnectionId::new(1), connection_cell.id().partition());
        let (connection, _physical) = ConnectionState::pending_open(info);
        connection
            .open(Some(OriginAdmission::lease_for_test(&admission)))
            .expect("bounded HTTP/2 connection did not open");
        let generation =
            OriginCell::install_h2_for_test(&connection_cell, connection.clone(), 1, None);

        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Required);
        assert!(
            requesting_cell.take_ready_event(waiter).is_none(),
            "H1-required demand reclaimed capacity without an HTTP/1 guarantee"
        );
        assert_eq!(None, connection.snapshot().close_reason);
        assert_eq!(Some(generation), connection_cell.accepting_h2_generation());

        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::ProtocolClosed,
        ));
        let super::super::AcquisitionEvent::Establish(permit) = requesting_cell
            .take_ready_event(waiter)
            .expect("ordinary H2 close did not release capacity")
        else {
            panic!("released capacity produced a non-establishment result");
        };
        drop(permit);
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn h1_required_demand_reclaims_idle_h2_capacity() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test(NonZeroUsize::MIN);
        let connection_cell = bounded_cell(&admission, 1);
        let requesting_cell = bounded_cell(&admission, 2);
        let info = ConnectionInfo::for_test(ConnectionId::new(1), connection_cell.id().partition());
        let (connection, _physical) = ConnectionState::pending_open(info);
        connection
            .open(Some(OriginAdmission::lease_for_test(&admission)))
            .expect("bounded HTTP/2 connection did not open");
        OriginCell::install_h2_for_test(&connection_cell, connection.clone(), 1, None);

        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Required);
        let super::super::AcquisitionEvent::Establish(permit) = requesting_cell
            .take_ready_event(waiter)
            .expect("idle HTTP/2 reclaim did not deliver capacity")
        else {
            panic!("idle HTTP/2 reclaim produced a non-capacity result");
        };

        assert_eq!(
            Some(CloseReason::Reclaimed),
            connection.snapshot().close_reason
        );
        assert_eq!(None, connection_cell.accepting_h2_generation());
        drop(permit);
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn h1_required_demand_waits_for_active_h2_before_reclaim() {
        use crate::client::pool::admission::OriginAdmission;
        use std::num::NonZeroUsize;

        let admission = OriginAdmission::for_test(NonZeroUsize::MIN);
        let connection_cell = bounded_cell(&admission, 1);
        let requesting_cell = bounded_cell(&admission, 2);
        let info = ConnectionInfo::for_test(ConnectionId::new(1), connection_cell.id().partition());
        let (connection, _physical) = ConnectionState::pending_open(info);
        connection
            .open(Some(OriginAdmission::lease_for_test(&admission)))
            .expect("bounded HTTP/2 connection did not open");
        let generation =
            OriginCell::install_h2_for_test(&connection_cell, connection.clone(), 1, None);
        let activation = OriginCell::activate_h2(&connection_cell, generation)
            .expect("accepting HTTP/2 generation did not activate");

        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Required);
        assert!(
            requesting_cell.take_ready_event(waiter).is_none(),
            "active HTTP/2 generation was reclaimed"
        );
        assert_eq!(None, connection.snapshot().close_reason);

        drop(activation);
        let super::super::AcquisitionEvent::Establish(permit) = requesting_cell
            .take_ready_event(waiter)
            .expect("idle transition did not resume H1-required demand")
        else {
            panic!("idle HTTP/2 reclaim produced a non-capacity result");
        };
        assert_eq!(
            Some(CloseReason::Reclaimed),
            connection.snapshot().close_reason
        );
        drop(permit);
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn pool_shutdown_closes_an_accepting_generation() {
        let cell = cell();
        let (_generation, connection, _physical) = install_generation(&cell, 1);

        OriginCell::close_all(&cell, CloseReason::PoolDropped);

        assert_eq!(
            Some(CloseReason::PoolDropped),
            connection.snapshot().close_reason
        );
        assert_eq!(None, cell.accepting_h2_generation());
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use crate::client::pool::connection::{CloseReason, ConnectionInfo};
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::EligibilityGroup;
    use aws_smithy_runtime_api::client::connection::ConnectionId;
    use http_1x::uri::Scheme;

    fn cell() -> Arc<OriginCell> {
        Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    fn install_generation(cell: &Arc<OriginCell>) -> (H2GenerationId, Arc<ConnectionState>) {
        let (connection, _physical) = ConnectionState::unbounded(ConnectionInfo::for_test(
            ConnectionId::new(1),
            PartitionId::from_index(1),
        ));
        let generation = OriginCell::install_h2_for_test(cell, connection.clone(), 1, None);
        (generation, connection)
    }

    #[test]
    fn activation_linearizes_against_generation_close() {
        loom::model(|| {
            let cell = cell();
            let (generation, connection) = install_generation(&cell);
            let activating_cell = cell.clone();
            let activation =
                loom::thread::spawn(move || OriginCell::activate_h2(&activating_cell, generation));
            let closing_cell = cell.clone();
            let close = loom::thread::spawn(move || {
                OriginCell::close_h2(&closing_cell, generation, CloseReason::Poisoned)
            });

            drop(activation.join().unwrap());
            assert!(close.join().unwrap());
            assert_eq!(
                Some(CloseReason::Poisoned),
                connection.snapshot().close_reason
            );
            assert!(!cell.state.lock().h2.generations.contains_key(&generation));
        });
    }

    #[test]
    fn concurrent_lease_endpoint_completion_releases_one_dispatch() {
        loom::model(|| {
            let cell = cell();
            let (generation, connection) = install_generation(&cell);
            let mut activation =
                OriginCell::activate_h2(&cell, generation).expect("generation did not activate");
            let H2DispatchParts {
                sender: _sender,
                send_endpoint: send,
                receive_endpoint: receive,
            } = activation.take_dispatch_parts();
            let dispatch = ConnectionState::try_commit_dispatch(&connection)
                .expect("open connection rejected dispatch");
            activation.accept(dispatch);

            let send = loom::thread::spawn(move || drop(send));
            let receive = loom::thread::spawn(move || drop(receive));
            send.join().unwrap();
            receive.join().unwrap();

            assert_eq!(0, connection.snapshot().in_flight);
            let state = cell.state.lock();
            let record = state
                .h2
                .generations
                .get(&generation)
                .expect("accepting generation disappeared");
            assert_eq!(0, record.accepted);
        });
    }

    #[test]
    fn generation_close_waits_for_both_lease_endpoints() {
        loom::model(|| {
            let cell = cell();
            let (connection, _physical) = ConnectionState::unbounded(ConnectionInfo::for_test(
                ConnectionId::new(1),
                PartitionId::from_index(1),
            ));
            let generation = OriginCell::install_h2_for_test(&cell, connection.clone(), 1, None);
            let mut activation =
                OriginCell::activate_h2(&cell, generation).expect("generation did not activate");
            let H2DispatchParts {
                sender: _sender,
                send_endpoint: send,
                receive_endpoint: receive,
            } = activation.take_dispatch_parts();
            let dispatch = ConnectionState::try_commit_dispatch(&connection)
                .expect("open connection rejected dispatch");
            activation.accept(dispatch);

            let first_endpoint = loom::thread::spawn(move || drop(send));
            let closing_cell = cell.clone();
            let close = loom::thread::spawn(move || {
                OriginCell::close_h2(&closing_cell, generation, CloseReason::ProtocolClosed)
            });
            first_endpoint.join().unwrap();
            assert!(close.join().unwrap());

            assert_eq!(1, connection.snapshot().in_flight);
            assert_eq!(
                Some((0, 1)),
                cell.h2_request_counts(generation),
                "draining generation released before its second endpoint"
            );
            drop(receive);
            assert_eq!(0, connection.snapshot().in_flight);
            assert_eq!(
                None,
                cell.h2_request_counts(generation),
                "finished draining generation remained installed"
            );
        });
    }
}

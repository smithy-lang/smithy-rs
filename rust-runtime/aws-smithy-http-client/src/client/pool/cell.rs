/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! State local to one partition and origin.
//!
//! An [`OriginCell`] owns its acquisition order and protocol records. A
//! bounded cell publishes aggregate demand to [`OriginAdmission`] after
//! releasing its lock, and admission returns capacity through the same
//! unlocked boundary.
//!
//! [`AcquisitionQueue`] owns every live acquisition attempt, including the
//! bounded FIFO, delivery crossings, establishment launch, cancellation, and
//! terminal results. Its module documents the complete transition model.
//! [`CellState`] couples that model to HTTP/1 sender residence, HTTP/2
//! flight and generation residence, generation gates, and the cross-cell reuse
//! reservation. A local HTTP/1 return either satisfies one acquisition or
//! becomes idle, never both. An HTTP/2 activation becomes visible only with a
//! prospective generation lease and one recorded gate opportunity.
//!
//! Permits, senders, admission updates, and task wakes are detached from
//! mutable state before their fallback or callback runs. This keeps drop and
//! wake paths outside the cell lock.

pub(super) mod h1;
pub(super) mod h2;
mod waiters;

#[cfg(test)]
use self::h1::H1CloseHandle;
#[cfg(all(test, not(smithy_http_client_loom)))]
use self::h1::H1DriverGuard;
#[cfg(test)]
use self::h1::H1Sender;
use self::h1::{H1Records, H1ReuseReservation, H1Selection, OwnedH1};
#[cfg(test)]
use self::waiters::CellSnapshot;
pub(in crate::client::pool) use self::waiters::WaiterId;
use self::waiters::{AcquisitionQueue, DeliveryReservation, ResultInstallError};
use super::admission::reuse::{H1Availability, H1AvailabilitySnapshot, ReuseId};
use super::admission::{
    AdmissionAction, CapacityLease, DeliveryGuard, DemandSnapshot, H2AdvertisementSnapshot,
    OriginAdmission, ProtocolRequirement,
};
use super::connection::CloseReason;
#[cfg(test)]
use super::connection::ConnectionInfo;
#[cfg(test)]
use super::connection::ConnectionState;
use super::maintenance::PartitionMaintenance;
use super::origin::OriginKey;
use super::partition::{EligibilityGroup, PartitionId};
use crate::sync::{Arc, Mutex};
#[cfg(test)]
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::task::{Context, Poll};
use std::time::SystemTime;

/// Stable identity of an [`OriginCell`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CellId {
    /// Partition that owns this cell's connection work.
    partition: PartitionId,
    /// Canonical origin whose requests share this cell.
    origin: OriginKey,
}

impl CellId {
    pub(super) fn new(partition: PartitionId, origin: OriginKey) -> Self {
        Self { partition, origin }
    }

    pub(crate) fn partition(&self) -> PartitionId {
        self.partition
    }

    pub(crate) fn origin(&self) -> &OriginKey {
        &self.origin
    }
}

/// Stable state shared by requests for one partition and canonical origin.
pub(crate) struct OriginCell {
    /// Complete identity used by partition and admission indexes.
    id: CellId,
    /// Partitions whose connections may satisfy this cell's demand.
    eligibility_group: EligibilityGroup,
    /// Origin-wide authority present only when capacity is bounded.
    admission: Option<Arc<OriginAdmission>>,
    /// Partition-owned idle deadline scheduler.
    maintenance: Option<Arc<PartitionMaintenance>>,
    /// Waiter order and protocol records protected by one cell-local lock.
    state: Mutex<CellState>,
}

/// Mutable state protected by one partition-origin cell lock.
///
/// Waiter outcomes and protocol residence share this lock. HTTP/1 returns,
/// HTTP/2 activation gates, flight participants, and cancellation therefore
/// commit against one acquisition order.
#[derive(Debug, Default)]
struct CellState {
    /// Cell-local acquisition order and delivered results.
    waiters: AcquisitionQueue,
    /// Cell-owned HTTP/1 records and reusable sender order.
    h1: H1Records,
    /// Cell-owned HTTP/2 flight and installed generations.
    h2: h2::H2Records,
    /// One cross-cell reuse reservation and its local fairness debt.
    reuse: H1ReuseReservation,
    /// Last HTTP/1 availability published to bounded-origin admission.
    published_h1_availability: Option<H1Availability>,
    /// Revision used to reject availability reports that cross out of order.
    h1_availability_revision: u64,
    /// Last complete H2 advertisement reported to origin admission.
    published_h2_advertisement: Option<(h2::H2GenerationId, bool)>,
    /// Revision used to reject H2 advertisements that cross out of order.
    h2_advertisement_revision: u64,
}

impl CellState {
    /// Checks the coupled waiter, sender, and reuse-reservation state machines.
    fn assert_consistent(&self) {
        #[cfg(any(debug_assertions, test))]
        if std::thread::panicking() {
            return;
        }
        self.waiters.assert_consistent();
        self.h1.assert_consistent();
        self.h2.assert_consistent();
        #[cfg(any(debug_assertions, test))]
        self.h2.assert_pending_waiters(&self.waiters);
        self.reuse
            .assert_consistent(self.h1.supports_installed_reuse());
    }

    /// Reports the HTTP/1 availability admission needs without exposing cell internals.
    fn h1_availability(&self) -> H1Availability {
        let local_h1_demand = self.waiters.can_accept_h1();
        H1Availability {
            advertised: self.h1.has_returnable(),
            blocked: !self.reuse.is_available()
                || self.reuse.blocks_peer_reuse(local_h1_demand)
                || self.waiters.has_prior_h1_candidate(),
        }
    }

    /// Returns an availability update only when admission's view must change.
    fn take_h1_availability_update(&mut self) -> Option<H1AvailabilitySnapshot> {
        let current = self.h1_availability();
        if self.published_h1_availability == Some(current) {
            return None;
        }
        Some(self.record_h1_availability(current))
    }

    /// Returns the current versioned availability for a reuse transition.
    fn report_h1_availability(&mut self) -> H1AvailabilitySnapshot {
        let current = self.h1_availability();
        if self.published_h1_availability == Some(current) {
            return H1AvailabilitySnapshot::new(self.h1_availability_revision, current);
        }
        self.record_h1_availability(current)
    }

    /// Advances the revision and records changed HTTP/1 availability.
    fn record_h1_availability(&mut self, availability: H1Availability) -> H1AvailabilitySnapshot {
        self.h1_availability_revision = self
            .h1_availability_revision
            .checked_add(1)
            .expect("HTTP/1 availability revision exhausted");
        self.published_h1_availability = Some(availability);
        H1AvailabilitySnapshot::new(self.h1_availability_revision, availability)
    }

    /// Returns an H2 advertisement only when admission's view must change.
    fn take_h2_advertisement_update(&mut self) -> Option<H2AdvertisementSnapshot> {
        let current = self.h2_advertisement();
        if self.published_h2_advertisement == current {
            return None;
        }
        Some(self.record_h2_advertisement(current))
    }

    /// Returns the current versioned H2 advertisement for a reclaim transition.
    fn report_h2_advertisement(&mut self) -> H2AdvertisementSnapshot {
        let current = self.h2_advertisement();
        if self.published_h2_advertisement == current {
            return Self::h2_advertisement_snapshot(self.h2_advertisement_revision, current);
        }
        self.record_h2_advertisement(current)
    }

    /// Returns the exact publishable generation and its idle state.
    fn h2_advertisement(&self) -> Option<(h2::H2GenerationId, bool)> {
        self.h2
            .publishable_generation()
            .map(|generation| (generation, self.h2.is_idle(generation)))
    }

    /// Advances the revision and records changed H2 availability.
    fn record_h2_advertisement(
        &mut self,
        advertisement: Option<(h2::H2GenerationId, bool)>,
    ) -> H2AdvertisementSnapshot {
        self.h2_advertisement_revision = self
            .h2_advertisement_revision
            .checked_add(1)
            .expect("HTTP/2 advertisement revision exhausted");
        self.published_h2_advertisement = advertisement;
        Self::h2_advertisement_snapshot(self.h2_advertisement_revision, advertisement)
    }

    fn h2_advertisement_snapshot(
        revision: u64,
        advertisement: Option<(h2::H2GenerationId, bool)>,
    ) -> H2AdvertisementSnapshot {
        match advertisement {
            Some((generation, true)) => H2AdvertisementSnapshot::idle(revision, generation),
            Some((generation, false)) => H2AdvertisementSnapshot::accepting(revision, generation),
            None => H2AdvertisementSnapshot::unavailable(revision),
        }
    }

    /// Atomically decides whether a peer may reserve this cell's connection.
    fn install_reuse(&mut self, reuse_id: ReuseId) -> ReuseInstall {
        let local_h1_demand = self.waiters.can_accept_h1();
        if self.reuse.blocks_peer_reuse(local_h1_demand)
            || self.waiters.has_prior_h1_candidate()
            || !self.reuse.is_available()
        {
            let report = self.report_h1_availability();
            self.assert_consistent();
            return ReuseInstall::Rejected(report);
        }

        if let Some(owner) = self.h1.take_idle_for_reuse() {
            assert!(
                self.reuse.install_resolving(reuse_id),
                "available HTTP/1 connection could not install its reuse reservation"
            );
            self.assert_consistent();
            return ReuseInstall::Candidate(owner);
        }
        if self.h1.has_returnable() {
            assert!(
                self.reuse.install(reuse_id),
                "available HTTP/1 connection could not install its reuse reservation"
            );
            self.assert_consistent();
            return ReuseInstall::Installed;
        }

        let report = self.report_h1_availability();
        self.assert_consistent();
        ReuseInstall::Rejected(report)
    }

    /// Clears a reuse reservation and returns the cell's complete availability.
    fn cancel_reuse(&mut self, reuse_id: ReuseId) -> H1AvailabilitySnapshot {
        self.reuse.reject(reuse_id);
        let local_h1_demand = self.waiters.can_accept_h1();
        self.reuse.clear_unused_turn(local_h1_demand);
        self.assert_consistent();
        self.report_h1_availability()
    }

    /// Clears a reservation before its provisional sender follows ordinary return.
    fn reject_reuse_candidate(&mut self, reuse_id: ReuseId) {
        self.reuse.reject(reuse_id);
        self.assert_consistent();
    }

    /// Revalidates a reuse operation and sender residence as one cell transition.
    fn commit_reuse(&mut self, reuse_id: ReuseId, owner: &OwnedH1) -> bool {
        let committed = self.reuse.names(reuse_id) && self.h1.commit_return_to_waiter(owner);
        self.assert_consistent();
        committed
    }

    /// Completes or rejects a reservation after its external action resolves.
    fn finish_reuse(&mut self, reuse_id: ReuseId, transferred: bool) -> H1AvailabilitySnapshot {
        if transferred {
            let local_h1_demand = self.waiters.can_accept_h1();
            self.reuse.complete_transfer(reuse_id, local_h1_demand);
        } else {
            self.reuse.reject(reuse_id);
        }
        self.assert_consistent();
        self.report_h1_availability()
    }

    /// Removes active HTTP/2-compatible demand already served by visible H2 state.
    fn publishable_demand_updates(
        &self,
        updates: [Option<DemandSnapshot>; 2],
    ) -> [Option<DemandSnapshot>; 2] {
        let h2_visible = self.h2.has_visible_h2();
        updates.map(|snapshot| {
            snapshot
                .filter(|snapshot| !h2_visible || !snapshot.is_active() || !snapshot.accepts_h2())
        })
    }
}

/// Cell-local result of attempting to install one peer reuse reservation.
enum ReuseInstall {
    /// The reservation will intercept a future sender return.
    Installed,
    /// An idle sender was detached for immediate resolution.
    Candidate(OwnedH1),
    /// The cell could not reserve a connection for the operation.
    Rejected(H1AvailabilitySnapshot),
}

/// A terminal result that may satisfy an acquisition waiter.
#[derive(Debug)]
pub(super) enum AcquisitionResult {
    /// An exclusive HTTP/1 sender selected from an installed cell-owned record.
    H1(H1Selection),
    /// A prospective request lease against one HTTP/2 generation.
    H2(h2::H2Activation),
    /// Establishment failed before producing a dispatchable connection.
    Failed(ConnectorError),
    /// An HTTP/2 generation closed before serving a transferred attempt.
    Reacquire,
}

/// One event observed while driving an acquisition attempt.
#[derive(Debug)]
pub(super) enum AcquisitionEvent {
    /// Capacity is available and one establishment attempt may start.
    Establish(EstablishmentPermit),
    /// A returned sender or establishment produced the terminal result.
    Complete(AcquisitionResult),
}

/// Optional bounded-origin capacity for one establishment attempt.
///
/// Unbounded origins carry no lease. For bounded origins, dropping this value
/// before connection installation returns the permit to admission.
#[derive(Debug)]
pub(super) struct EstablishmentPermit {
    /// Bounded-origin slot transferred into a connection on success.
    lease: Option<CapacityLease>,
}

impl EstablishmentPermit {
    /// Creates a permit for an origin without a connection bound.
    fn unbounded() -> Self {
        Self { lease: None }
    }

    /// Takes ownership of one bounded-origin capacity lease.
    pub(in crate::client::pool) fn bounded(lease: CapacityLease) -> Self {
        Self { lease: Some(lease) }
    }

    /// Transfers bounded capacity into an installed connection, when present.
    pub(super) fn into_lease(self) -> Option<CapacityLease> {
        self.lease
    }
}

impl OriginCell {
    /// Creates a cell before its one-time publication in a partition registry.
    pub(super) fn new(
        partition: PartitionId,
        origin: OriginKey,
        eligibility_group: EligibilityGroup,
        admission: Option<Arc<OriginAdmission>>,
        maintenance: Option<Arc<PartitionMaintenance>>,
    ) -> Self {
        Self {
            id: CellId::new(partition, origin),
            eligibility_group,
            admission,
            maintenance,
            state: Mutex::new(CellState::default()),
        }
    }

    /// Returns the next idle deadline from immutable pool policy.
    pub(in crate::client::pool) fn idle_deadline(&self) -> Option<SystemTime> {
        self.maintenance
            .as_ref()
            .and_then(|maintenance| maintenance.idle_deadline())
    }

    /// Publishes a newly idle deadline to partition maintenance.
    fn notify_maintenance(&self, deadline: Option<SystemTime>) {
        if let Some(maintenance) = &self.maintenance {
            maintenance.notify_deadline(deadline);
        }
    }

    pub(crate) fn id(&self) -> &CellId {
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn eligibility_group(&self) -> &EligibilityGroup {
        &self.eligibility_group
    }

    #[cfg(test)]
    pub(crate) fn admission(&self) -> Option<&Arc<OriginAdmission>> {
        self.admission.as_ref()
    }

    /// Closes H1 records and H2 generations whose idle deadline elapsed.
    pub(super) fn expire_idle(cell: &Arc<Self>, now: SystemTime) {
        let (expired_h1, expired_h2) = {
            let state = cell.state.lock();
            (state.h1.expired_idle(now), state.h2.expired(now))
        };
        for id in expired_h1 {
            if Self::close_h1(cell, id, CloseReason::IdleTimeout) {
                tracing::trace!(
                    connection_id = %id,
                    connection_partition = ?cell.id.partition(),
                    origin_scheme = %cell.id.origin().scheme(),
                    origin_host = cell.id.origin().host(),
                    origin_port = ?cell.id.origin().port(),
                    "HTTP/1 idle connection expired"
                );
            }
        }
        if let Some(generation) = expired_h2 {
            if Self::close_h2(cell, generation, CloseReason::IdleTimeout) {
                tracing::trace!(
                    connection_partition = ?cell.id.partition(),
                    origin_scheme = %cell.id.origin().scheme(),
                    origin_host = cell.id.origin().host(),
                    origin_port = ?cell.id.origin().port(),
                    h2_generation = ?generation,
                    "HTTP/2 idle generation expired"
                );
            }
        }
    }

    /// Returns this cell's nearest reusable connection deadline.
    pub(super) fn nearest_idle_deadline(&self) -> Option<SystemTime> {
        let state = self.state.lock();
        [
            state.h1.nearest_idle_deadline(),
            state.h2.nearest_idle_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Logically closes every retained connection for pool shutdown.
    pub(super) fn close_all(cell: &Arc<Self>, reason: CloseReason) {
        let (h1, h2) = {
            let state = cell.state.lock();
            (state.h1.connection_ids(), state.h2.generation_ids())
        };
        for id in h1 {
            Self::close_h1(cell, id, reason);
        }
        for generation in h2 {
            Self::close_h2(cell, generation, reason);
        }
    }

    /// Applies one materialized acquisition delivery after admission unlock.
    ///
    /// Connection-cell H1 revalidation has already completed, so reserving
    /// the requesting cell cannot be followed by another fallible connection-
    /// cell transition.
    ///
    /// # Panics
    ///
    /// Panics if a waiter reserved by this function disappears or enters a
    /// state that cannot accept the committed acquisition event.
    pub(in crate::client::pool) fn receive_delivery(
        cell: &Arc<Self>,
        delivery: DeliveryGuard,
    ) -> Option<AdmissionAction> {
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_waiter(delivery.demand(), &cell.eligibility_group)
        };

        let DeliveryReservation::Reserved { waiter, successor } = reservation else {
            return delivery.reject(None);
        };

        let (event, mut acknowledgement) = delivery.commit(successor);
        let (installation, suppress_successor) = {
            let mut state = cell.state.lock();
            let installation = match event {
                AcquisitionEvent::Establish(permit) => {
                    state.waiters.install_capacity(waiter, permit)
                }
                AcquisitionEvent::Complete(result) => {
                    state.waiters.install_borrowed_h1(waiter, result)
                }
            };
            (installation, state.h2.has_visible_h2())
        };
        if suppress_successor {
            acknowledgement.suppress_h2_successor();
        }

        let accepted = installation.accepted;
        let error = installation.error;
        let waker = installation.waker;
        let next = if accepted {
            drop(installation.returned_events);
            acknowledgement.accept()
        } else {
            acknowledgement.reject(installation.returned_events)
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        if let Some(error) = error {
            match error {
                ResultInstallError::MissingWaiter => {
                    panic!("reserved waiter disappeared before acquisition delivery")
                }
                ResultInstallError::UnexpectedState => {
                    panic!("reserved waiter entered an invalid acquisition-delivery state")
                }
            }
        }
        next
    }

    /// Returns the number of retained acquisition waiters for boundary tests.
    #[cfg(test)]
    pub(super) fn retained_waiters_for_test(&self) -> usize {
        self.state.lock().waiters.snapshot().retained
    }

    /// Registers one acquisition waiter in cell-local arrival order.
    ///
    /// The waiter and its demand snapshot are committed under the cell lock.
    /// A bounded cell publishes the snapshot to origin admission only after
    /// that lock is released. An unbounded cell retains only local order.
    pub(super) fn register_waiter(cell: &Arc<Self>, requirement: ProtocolRequirement) -> WaiterId {
        let (waiter, snapshot, h2_visible) = {
            let mut state = cell.state.lock();
            let (waiter, snapshot) = state.waiters.register_waiter(
                requirement,
                &cell.eligibility_group,
                cell.admission.is_some(),
            );
            (
                waiter,
                snapshot,
                state.h2.has_visible_h2() && requirement.accepts_h2(),
            )
        };

        if let (Some(admission), Some(snapshot), false) = (&cell.admission, snapshot, h2_visible) {
            OriginAdmission::publish_demand(admission, cell.id.partition(), snapshot);
        }
        Self::service_h2_waiters(cell);
        Self::service_peer_h2_waiters(cell);
        waiter
    }

    /// Cancels a waiter and re-examines local idle H1 state atomically.
    ///
    /// A newly compatible successor may be serviceable by an H1 that an older
    /// HTTP/2-only head could not use. The local sender is selected under the
    /// same cell lock as cancellation, so it never needs a same-cell peer
    /// reuse operation. Demand publication, fallback, and waking remain outside the lock.
    pub(super) fn cancel_waiter(cell: &Arc<Self>, waiter: WaiterId) -> bool {
        let transition = {
            let mut state = cell.state.lock();
            state.h2.cancel_flight_participant(waiter);
            state.h2.cancel_peer_activation(waiter);
            state.h2.cancel_pending_waiter(waiter);
            state
                .waiters
                .cancel_waiter(waiter, &cell.eligibility_group)
                .map(|mut cancelled| {
                    let mut local_install = if state.waiters.can_accept_h1() {
                        state.h1.select_idle().map(|owner| {
                            state.reuse.consume_local_turn();
                            state.waiters.install_returned_h1(
                                || AcquisitionResult::H1(H1Selection::new(cell, owner)),
                                &cell.eligibility_group,
                            )
                        })
                    } else {
                        None
                    };
                    cancelled.demand_updates =
                        state.publishable_demand_updates(cancelled.demand_updates);
                    if let Some(install) = &mut local_install {
                        if let Some(waiter) = install.waiter {
                            state.h2.cancel_pending_waiter(waiter);
                        }
                        install.demand_updates = state.publishable_demand_updates(std::mem::take(
                            &mut install.demand_updates,
                        ));
                    }
                    (cancelled, local_install)
                })
        };
        let Some((cancelled, local_install)) = transition else {
            return false;
        };

        if let Some(admission) = &cell.admission {
            for snapshot in cancelled
                .demand_updates
                .into_iter()
                .chain(
                    local_install
                        .as_ref()
                        .into_iter()
                        .flat_map(|install| install.demand_updates.iter().cloned()),
                )
                .flatten()
            {
                OriginAdmission::publish_demand(admission, cell.id.partition(), snapshot);
            }
        }

        // Ready results and any locally rejected event cross the lock boundary
        // before their fallback can re-enter the pool.
        drop(cancelled.returned_events);
        if let Some(install) = local_install {
            drop(install.returned_event);
            if let Some(waker) = install.waker {
                waker.wake();
            }
        }

        let availability = {
            let mut state = cell.state.lock();
            let local_h1_demand = state.waiters.can_accept_h1();
            state.reuse.clear_unused_turn(local_h1_demand);
            state.assert_consistent();
            state.take_h1_availability_update()
        };
        cell.publish_h1_availability(availability);
        Self::service_h2_waiters(cell);
        Self::service_peer_h2_waiters(cell);
        true
    }

    /// Polls the next event for one acquisition waiter.
    ///
    /// # Panics
    ///
    /// Panics if `waiter` is unknown, was cancelled, or is polled again after
    /// its ready result was consumed.
    pub(super) fn poll_waiter(
        &self,
        waiter: WaiterId,
        cx: &mut Context<'_>,
    ) -> Poll<AcquisitionEvent> {
        let mut state = self.state.lock();
        state.waiters.poll_waiter(waiter, cx)
    }

    /// Marks an establishment attempt as started before its first connector poll.
    pub(super) fn start_establishment(&self, waiter: WaiterId) -> bool {
        self.state.lock().waiters.start_establishment(waiter)
    }

    /// Commits one terminal establishment result to its launching waiter.
    ///
    /// A returned H1 may already have served or cancelled the waiter. In that
    /// case the losing result leaves the cell lock and follows its ordinary
    /// sender-return or error-drop fallback.
    ///
    /// # Panics
    ///
    /// Panics if the launching waiter disappeared or left the launching state
    /// before its owned establishment completed.
    pub(super) fn complete_establishment(&self, waiter: WaiterId, result: AcquisitionResult) {
        let served_with_h1 = matches!(&result, AcquisitionResult::H1(_));
        let installation = {
            let mut state = self.state.lock();
            let installation = state.waiters.install_establishment_result(waiter, result);
            let served_with_h1 = served_with_h1
                && !installation.returned_events.iter().any(|event| {
                    matches!(
                        event,
                        Some(AcquisitionEvent::Complete(AcquisitionResult::H1(_)))
                    )
                });
            if installation.accepted {
                if served_with_h1 {
                    state.reuse.consume_local_turn();
                } else {
                    let local_h1_demand = state.waiters.can_accept_h1();
                    state.reuse.clear_unused_turn(local_h1_demand);
                }
                state.assert_consistent();
            }
            installation
        };
        drop(installation.returned_events);
        if let Some(waker) = installation.waker {
            waker.wake();
        }
        if let Some(error) = installation.error {
            match error {
                ResultInstallError::MissingWaiter => {
                    unreachable!("establishment result reported a missing waiter")
                }
                ResultInstallError::UnexpectedState => {
                    panic!("establishment completed for a waiter that was not launching")
                }
            }
        }
        let availability = {
            let mut state = self.state.lock();
            state.assert_consistent();
            state.take_h1_availability_update()
        };
        self.publish_h1_availability(availability);
    }

    #[cfg(test)]
    pub(super) fn take_ready_lease(cell: &Arc<Self>, waiter: WaiterId) -> Option<CapacityLease> {
        let permit = match cell.take_ready_event(waiter)? {
            AcquisitionEvent::Establish(permit) => permit,
            AcquisitionEvent::Complete(_) => {
                panic!("capacity test received a terminal acquisition result")
            }
        };
        assert!(Self::cancel_waiter(cell, waiter));
        permit.into_lease()
    }

    #[cfg(test)]
    fn take_ready_h1(&self, waiter: WaiterId) -> Option<H1Selection> {
        match self.take_ready_event(waiter)? {
            AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => Some(selection),
            AcquisitionEvent::Complete(AcquisitionResult::H2(_)) => {
                panic!("HTTP/1 ownership test received an HTTP/2 activation")
            }
            AcquisitionEvent::Complete(AcquisitionResult::Failed(_)) => {
                panic!("HTTP/1 ownership test received establishment failure")
            }
            AcquisitionEvent::Complete(AcquisitionResult::Reacquire) => {
                panic!("HTTP/1 ownership test received an internal reacquisition")
            }
            AcquisitionEvent::Establish(_) => {
                panic!("HTTP/1 ownership test received establishment capacity")
            }
        }
    }

    #[cfg(test)]
    fn take_ready_event(&self, waiter: WaiterId) -> Option<AcquisitionEvent> {
        let mut state = self.state.lock();
        match state
            .waiters
            .poll_waiter(waiter, &mut Context::from_waker(std::task::Waker::noop()))
        {
            Poll::Ready(event) => Some(event),
            Poll::Pending => None,
        }
    }

    #[cfg(test)]
    pub(super) fn register_waiter_without_publish(
        &self,
        requirement: ProtocolRequirement,
    ) -> (WaiterId, DemandSnapshot) {
        let (waiter, snapshot) =
            self.state
                .lock()
                .waiters
                .register_waiter(requirement, &self.eligibility_group, true);
        (
            waiter,
            snapshot.expect("first unpublished waiter did not create demand"),
        )
    }

    #[cfg(test)]
    fn snapshot(&self) -> CellSnapshot {
        self.state.lock().waiters.snapshot()
    }
}

impl std::fmt::Debug for OriginCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginCell")
            .field("id", &self.id)
            .field("eligibility_group", &self.eligibility_group)
            .field("admission", &self.admission)
            .field("state", &self.state)
            .finish()
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use http_1x::uri::Scheme;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;
    use std::task::{Wake, Waker};

    struct WakeCounter(AtomicUsize);

    impl Wake for WakeCounter {
        fn wake(self: StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn wake_by_ref(self: &StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        bounded_cell_with_limit(1)
    }

    fn bounded_cell_with_limit(limit: usize) -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        let admission = OriginAdmission::for_test(NonZeroUsize::new(limit).unwrap());
        let candidate = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(admission.clone()),
            None,
        ));
        let cell = OriginAdmission::register_cell(&admission, candidate);
        (admission, cell)
    }

    fn saturated_bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>, CapacityLease) {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        (admission, cell, lease)
    }

    fn bounded_peer_cells(
        limit: usize,
        owning_group: EligibilityGroup,
        requesting_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        let admission = OriginAdmission::for_test(NonZeroUsize::new(limit).unwrap());
        let origin = OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap();
        let connection_cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                origin.clone(),
                owning_group,
                Some(admission.clone()),
                None,
            )),
        );
        let requesting_cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(2),
                origin,
                requesting_group,
                Some(admission.clone()),
                None,
            )),
        );
        (admission, connection_cell, requesting_cell)
    }

    fn unbounded_cell() -> Arc<OriginCell> {
        Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    fn connection_info(id: u64) -> Arc<ConnectionInfo> {
        ConnectionInfo::for_test(ConnectionId::new(id), PartitionId::from_index(1))
    }

    fn unbounded_connection(
        id: u64,
    ) -> (
        Arc<ConnectionState>,
        super::super::connection::PhysicalConnectionGuard,
    ) {
        ConnectionState::unbounded(connection_info(id))
    }

    #[test]
    fn h1_availability_revision_advances_only_when_availability_changes() {
        let mut state = CellState::default();
        assert!(state.take_h1_availability_update().is_some());
        assert_eq!(1, state.h1_availability_revision);
        assert!(state.take_h1_availability_update().is_none());
        assert_eq!(1, state.h1_availability_revision);

        let (connection, _physical) = unbounded_connection(1);
        state
            .h1
            .install_idle(connection, H1Sender::test(11), None)
            .unwrap();
        assert!(state.take_h1_availability_update().is_some());
        assert_eq!(2, state.h1_availability_revision);
    }

    #[test]
    fn active_reuse_reservation_blocks_peer_availability() {
        let mut state = CellState::default();
        let (connection, _physical) = unbounded_connection(1);
        let owner = state
            .h1
            .install_selected(connection, H1Sender::test(11))
            .expect("fresh HTTP/1 record was rejected");
        let reuse_id = ReuseId::for_test(1);
        assert!(state.reuse.install(reuse_id));

        assert!(state.h1_availability().blocked);

        assert!(state.reuse.reject(reuse_id));
        assert!(state.h1.close_owned(&owner));
        drop(owner);
    }

    #[test]
    fn idle_h1_selection_returns_to_its_owning_cell() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        OriginCell::install_idle_h1(&cell, connection, H1Sender::test(11));

        let selection = OriginCell::select_h1(&cell).expect("idle sender was not selected");
        assert_eq!(ConnectionId::new(1), selection.connection_id());
        assert_eq!(11, selection.test_sender_id());
        assert!(selection.is_reused());
        assert_eq!((1, 0), cell.h1_counts());

        drop(selection);
        assert_eq!((1, 1), cell.h1_counts());
        assert_eq!(
            11,
            OriginCell::select_h1(&cell)
                .expect("returned sender was not reusable")
                .test_sender_id()
        );
    }

    #[test]
    fn selected_h1_closes_when_its_owning_cell_is_gone() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));

        drop(cell);
        drop(selection);

        assert_eq!(
            Some(CloseReason::PoolDropped),
            connection.snapshot().close_reason
        );
    }

    #[test]
    fn complete_h1_exchange_returns_sender_to_idle() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        assert!(!selection.is_reused());

        selection.into_exchange().offer_for_reuse();
        assert_eq!((1, 1), cell.h1_counts());
    }

    #[test]
    fn incomplete_h1_return_closes_and_releases_capacity() {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));

        drop(selection.into_exchange());

        assert_eq!((0, 0), cell.h1_counts());
        assert_eq!(1, admission.available_capacity_for_test());
        assert_eq!(
            Some(CloseReason::IncompleteH1Exchange),
            connection.snapshot().close_reason
        );
    }

    #[test]
    fn returned_h1_satisfies_an_unbounded_waiter() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        drop(selection);

        let selected = cell
            .take_ready_h1(waiter)
            .expect("returned sender did not satisfy the waiter");
        assert_eq!(11, selected.test_sender_id());
        assert!(selected.is_reused());
        assert_eq!(0, cell.snapshot().retained);
        drop(selected);
        assert_eq!((1, 1), cell.h1_counts());
    }

    #[test]
    fn returned_h1_satisfies_bounded_demand_without_releasing_capacity() {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        assert_eq!(1, admission.ordered_demand_count_for_test());

        drop(selection);

        assert_eq!(0, admission.ordered_demand_count_for_test());
        assert_eq!(0, admission.available_capacity_for_test());
        let selected = cell
            .take_ready_h1(waiter)
            .expect("returned sender did not satisfy bounded demand");
        drop(selected);
        assert_eq!((1, 1), cell.h1_counts());
    }

    #[test]
    fn eligible_peer_borrows_an_idle_h1_without_moving_capacity() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&connection_cell, connection, H1Sender::test(11));

        let waiter =
            OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);
        let borrowed = requesting_cell
            .take_ready_h1(waiter)
            .expect("eligible peer did not receive the idle HTTP/1 sender");

        assert_eq!(11, borrowed.test_sender_id());
        assert_eq!(0, admission.available_capacity_for_test());
        assert_eq!((1, 0), connection_cell.h1_counts());
        drop(borrowed);
        assert_eq!((1, 1), connection_cell.h1_counts());
    }

    #[test]
    fn ineligible_peer_reclaims_h1_capacity_without_moving_the_sender() {
        let (admission, connection_cell, requesting_cell) = bounded_peer_cells(
            1,
            EligibilityGroup::Partition(PartitionId::from_index(1)),
            EligibilityGroup::Partition(PartitionId::from_index(2)),
        );
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&connection_cell, connection.clone(), H1Sender::test(11));

        let waiter =
            OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);
        let replacement = OriginCell::take_ready_lease(&requesting_cell, waiter)
            .expect("ineligible peer did not receive reclaimed capacity");

        assert_eq!(
            Some(CloseReason::Reclaimed),
            connection.snapshot().close_reason
        );
        assert_eq!((0, 0), connection_cell.h1_counts());
        assert_eq!(0, admission.available_capacity_for_test());
        drop(replacement);
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn installed_peer_reuse_intercepts_the_next_active_h1_return() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected =
            OriginCell::install_selected_h1(&connection_cell, connection, H1Sender::test(11));
        let waiter =
            OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);

        drop(selected);

        let borrowed = requesting_cell
            .take_ready_h1(waiter)
            .expect("installed reuse operation did not intercept the active return");
        assert_eq!(11, borrowed.test_sender_id());
        drop(borrowed);
        assert_eq!((1, 1), connection_cell.h1_counts());
    }

    #[test]
    fn cancelling_requesting_cell_releases_connection_reservation() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected =
            OriginCell::install_selected_h1(&connection_cell, connection, H1Sender::test(11));
        let waiter =
            OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);

        assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
        assert!(
            connection_cell.state.lock().reuse.is_available(),
            "requesting cell cancellation did not reconcile the installed connection-owning cell reuse operation"
        );
        drop(selected);

        assert_eq!((1, 1), connection_cell.h1_counts());
        assert_eq!(0, admission.ordered_demand_count_for_test());
    }

    #[test]
    fn cross_cell_borrow_owes_one_usable_turn_to_the_owning_cell() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected =
            OriginCell::install_selected_h1(&connection_cell, connection, H1Sender::test(11));

        let requesting_waiter =
            OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);
        let local_waiter =
            OriginCell::register_waiter(&connection_cell, ProtocolRequirement::H1Compatible);
        drop(selected);

        let borrowed = requesting_cell
            .take_ready_h1(requesting_waiter)
            .expect("older peer demand did not receive the intercepted sender");
        assert!(connection_cell.state.lock().reuse.local_turn_owed());
        drop(borrowed);

        let local = connection_cell
            .take_ready_h1(local_waiter)
            .expect("connection-owning cell local demand did not receive its owed turn");
        assert!(!connection_cell.state.lock().reuse.local_turn_owed());
        drop(local);
        assert_eq!((1, 1), connection_cell.h1_counts());
    }

    #[test]
    fn cancelling_ready_h1_returns_it_after_unlock() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        drop(selection);

        assert!(OriginCell::cancel_waiter(&cell, waiter));
        assert_eq!((1, 1), cell.h1_counts());
        assert_eq!(
            11,
            OriginCell::select_h1(&cell)
                .expect("cancelled result did not return its sender")
                .test_sender_id()
        );
    }

    #[test]
    fn returned_h1_and_establishment_complete_one_acquisition_attempt() {
        let cell = unbounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not start establishment")
        else {
            panic!("unbounded miss completed before establishment started");
        };
        assert!(cell.start_establishment(waiter));
        assert!(permit.into_lease().is_none());

        let (returning_connection, _returning_physical) = unbounded_connection(1);
        let returning =
            OriginCell::install_selected_h1(&cell, returning_connection, H1Sender::test(11));
        let (fresh_connection, _fresh_physical) = unbounded_connection(2);
        let fresh = OriginCell::install_selected_h1(&cell, fresh_connection, H1Sender::test(22));

        drop(returning);
        cell.complete_establishment(waiter, AcquisitionResult::H1(fresh));

        assert_eq!((2, 1), cell.h1_counts());
        let winner = cell
            .take_ready_h1(waiter)
            .expect("acquisition attempt had no terminal H1");
        assert_eq!(11, winner.test_sender_id());
        drop(winner);
        assert_eq!((2, 2), cell.h1_counts());
    }

    #[test]
    fn bounded_attempt_remains_eligible_for_a_returned_h1() {
        let (admission, cell) = bounded_cell_with_limit(2);
        let installed_lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), installed_lease);
        let returning = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));

        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("bounded miss received no capacity")
        else {
            panic!("bounded miss completed before establishment started");
        };
        let attempt_lease = permit
            .into_lease()
            .expect("bounded establishment carried no capacity");
        assert_eq!(0, admission.available_capacity_for_test());

        drop(returning);
        let selected = cell
            .take_ready_h1(waiter)
            .expect("returned H1 did not beat the pending establishment");
        drop(attempt_lease);
        assert_eq!(1, admission.available_capacity_for_test());
        drop(selected);
        assert_eq!((1, 1), cell.h1_counts());
    }

    #[test]
    fn older_launching_waiter_precedes_younger_capacity_waiter_for_returned_h1() {
        let (admission, cell) = bounded_cell_with_limit(2);
        let installed_lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), installed_lease);
        let returning = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));

        let older = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(older)
            .expect("older waiter received no establishment capacity")
        else {
            panic!("older waiter completed before establishment started");
        };
        let attempt_lease = permit
            .into_lease()
            .expect("bounded establishment carried no capacity");

        let younger = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        assert!(cell.take_ready_event(younger).is_none());

        drop(returning);

        let selected = cell
            .take_ready_h1(older)
            .expect("younger capacity waiter overtook the older launching waiter");
        assert_eq!(11, selected.test_sender_id());
        assert!(cell.take_ready_event(younger).is_none());

        assert!(OriginCell::cancel_waiter(&cell, younger));
        drop(attempt_lease);
        drop(selected);
        assert_eq!((1, 1), cell.h1_counts());
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn establishment_failure_is_a_terminal_acquisition_result() {
        let cell = unbounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not start establishment")
        else {
            panic!("unbounded miss completed before establishment started");
        };
        assert!(cell.start_establishment(waiter));
        drop(permit);

        cell.complete_establishment(
            waiter,
            AcquisitionResult::Failed(ConnectorError::io(
                std::io::Error::other("synthetic establishment failure").into(),
            )),
        );

        let AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) = cell
            .take_ready_event(waiter)
            .expect("establishment failure was not delivered")
        else {
            panic!("establishment failure produced the wrong acquisition event");
        };
        assert!(error.is_io());
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn establishment_completion_clears_the_served_waiters_local_turn() {
        let cell = unbounded_cell();
        let reuse_id = ReuseId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.reuse.install_resolving(reuse_id));
            assert!(state.reuse.complete_transfer(reuse_id, true));
        }
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not prepare establishment")
        else {
            panic!("unbounded miss completed before establishment was prepared");
        };
        assert!(cell.start_establishment(waiter));
        drop(permit);
        let (connection, _physical) = unbounded_connection(1);
        let fresh = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));

        cell.complete_establishment(waiter, AcquisitionResult::H1(fresh));

        assert!(!cell.state.lock().reuse.local_turn_owed());
        drop(
            cell.take_ready_h1(waiter)
                .expect("successful establishment result was not retained"),
        );
    }

    #[test]
    fn failed_establishment_preserves_a_turn_for_compatible_successor() {
        let cell = unbounded_cell();
        let reuse_id = ReuseId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.reuse.install_resolving(reuse_id));
            assert!(state.reuse.complete_transfer(reuse_id, true));
        }
        let failed = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(failed)
            .expect("unbounded miss did not prepare establishment")
        else {
            panic!("unbounded miss completed before establishment was prepared");
        };
        assert!(cell.start_establishment(failed));
        drop(permit);
        let successor = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        cell.complete_establishment(
            failed,
            AcquisitionResult::Failed(ConnectorError::io(
                std::io::Error::other("synthetic establishment failure").into(),
            )),
        );

        assert!(cell.state.lock().reuse.local_turn_owed());
        assert!(matches!(
            cell.take_ready_event(failed),
            Some(AcquisitionEvent::Complete(AcquisitionResult::Failed(_)))
        ));
        assert!(OriginCell::cancel_waiter(&cell, successor));
        assert!(!cell.state.lock().reuse.local_turn_owed());
    }

    #[test]
    fn failed_establishment_clears_a_turn_after_local_demand_drains() {
        let cell = unbounded_cell();
        let reuse_id = ReuseId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.reuse.install_resolving(reuse_id));
            assert!(state.reuse.complete_transfer(reuse_id, true));
        }
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not prepare establishment")
        else {
            panic!("unbounded miss completed before establishment was prepared");
        };
        assert!(cell.start_establishment(waiter));
        drop(permit);

        cell.complete_establishment(
            waiter,
            AcquisitionResult::Failed(ConnectorError::io(
                std::io::Error::other("synthetic establishment failure").into(),
            )),
        );

        assert!(!cell.state.lock().reuse.local_turn_owed());
        assert!(matches!(
            cell.take_ready_event(waiter),
            Some(AcquisitionEvent::Complete(AcquisitionResult::Failed(_)))
        ));
    }

    #[test]
    fn completion_after_waiter_cancellation_returns_the_new_h1() {
        let cell = unbounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not start establishment")
        else {
            panic!("unbounded miss completed before establishment started");
        };
        assert!(cell.start_establishment(waiter));
        assert!(OriginCell::cancel_waiter(&cell, waiter));
        drop(permit);

        let (connection, _physical) = unbounded_connection(1);
        let fresh = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        cell.complete_establishment(waiter, AcquisitionResult::H1(fresh));

        assert_eq!((1, 1), cell.h1_counts());
        assert_eq!(
            11,
            OriginCell::select_h1(&cell)
                .expect("cancelled acquisition lost its completed H1")
                .test_sender_id()
        );
    }

    #[test]
    fn returned_h1_prevents_the_first_establishment_poll() {
        let cell = unbounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not prepare establishment")
        else {
            panic!("unbounded miss completed before establishment was prepared");
        };
        let (connection, _physical) = unbounded_connection(1);
        let returning = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));

        drop(returning);
        assert!(!cell.start_establishment(waiter));
        drop(permit);
        let selected = cell
            .take_ready_h1(waiter)
            .expect("returned H1 did not win before establishment started");
        assert_eq!(11, selected.test_sender_id());
    }

    #[test]
    fn h2_required_head_does_not_take_h1() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);

        drop(selection);

        assert_eq!((1, 1), cell.h1_counts());
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(waiter)
            .expect("unbounded miss did not start establishment")
        else {
            panic!("H2-required waiter accepted an HTTP/1 selection");
        };
        assert!(OriginCell::cancel_waiter(&cell, waiter));
        drop(permit);
    }

    #[test]
    fn cancelling_h2_head_refunnels_reclaimed_capacity_to_successor() {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&cell, connection, H1Sender::test(11));
        let h2_head = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        let h1_successor = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(h2_head)
            .expect("HTTP/2 demand did not receive reclaimed local capacity")
        else {
            panic!("HTTP/2 demand received a protocol handle instead of capacity");
        };
        assert!(OriginCell::cancel_waiter(&cell, h2_head));
        drop(permit);

        let successor = OriginCell::take_ready_lease(&cell, h1_successor)
            .expect("reclaimed capacity did not reach the compatible successor");
        assert_eq!((0, 0), cell.h1_counts());
        assert!(cell.state.lock().reuse.is_available());
        drop(successor);
    }

    #[test]
    fn close_marks_selected_record_until_sender_returns() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));
        let close = H1CloseHandle::new(&cell, selection.connection());

        assert!(close.close(CloseReason::Poisoned));
        assert!(OriginCell::select_h1(&cell).is_none());
        assert_eq!((1, 0), cell.h1_counts());

        drop(selection);
        assert_eq!((0, 0), cell.h1_counts());
        assert_eq!(
            Some(CloseReason::Poisoned),
            connection.snapshot().close_reason
        );
    }

    #[test]
    fn confirmed_upgrade_refines_an_earlier_driver_close() {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));
        let driver = H1DriverGuard::new(H1CloseHandle::new(&cell, &connection));
        let return_task = selection.into_exchange();

        driver.protocol_closed();
        return_task.retire_connection(CloseReason::Upgraded);

        assert_eq!(
            Some(CloseReason::Upgraded),
            connection.snapshot().close_reason
        );
        assert_eq!(1, admission.available_capacity_for_test());
        assert_eq!((0, 0), cell.h1_counts());
    }

    #[test]
    fn driver_drop_closes_its_installed_generation() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));
        let driver = H1DriverGuard::new(H1CloseHandle::new(&cell, &connection));

        drop(driver);
        assert_eq!(
            Some(CloseReason::OwnerRuntimeShutdown),
            connection.snapshot().close_reason
        );
        drop(selection);
        assert_eq!((0, 0), cell.h1_counts());
    }

    #[test]
    fn owner_runtime_shutdown_drops_live_driver_guard() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));
        let driver = H1DriverGuard::new(H1CloseHandle::new(&cell, &connection));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (started, observed) = std::sync::mpsc::sync_channel(0);

        runtime.spawn(async move {
            let _driver = driver;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        observed
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("driver task did not start");
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));

        assert_eq!(
            Some(CloseReason::OwnerRuntimeShutdown),
            connection.snapshot().close_reason
        );
        drop(selection);
        assert_eq!((0, 0), cell.h1_counts());
    }

    #[test]
    fn three_waiters_receive_capacity_in_fifo_order() {
        let (_admission, cell) = bounded_cell();
        let first = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let third = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        let first_lease = OriginCell::take_ready_lease(&cell, first).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, second).is_none());
        assert!(OriginCell::take_ready_lease(&cell, third).is_none());

        drop(first_lease);
        let second_lease = OriginCell::take_ready_lease(&cell, second).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, third).is_none());

        drop(second_lease);
        let third_lease = OriginCell::take_ready_lease(&cell, third).unwrap();
        drop(third_lease);
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_middle_and_tail_waiters_repairs_fifo_links() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let first = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        let third = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let fourth = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let demand = cell.snapshot().demand;

        assert!(OriginCell::cancel_waiter(&cell, second));
        assert!(OriginCell::cancel_waiter(&cell, fourth));
        assert_eq!(demand, cell.snapshot().demand);
        assert_eq!(2, cell.snapshot().waiting);
        let fifth = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        assert_eq!(3, cell.snapshot().waiting);

        drop(held);
        let first_lease = OriginCell::take_ready_lease(&cell, first).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, third).is_none());
        assert!(OriginCell::take_ready_lease(&cell, fifth).is_none());
        drop(first_lease);

        let third_lease = OriginCell::take_ready_lease(&cell, third).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, fifth).is_none());
        drop(third_lease);

        drop(OriginCell::take_ready_lease(&cell, fifth).unwrap());
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_head_waiter_preserves_remaining_fifo_order() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let first = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let third = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let fourth = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        assert!(OriginCell::cancel_waiter(&cell, first));
        drop(held);

        let second_lease = OriginCell::take_ready_lease(&cell, second).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, third).is_none());
        assert!(OriginCell::take_ready_lease(&cell, fourth).is_none());
        drop(second_lease);

        let third_lease = OriginCell::take_ready_lease(&cell, third).unwrap();
        assert!(OriginCell::take_ready_lease(&cell, fourth).is_none());
        drop(third_lease);

        drop(OriginCell::take_ready_lease(&cell, fourth).unwrap());
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn cancelling_ready_waiter_refunnels_capacity() {
        let (_admission, cell) = bounded_cell();
        let first = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

        assert!(OriginCell::cancel_waiter(&cell, first));
        let lease = OriginCell::take_ready_lease(&cell, second).unwrap();
        drop(lease);
    }

    #[test]
    fn cancelling_during_delivery_refunnels_capacity_after_unlock() {
        let (admission, cell) = bounded_cell();
        let (waiter, demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let mut delivery =
            OriginAdmission::publish_without_driving(&admission, cell.id().partition(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_waiter(delivery.demand(), &cell.eligibility_group)
        };
        let DeliveryReservation::Reserved {
            waiter: reserved,
            successor,
        } = reservation
        else {
            panic!("current delivery was rejected");
        };
        assert_eq!(waiter, reserved);
        let (event, acknowledgement) = delivery.commit(successor);
        let AcquisitionEvent::Establish(permit) = event else {
            panic!("capacity delivery materialized a non-capacity event");
        };

        assert!(OriginCell::cancel_waiter(&cell, waiter));
        let installation = {
            let mut state = cell.state.lock();
            state.waiters.install_capacity(waiter, permit)
        };
        assert!(installation.returned_events[0].is_none());
        assert!(matches!(
            installation.returned_events[1],
            Some(AcquisitionEvent::Establish(_))
        ));
        assert!(installation.error.is_none());
        assert_eq!(0, cell.snapshot().retained);

        drop(installation.returned_events);
        assert_eq!(1, admission.available_capacity_for_test());
        assert!(acknowledgement.accept().is_none());
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn h1_return_during_capacity_delivery_refunnels_the_permit() {
        let (admission, cell) = bounded_cell_with_limit(3);
        let first_lease = OriginAdmission::lease_for_test(&admission);
        let (first_connection, _first_physical) =
            ConnectionState::bounded(connection_info(1), first_lease);
        let first_return =
            OriginCell::install_selected_h1(&cell, first_connection, H1Sender::test(11));
        let second_lease = OriginAdmission::lease_for_test(&admission);
        let (second_connection, _second_physical) =
            ConnectionState::bounded(connection_info(2), second_lease);
        let second_return =
            OriginCell::install_selected_h1(&cell, second_connection, H1Sender::test(22));
        let (waiter, demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let mut delivery =
            OriginAdmission::publish_without_driving(&admission, cell.id().partition(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_waiter(delivery.demand(), &cell.eligibility_group)
        };
        let DeliveryReservation::Reserved {
            waiter: reserved,
            successor,
        } = reservation
        else {
            panic!("current delivery was rejected");
        };
        assert_eq!(waiter, reserved);

        drop(first_return);
        drop(second_return);
        assert_eq!((2, 1), cell.h1_counts());
        let (event, acknowledgement) = delivery.commit(successor);
        let AcquisitionEvent::Establish(permit) = event else {
            panic!("capacity delivery materialized a non-capacity event");
        };
        let installation = {
            let mut state = cell.state.lock();
            state.waiters.install_capacity(waiter, permit)
        };
        assert!(installation.returned_events[0].is_some());
        drop(installation.returned_events);
        assert!(acknowledgement.accept().is_none());

        assert_eq!(1, admission.available_capacity_for_test());
        let selected = cell
            .take_ready_h1(waiter)
            .expect("returning H1 did not win the crossing delivery");
        assert_eq!(11, selected.test_sender_id());
        drop(selected);
    }

    #[test]
    fn cancellation_during_h1_and_capacity_crossing_returns_both() {
        let (admission, cell) = bounded_cell_with_limit(2);
        let installed_lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), installed_lease);
        let returning = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let (waiter, demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
        let mut delivery =
            OriginAdmission::publish_without_driving(&admission, cell.id().partition(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_waiter(delivery.demand(), &cell.eligibility_group)
        };
        let DeliveryReservation::Reserved {
            waiter: reserved,
            successor,
        } = reservation
        else {
            panic!("current delivery was rejected");
        };
        assert_eq!(waiter, reserved);

        drop(returning);
        assert!(OriginCell::cancel_waiter(&cell, waiter));
        let (event, acknowledgement) = delivery.commit(successor);
        let AcquisitionEvent::Establish(permit) = event else {
            panic!("capacity delivery materialized a non-capacity event");
        };
        let installation = {
            let mut state = cell.state.lock();
            state.waiters.install_capacity(waiter, permit)
        };
        assert!(installation.returned_events.iter().all(Option::is_some));
        drop(installation.returned_events);
        assert!(acknowledgement.accept().is_none());

        assert_eq!(1, admission.available_capacity_for_test());
        assert_eq!(0, cell.snapshot().retained);
        assert_eq!((1, 1), cell.h1_counts());
        assert_eq!(
            11,
            OriginCell::select_h1(&cell)
                .expect("cancelled crossing lost its H1 sender")
                .test_sender_id()
        );
    }

    #[test]
    fn stale_delivery_is_rejected_after_head_cancellation() {
        let (admission, cell) = bounded_cell();
        let (first, first_demand) =
            cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let second = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let stale = OriginAdmission::publish_without_driving(
            &admission,
            cell.id().partition(),
            first_demand,
        )
        .expect("first demand did not reserve capacity");

        assert!(OriginCell::cancel_waiter(&cell, first));
        OriginAdmission::drive(Some(AdmissionAction::Delivery(stale)));

        let lease = OriginCell::take_ready_lease(&cell, second)
            .expect("replacement demand did not receive refunnelled capacity");
        drop(lease);
    }

    #[test]
    fn cancelling_the_only_waiter_retires_admission_demand() {
        let (admission, cell, held) = saturated_bounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        assert_eq!(1, admission.ordered_demand_count_for_test());

        assert!(OriginCell::cancel_waiter(&cell, waiter));
        assert_eq!(0, admission.ordered_demand_count_for_test());
        drop(held);
    }

    #[test]
    fn unknown_waiter_cancellation_is_a_noop() {
        let (_admission, cell) = bounded_cell();
        assert!(!OriginCell::cancel_waiter(&cell, WaiterId(99)));
    }

    #[test]
    fn unbounded_waiter_registers_and_cancels_locally() {
        let cell = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ));

        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        assert_eq!(0, cell.snapshot().waiting);
        assert_eq!(1, cell.snapshot().retained);
        assert!(OriginCell::cancel_waiter(&cell, waiter));
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn delivered_waiter_is_woken_once() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
        let counter = StdArc::new(WakeCounter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut context = Context::from_waker(&waker);
        assert!(cell.poll_waiter(waiter, &mut context).is_pending());

        drop(held);
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
        drop(OriginCell::take_ready_lease(&cell, waiter).expect("woken waiter had no capacity"));
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
    }

    fn install_bounded_h2(
        admission: &Arc<OriginAdmission>,
        cell: &Arc<OriginCell>,
        connection_id: u64,
    ) -> (
        h2::H2GenerationId,
        Arc<ConnectionState>,
        super::super::connection::PhysicalConnectionGuard,
    ) {
        let lease = OriginAdmission::lease_for_test(admission);
        let (connection, physical) = ConnectionState::bounded(
            ConnectionInfo::for_test(ConnectionId::new(connection_id), cell.id().partition()),
            lease,
        );
        let generation =
            OriginCell::install_h2_for_test(cell, connection.clone(), connection_id, None);
        (generation, connection, physical)
    }

    fn take_ready_h2(cell: &OriginCell, waiter: WaiterId) -> h2::H2Activation {
        match cell
            .take_ready_event(waiter)
            .expect("HTTP/2 waiter did not receive an activation")
        {
            AcquisitionEvent::Complete(AcquisitionResult::H2(activation)) => activation,
            AcquisitionEvent::Establish(_) => {
                panic!("HTTP/2 waiter received establishment capacity")
            }
            AcquisitionEvent::Complete(AcquisitionResult::H1(_)) => {
                panic!("HTTP/2 waiter received an HTTP/1 sender")
            }
            AcquisitionEvent::Complete(AcquisitionResult::Reacquire) => {
                panic!("HTTP/2 waiter was returned for reacquisition")
            }
            AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) => {
                panic!("HTTP/2 waiter received an establishment failure: {error}")
            }
        }
    }

    #[test]
    fn eligible_peer_h2_generation_satisfies_bounded_demand() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);

        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);
        let activation = take_ready_h2(&requesting_cell, waiter);

        assert_eq!(generation, activation.generation());
        assert!(Arc::ptr_eq(&connection, activation.connection()));
        assert_eq!(0, admission.available_capacity_for_test());
        assert_eq!(Some(generation), connection_cell.accepting_h2_generation());
        drop(activation);
        assert_eq!(0, requesting_cell.snapshot().retained);
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped,
        ));
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn peer_h2_publication_respects_eligibility_group() {
        let groups = [
            (
                EligibilityGroup::Partition(PartitionId::from_index(1)),
                EligibilityGroup::Partition(PartitionId::from_index(2)),
            ),
            (
                EligibilityGroup::NetworkInterface(Some(Arc::from("eth0"))),
                EligibilityGroup::NetworkInterface(Some(Arc::from("eth1"))),
            ),
        ];
        for (connection_group, requesting_group) in groups {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(1, connection_group, requesting_group);
            let (generation, _connection, _physical) =
                install_bounded_h2(&admission, &connection_cell, 1);
            let waiter =
                OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);

            assert!(requesting_cell.take_ready_event(waiter).is_none());
            assert_eq!(1, admission.ordered_demand_count_for_test());
            assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
            assert!(OriginCell::close_h2(
                &connection_cell,
                generation,
                CloseReason::PoolDropped,
            ));
            assert_eq!(1, admission.available_capacity_for_test());
        }
    }

    #[test]
    fn dropped_publication_guard_retries_the_same_demand() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, _connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let (waiter, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let action = OriginAdmission::publish_action_without_driving(
            &admission,
            requesting_cell.id().partition(),
            demand,
        )
        .expect("peer demand did not prepare publication");

        drop(action);

        drop(take_ready_h2(&requesting_cell, waiter));
        assert_eq!(0, admission.ordered_demand_count_for_test());
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }

    #[test]
    fn requesting_cell_cancellation_closes_an_in_flight_publication_fence() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, _connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let (waiter, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let action = OriginAdmission::publish_action_without_driving(
            &admission,
            requesting_cell.id().partition(),
            demand,
        )
        .expect("peer demand did not prepare publication");

        assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
        OriginAdmission::drive(Some(action));

        assert_eq!(0, requesting_cell.snapshot().retained);
        assert_eq!(0, admission.ordered_demand_count_for_test());
        assert_eq!(0, admission.available_capacity_for_test());
        assert_eq!(Some(generation), connection_cell.accepting_h2_generation());
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }

    #[test]
    fn stale_publication_retries_against_a_replacement_generation() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (first, _first_connection, _first_physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let (waiter, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let action = OriginAdmission::publish_action_without_driving(
            &admission,
            requesting_cell.id().partition(),
            demand,
        )
        .expect("peer demand did not prepare publication");

        assert!(OriginCell::close_h2(
            &connection_cell,
            first,
            CloseReason::ProtocolClosed
        ));
        let (second, _second_connection, _second_physical) =
            install_bounded_h2(&admission, &connection_cell, 2);
        OriginAdmission::drive(Some(action));

        let activation = take_ready_h2(&requesting_cell, waiter);
        assert_eq!(second, activation.generation());
        drop(activation);
        assert!(OriginCell::close_h2(
            &connection_cell,
            second,
            CloseReason::PoolDropped
        ));
    }

    #[test]
    fn stale_peer_route_republishes_demand_to_a_replacement_generation() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (first, _first_connection, _first_physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let (waiter, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        assert!(OriginCell::install_h2_route(
            &requesting_cell,
            h2::H2Route::new(&connection_cell, first),
            &EligibilityGroup::Pool,
            demand.id_for_test(),
        ));

        assert!(OriginCell::close_h2(
            &connection_cell,
            first,
            CloseReason::ProtocolClosed
        ));
        let (second, _second_connection, _second_physical) =
            install_bounded_h2(&admission, &connection_cell, 2);
        OriginCell::service_peer_h2_waiters(&requesting_cell);
        let activation = take_ready_h2(&requesting_cell, waiter);
        assert_eq!(second, activation.generation());
        drop(activation);
        assert!(OriginCell::close_h2(
            &connection_cell,
            second,
            CloseReason::PoolDropped
        ));
    }

    #[test]
    fn direct_selection_removes_a_stale_peer_route() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, _connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);
        drop(take_ready_h2(&requesting_cell, waiter));

        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::ProtocolClosed,
        ));
        assert!(OriginCell::select_h2(&requesting_cell).is_none());
        assert!(
            !requesting_cell.state.lock().h2.has_visible_h2(),
            "direct selection retained a stale peer route"
        );
    }

    #[test]
    fn open_peer_route_allows_concurrent_prospective_activations() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, _connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let waiter = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);
        drop(take_ready_h2(&requesting_cell, waiter));

        let first = OriginCell::select_h2(&requesting_cell)
            .expect("first direct peer activation was not selected");
        let second = OriginCell::select_h2(&requesting_cell)
            .expect("open peer route serialized the second activation");
        assert_eq!(generation, first.generation());
        assert_eq!(generation, second.generation());
        assert_eq!(Some((2, 0)), connection_cell.h2_request_counts(generation));
        drop(first);
        drop(second);
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped
        ));
    }

    #[test]
    fn peer_generation_gate_preserves_prepublication_order() {
        let (admission, connection_cell, requesting_cell) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let (generation, _connection, _physical) =
            install_bounded_h2(&admission, &connection_cell, 1);
        let (first, demand) =
            requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
        let second = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);
        let action = OriginAdmission::publish_action_without_driving(
            &admission,
            requesting_cell.id().partition(),
            demand,
        )
        .expect("peer demand did not prepare publication");
        OriginAdmission::drive(Some(action));
        let third = OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H2Required);

        let first_activation = take_ready_h2(&requesting_cell, first);
        assert!(requesting_cell.take_ready_event(second).is_none());
        assert!(requesting_cell.take_ready_event(third).is_none());
        drop(first_activation);

        let second_activation = take_ready_h2(&requesting_cell, second);
        assert!(requesting_cell.take_ready_event(third).is_none());
        drop(second_activation);
        drop(take_ready_h2(&requesting_cell, third));
        assert!(OriginCell::close_h2(
            &connection_cell,
            generation,
            CloseReason::PoolDropped,
        ));
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use http_1x::uri::Scheme;
    use std::num::NonZeroUsize;

    fn bounded_cell() -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        bounded_cell_with_limit(1)
    }

    fn bounded_cell_with_limit(limit: usize) -> (Arc<OriginAdmission>, Arc<OriginCell>) {
        let admission = OriginAdmission::for_test(NonZeroUsize::new(limit).unwrap());
        let candidate = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            Some(admission.clone()),
            None,
        ));
        let cell = OriginAdmission::register_cell(&admission, candidate);
        (admission, cell)
    }

    fn bounded_peer_cells(
        owning_group: EligibilityGroup,
        requesting_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        bounded_peer_cells_with_limit(1, owning_group, requesting_group)
    }

    fn bounded_peer_cells_with_limit(
        limit: usize,
        owning_group: EligibilityGroup,
        requesting_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        let admission = OriginAdmission::for_test(NonZeroUsize::new(limit).unwrap());
        let origin = OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap();
        let connection_cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                origin.clone(),
                owning_group,
                Some(admission.clone()),
                None,
            )),
        );
        let requesting_cell = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(2),
                origin,
                requesting_group,
                Some(admission.clone()),
                None,
            )),
        );
        (admission, connection_cell, requesting_cell)
    }

    fn unbounded_cell() -> Arc<OriginCell> {
        Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    fn connection_info(id: u64) -> Arc<ConnectionInfo> {
        ConnectionInfo::for_test(ConnectionId::new(id), PartitionId::from_index(1))
    }

    #[test]
    fn h1_selection_linearizes_against_close() {
        loom::model(|| {
            let cell = unbounded_cell();
            let (connection, _physical) = ConnectionState::unbounded(connection_info(1));
            OriginCell::install_idle_h1(&cell, connection.clone(), H1Sender::test(11));
            let close = H1CloseHandle::new(&cell, &connection);

            let select_cell = cell.clone();
            let select = loom::thread::spawn(move || OriginCell::select_h1(&select_cell));
            let close = loom::thread::spawn(move || close.close(CloseReason::Poisoned));

            let selected = select.join().unwrap();
            assert!(close.join().unwrap());
            drop(selected);

            assert_eq!((0, 0), cell.h1_counts());
            assert_eq!(
                Some(CloseReason::Poisoned),
                connection.snapshot().close_reason
            );
        });
    }

    #[test]
    fn h1_return_linearizes_against_close() {
        loom::model(|| {
            let cell = unbounded_cell();
            let (connection, _physical) = ConnectionState::unbounded(connection_info(1));
            let selection =
                OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));
            let exchange = selection.into_exchange();
            let close = H1CloseHandle::new(&cell, &connection);

            let returning = loom::thread::spawn(move || exchange.offer_for_reuse());
            let closing = loom::thread::spawn(move || close.close(CloseReason::Poisoned));
            returning.join().unwrap();
            assert!(closing.join().unwrap());

            assert_eq!((0, 0), cell.h1_counts());
            assert_eq!(
                Some(CloseReason::Poisoned),
                connection.snapshot().close_reason
            );
        });
    }

    #[test]
    fn h1_delivery_and_waiter_cancellation_preserve_the_sender() {
        loom::model(|| {
            let cell = unbounded_cell();
            let (connection, _physical) = ConnectionState::unbounded(connection_info(1));
            let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
            let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);

            let returning = loom::thread::spawn(move || drop(selection));
            let cancel_cell = cell.clone();
            let cancelling =
                loom::thread::spawn(move || OriginCell::cancel_waiter(&cancel_cell, waiter));
            returning.join().unwrap();
            assert!(cancelling.join().unwrap());

            assert_eq!((1, 1), cell.h1_counts());
            assert_eq!(0, cell.snapshot().retained);
            assert_eq!(
                11,
                OriginCell::select_h1(&cell)
                    .expect("sender was lost during return/cancel")
                    .test_sender_id()
            );
        });
    }

    #[test]
    fn returned_h1_and_establishment_race_for_one_waiter() {
        loom::model(|| {
            let cell = unbounded_cell();
            let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
            let AcquisitionEvent::Establish(permit) = cell
                .take_ready_event(waiter)
                .expect("unbounded miss did not start establishment")
            else {
                panic!("unbounded miss completed before establishment started");
            };
            assert!(cell.start_establishment(waiter));
            drop(permit);

            let (returning_connection, _returning_physical) =
                ConnectionState::unbounded(connection_info(1));
            let returning =
                OriginCell::install_selected_h1(&cell, returning_connection, H1Sender::test(11));
            let (fresh_connection, _fresh_physical) =
                ConnectionState::unbounded(connection_info(2));
            let fresh =
                OriginCell::install_selected_h1(&cell, fresh_connection, H1Sender::test(22));

            let returning = loom::thread::spawn(move || drop(returning));
            let completion_cell = cell.clone();
            let completing = loom::thread::spawn(move || {
                completion_cell.complete_establishment(waiter, AcquisitionResult::H1(fresh));
            });
            returning.join().unwrap();
            completing.join().unwrap();

            assert_eq!((2, 1), cell.h1_counts());
            let winner = cell
                .take_ready_h1(waiter)
                .expect("both acquisition results were lost");
            assert!(matches!(winner.test_sender_id(), 11 | 22));
            drop(winner);
            assert_eq!((2, 2), cell.h1_counts());
        });
    }

    #[test]
    fn first_establishment_poll_races_returned_h1() {
        loom::model(|| {
            let cell = unbounded_cell();
            let waiter = OriginCell::register_waiter(&cell, ProtocolRequirement::H1Compatible);
            let AcquisitionEvent::Establish(permit) = cell
                .take_ready_event(waiter)
                .expect("unbounded miss did not prepare establishment")
            else {
                panic!("unbounded miss completed before establishment was prepared");
            };

            let (returning_connection, _returning_physical) =
                ConnectionState::unbounded(connection_info(1));
            let returning =
                OriginCell::install_selected_h1(&cell, returning_connection, H1Sender::test(11));
            let return_thread = loom::thread::spawn(move || drop(returning));
            let start_cell = cell.clone();
            let start_thread = loom::thread::spawn(move || start_cell.start_establishment(waiter));

            return_thread.join().unwrap();
            let started = start_thread.join().unwrap();
            drop(permit);

            if started {
                let (fresh_connection, _fresh_physical) =
                    ConnectionState::unbounded(connection_info(2));
                let fresh =
                    OriginCell::install_selected_h1(&cell, fresh_connection, H1Sender::test(22));
                cell.complete_establishment(waiter, AcquisitionResult::H1(fresh));
            }

            let winner = cell
                .take_ready_h1(waiter)
                .expect("returned H1 was lost at the first-poll boundary");
            assert_eq!(11, winner.test_sender_id());
            drop(winner);
            assert_eq!(
                (usize::from(started) + 1, usize::from(started) + 1),
                cell.h1_counts()
            );
        });
    }

    #[test]
    fn h1_return_and_capacity_delivery_complete_one_waiter() {
        loom::model(|| {
            let (admission, cell) = bounded_cell_with_limit(2);
            let installed_lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) =
                ConnectionState::bounded(connection_info(1), installed_lease);
            let returning = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
            let (waiter, demand) =
                cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let mut delivery =
                OriginAdmission::publish_without_driving(&admission, cell.id().partition(), demand)
                    .expect("published demand did not reserve capacity");
            assert!(
                delivery.materialize_for_test(),
                "capacity delivery did not materialize"
            );

            let delivery_cell = cell.clone();
            let delivering = loom::thread::spawn(move || {
                drop(OriginCell::receive_delivery(&delivery_cell, delivery));
            });
            let returning = loom::thread::spawn(move || drop(returning));
            delivering.join().unwrap();
            returning.join().unwrap();

            assert_eq!(1, admission.available_capacity_for_test());
            let selection = cell
                .take_ready_h1(waiter)
                .expect("return and capacity delivery lost the HTTP/1 sender");
            assert_eq!(11, selection.test_sender_id());
            drop(selection);
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn peer_reuse_and_request_cancellation_preserve_the_sender() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let returning =
                OriginCell::install_selected_h1(&connection_cell, connection, H1Sender::test(11));
            let waiter =
                OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);

            let returning = loom::thread::spawn(move || drop(returning));
            let cancel_requesting_cell = requesting_cell.clone();
            let cancelling = loom::thread::spawn(move || {
                OriginCell::cancel_waiter(&cancel_requesting_cell, waiter)
            });
            returning.join().unwrap();
            assert!(cancelling.join().unwrap());

            assert_eq!((1, 1), connection_cell.h1_counts());
            assert_eq!(0, requesting_cell.snapshot().retained);
            assert_eq!(0, admission.ordered_demand_count_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn borrowed_delivery_racing_a_local_return_preserves_both_senders() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells_with_limit(2, EligibilityGroup::Pool, EligibilityGroup::Pool);
            let owning_lease = OriginAdmission::lease_for_test(&admission);
            let (owned_connection, _owning_physical) =
                ConnectionState::bounded(connection_info(1), owning_lease);
            OriginCell::install_idle_h1(&connection_cell, owned_connection, H1Sender::test(11));
            let requesting_lease = OriginAdmission::lease_for_test(&admission);
            let (requesting_connection, _requesting_physical) =
                ConnectionState::bounded(connection_info(2), requesting_lease);
            let local = OriginCell::install_selected_h1(
                &requesting_cell,
                requesting_connection,
                H1Sender::test(22),
            );
            let (waiter, demand) =
                requesting_cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let install = OriginAdmission::publish_action_without_driving(
                &admission,
                requesting_cell.id().partition(),
                demand,
            )
            .expect("peer demand did not prepare a reuse operation");
            let delivery = install
                .drive_once_for_test()
                .expect("reuse operation installation did not prepare a delivery");

            let delivering = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(delivery));
            });
            let returning = loom::thread::spawn(move || drop(local));
            delivering.join().unwrap();
            returning.join().unwrap();

            let selected = requesting_cell
                .take_ready_h1(waiter)
                .expect("delivery/return race did not satisfy the requesting cell waiter");
            drop(selected);
            assert_eq!(
                2,
                connection_cell.h1_counts().0 + requesting_cell.h1_counts().0
            );
            assert_eq!(
                2,
                connection_cell.h1_counts().1 + requesting_cell.h1_counts().1
            );
            assert!(connection_cell.state.lock().reuse.is_available());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn borrow_materialization_races_owning_cell_close_without_stranding_request() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            OriginCell::install_idle_h1(&connection_cell, connection.clone(), H1Sender::test(11));
            let close = H1CloseHandle::new(&connection_cell, &connection);
            let (waiter, demand) =
                requesting_cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let install = OriginAdmission::publish_action_without_driving(
                &admission,
                requesting_cell.id().partition(),
                demand,
            )
            .expect("peer demand did not prepare a reuse operation");
            let delivery = install
                .drive_once_for_test()
                .expect("reuse operation installation did not prepare a delivery");

            let delivering = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(delivery));
            });
            let closing = loom::thread::spawn(move || close.close(CloseReason::Poisoned));
            delivering.join().unwrap();
            closing.join().unwrap();

            let event = requesting_cell
                .take_ready_event(waiter)
                .expect("borrow/close race stranded its requesting cell waiter");
            match event {
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => {
                    drop(selection);
                }
                AcquisitionEvent::Establish(permit) => {
                    assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
                    drop(permit);
                }
                AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) => {
                    panic!("unexpected establishment failure: {error}")
                }
                AcquisitionEvent::Complete(AcquisitionResult::Reacquire) => {
                    panic!("HTTP/1 reuse model requested reacquisition")
                }
                AcquisitionEvent::Complete(AcquisitionResult::H2(_)) => {
                    panic!("HTTP/1 reuse model received an HTTP/2 activation")
                }
            }
            assert_eq!(0, requesting_cell.snapshot().retained);
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn reclaim_and_connection_close_release_exactly_one_capacity_slot() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) = bounded_peer_cells(
                EligibilityGroup::Partition(PartitionId::from_index(1)),
                EligibilityGroup::Partition(PartitionId::from_index(2)),
            );
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let returning = OriginCell::install_selected_h1(
                &connection_cell,
                connection.clone(),
                H1Sender::test(11),
            );
            let close = H1CloseHandle::new(&connection_cell, &connection);
            let waiter =
                OriginCell::register_waiter(&requesting_cell, ProtocolRequirement::H1Compatible);
            let local_waiter =
                OriginCell::register_waiter(&connection_cell, ProtocolRequirement::H1Compatible);

            let returning = loom::thread::spawn(move || drop(returning));
            let closing = loom::thread::spawn(move || close.close(CloseReason::Poisoned));
            returning.join().unwrap();
            let close_won = closing.join().unwrap();

            assert_eq!(
                !close_won,
                connection_cell.state.lock().reuse.local_turn_owed(),
                "only a completed reclaim may earn the connection-owning cell a local fairness turn"
            );

            let replacement = OriginCell::take_ready_lease(&requesting_cell, waiter)
                .expect("close/reclaim race did not deliver released capacity");
            assert_eq!((0, 0), connection_cell.h1_counts());
            assert_eq!(0, admission.available_capacity_for_test());
            assert!(OriginCell::cancel_waiter(&connection_cell, local_waiter));
            drop(replacement);
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn delivery_and_cancellation_race_refunnels_capacity() {
        loom::model(|| {
            let (admission, cell) = bounded_cell();
            let (first, demand) =
                cell.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let mut delivery =
                OriginAdmission::publish_without_driving(&admission, cell.id().partition(), demand)
                    .unwrap();
            assert!(
                delivery.materialize_for_test(),
                "capacity delivery did not materialize"
            );

            let delivery_cell = cell.clone();
            let deliver = loom::thread::spawn(move || {
                drop(OriginCell::receive_delivery(&delivery_cell, delivery));
            });
            let cancel_cell = cell.clone();
            let cancel =
                loom::thread::spawn(move || OriginCell::cancel_waiter(&cancel_cell, first));
            deliver.join().unwrap();
            cancel.join().unwrap();

            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn h2_route_installation_races_requesting_cell_cancellation() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let generation = OriginCell::install_h2_for_test(&connection_cell, connection, 1, None);
            let (waiter, demand) =
                requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
            let route = h2::H2Route::new(&connection_cell, generation);

            let install_cell = requesting_cell.clone();
            let installing = loom::thread::spawn(move || {
                OriginCell::install_h2_route(
                    &install_cell,
                    route,
                    &EligibilityGroup::Pool,
                    demand.id_for_test(),
                )
            });
            let cancel_cell = requesting_cell.clone();
            let cancelling =
                loom::thread::spawn(move || OriginCell::cancel_waiter(&cancel_cell, waiter));
            let _installed = installing.join().unwrap();
            assert!(cancelling.join().unwrap());

            assert_eq!(0, requesting_cell.snapshot().retained);
            assert_eq!(0, admission.ordered_demand_count_for_test());
            assert_eq!(0, admission.available_capacity_for_test());
            assert!(OriginCell::close_h2(
                &connection_cell,
                generation,
                CloseReason::PoolDropped,
            ));
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn h2_publication_acknowledgement_races_generation_close_and_route_service() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(3);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let generation = OriginCell::install_h2_for_test(&connection_cell, connection, 1, None);
            let (waiter, demand) =
                requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
            let action = OriginAdmission::publish_action_without_driving(
                &admission,
                requesting_cell.id().partition(),
                demand,
            )
            .expect("peer demand did not prepare publication");

            let publishing = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(action));
            });
            let closing_cell = connection_cell.clone();
            let closing = loom::thread::spawn(move || {
                OriginCell::close_h2(&closing_cell, generation, CloseReason::Poisoned)
            });
            let servicing_cell = requesting_cell.clone();
            let servicing = loom::thread::spawn(move || {
                OriginCell::service_peer_h2_waiters(&servicing_cell);
            });

            publishing.join().unwrap();
            assert!(closing.join().unwrap());
            servicing.join().unwrap();

            assert!(
                OriginCell::cancel_waiter(&requesting_cell, waiter),
                "publication acknowledgement lost the live demand"
            );
            assert_eq!(0, requesting_cell.snapshot().retained);
            assert_eq!(0, admission.ordered_demand_count_for_test());
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn h2_publication_close_race_preserves_capacity_ownership() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, connection_cell, requesting_cell) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let generation = OriginCell::install_h2_for_test(&connection_cell, connection, 1, None);
            let (waiter, demand) =
                requesting_cell.register_waiter_without_publish(ProtocolRequirement::H2Required);
            let action = OriginAdmission::publish_action_without_driving(
                &admission,
                requesting_cell.id().partition(),
                demand,
            )
            .expect("peer demand did not prepare publication");

            let publishing = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(action));
            });
            let closing_cell = connection_cell.clone();
            let closing = loom::thread::spawn(move || {
                OriginCell::close_h2(&closing_cell, generation, CloseReason::Poisoned)
            });
            publishing.join().unwrap();
            assert!(closing.join().unwrap());

            assert!(OriginCell::cancel_waiter(&requesting_cell, waiter));
            assert_eq!(0, requesting_cell.snapshot().retained);
            assert_eq!(0, admission.ordered_demand_count_for_test());
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }
}

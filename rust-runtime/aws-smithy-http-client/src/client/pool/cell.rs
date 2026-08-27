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
//! A bounded episode enters the capacity FIFO; an unbounded episode starts
//! ready to establish. Capacity is intermediate: the same waiter continues
//! racing establishment against a reusable H1 until one result is consumed:
//!
//! ```text
//! bounded register --> Waiting
//! unbounded register -------------------------------> ReadyToEstablish
//!
//! Waiting --reserve capacity--> Receiving
//! Waiting --reusable H1-----------------------------> Ready
//! Receiving --capacity------------------------------> ReadyToEstablish
//! Receiving --reusable H1--> Receiving(pending H1)
//! Receiving(pending H1) --capacity------------------> Ready + return capacity
//! ReadyToEstablish --reusable H1--------------------> Ready + return capacity
//! ReadyToEstablish --poll---------------------------> Launching
//! Launching --reusable H1 or attempt result---------> Ready
//! Ready --poll--------------------------------------> consumed
//! ```
//!
//! Cancellation has one crossing-only residence:
//!
//! ```text
//! Waiting --------------------------> removed; retire or advance demand
//! Receiving --> CancelledReceiving -> removed when delivery lands; return payload
//! ReadyToEstablish / Ready ---------> removed; return the held event
//! Launching ------------------------> removed; a started pool-owned attempt may finish
//! ```
//!
//! Returned permits and senders leave the lock before their fallback runs.
//! A result from a started attempt whose waiter was removed follows the same
//! unlocked return path.

mod claims;
pub(super) mod h1;
mod waiters;

use self::claims::SourceClaimSlot;
#[cfg(test)]
use self::h1::H1CloseHandle;
#[cfg(all(test, not(smithy_http_client_loom)))]
use self::h1::H1DriverGuard;
use self::h1::{H1Records, H1Selection, H1Sender, OwnedH1, ProvisionalH1};
#[cfg(test)]
use self::waiters::CellSnapshot;
pub(in crate::client::pool) use self::waiters::WaiterId;
use self::waiters::{DeliveryReservation, ResultInstallError, WaiterQueue};
use super::admission::claims::{
    ClaimCandidate, ClaimId, PreparedClaim, SourceAvailability, SourceInstallResult, SourceSnapshot,
};
#[cfg(test)]
use super::admission::DemandSnapshot;
use super::admission::{
    AdmissionAction, CapacityLease, DeliveryGuard, OriginAdmission, ProtocolRequirement,
};
#[cfg(test)]
use super::connection::ConnectionInfo;
use super::connection::{CloseReason, ConnectionState};
use super::maintenance::PartitionMaintenance;
use super::origin::OriginKey;
use super::partition::{EligibilityGroup, PartitionId};
use crate::sync::{Arc, Mutex};
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::task::{Context, Poll};
use std::time::SystemTime;

/// Stable identity of an [`OriginCell`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CellId {
    partition: PartitionId,
    origin: OriginKey,
}

impl CellId {
    /// Creates an identity from its complete partition and origin axes.
    pub(super) fn new(partition: PartitionId, origin: OriginKey) -> Self {
        Self { partition, origin }
    }

    /// Returns the partition axis.
    pub(crate) fn partition(&self) -> PartitionId {
        self.partition
    }

    /// Returns the canonical origin axis.
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
/// Waiter outcomes and HTTP/1 residence share this lock so a returning sender
/// either becomes visible to one waiter or enters idle storage, never both.
#[derive(Debug, Default)]
struct CellState {
    /// Cell-local acquisition order and delivered results.
    waiters: WaiterQueue,
    /// Source-owned HTTP/1 records and reusable sender order.
    h1: H1Records,
    /// One bounded-origin claim endpoint and its local fairness debt.
    claims: SourceClaimSlot,
    /// Last source state published to bounded-origin admission.
    published_source: Option<SourceAvailability>,
    /// Revision used to reject source reports that cross admission out of order.
    source_revision: u64,
}

impl CellState {
    /// Checks the coupled waiter, sender, and source-claim state machines.
    fn assert_consistent(&self) {
        self.waiters.assert_consistent();
        self.h1.assert_consistent();
        self.claims
            .assert_consistent(self.h1.supports_installed_claim());
    }

    /// Reports the source state admission needs without exposing cell internals.
    fn source_availability(&self) -> SourceAvailability {
        let local_h1_demand = self.waiters.can_accept_h1();
        SourceAvailability {
            advertised: self.h1.is_advertisable(),
            blocked: !self.claims.is_available()
                || self.claims.blocks_peer_claim(local_h1_demand)
                || self.waiters.has_prior_h1_candidate(),
        }
    }

    /// Returns a source publication only when admission's view must change.
    fn take_source_update(&mut self) -> Option<SourceSnapshot> {
        let current = self.source_availability();
        if self.published_source == Some(current) {
            return None;
        }
        Some(self.record_source_state(current))
    }

    /// Records the current source state for a claim acknowledgement.
    fn report_source_state(&mut self) -> SourceSnapshot {
        let current = self.source_availability();
        if self.published_source == Some(current) {
            return SourceSnapshot::new(self.source_revision, current);
        }
        self.record_source_state(current)
    }

    /// Advances the source revision and records one changed availability.
    fn record_source_state(&mut self, availability: SourceAvailability) -> SourceSnapshot {
        self.source_revision = self
            .source_revision
            .checked_add(1)
            .expect("HTTP/1 source revision exhausted");
        self.published_source = Some(availability);
        SourceSnapshot::new(self.source_revision, availability)
    }

    /// Atomically decides whether a peer claim can occupy this source.
    fn install_source_claim(&mut self, claim: ClaimId) -> SourceClaimInstall {
        let local_h1_demand = self.waiters.can_accept_h1();
        if self.claims.blocks_peer_claim(local_h1_demand)
            || self.waiters.has_prior_h1_candidate()
            || !self.claims.is_available()
        {
            let report = self.report_source_state();
            self.assert_consistent();
            return SourceClaimInstall::Rejected(report);
        }

        if let Some(owner) = self.h1.take_idle_for_claim() {
            assert!(
                self.claims.install_resolving(claim),
                "available HTTP/1 source claim slot rejected installation"
            );
            self.assert_consistent();
            return SourceClaimInstall::Candidate(owner);
        }
        if self.h1.has_returnable() {
            assert!(
                self.claims.install(claim),
                "available HTTP/1 source claim slot rejected installation"
            );
            self.assert_consistent();
            return SourceClaimInstall::Installed;
        }

        let report = self.report_source_state();
        self.assert_consistent();
        SourceClaimInstall::Rejected(report)
    }

    /// Clears a claim endpoint and returns its complete source report.
    fn cancel_source_claim(&mut self, claim: ClaimId) -> SourceSnapshot {
        self.claims.reject(claim);
        let local_h1_demand = self.waiters.can_accept_h1();
        self.claims.clear_unused_turn(local_h1_demand);
        self.assert_consistent();
        self.report_source_state()
    }

    /// Clears a provisional claim before its sender follows ordinary return.
    fn reject_source_candidate(&mut self, claim: ClaimId) {
        self.claims.reject(claim);
        self.assert_consistent();
    }

    /// Revalidates a claim and sender residence as one cell transition.
    fn commit_source_claim(&mut self, claim: ClaimId, owner: &OwnedH1) -> bool {
        let committed = self.claims.names(claim) && self.h1.commit_return_to_waiter(owner);
        self.assert_consistent();
        committed
    }

    /// Completes or rejects an endpoint after its external action resolves.
    fn finish_source_claim(&mut self, claim: ClaimId, transferred: bool) -> SourceSnapshot {
        if transferred {
            let local_h1_demand = self.waiters.can_accept_h1();
            self.claims.complete_transfer(claim, local_h1_demand);
        } else {
            self.claims.reject(claim);
        }
        self.assert_consistent();
        self.report_source_state()
    }
}

/// Cell-local result of attempting to install one peer claim.
enum SourceClaimInstall {
    /// The endpoint will intercept a future sender return.
    Installed,
    /// An idle sender was detached for immediate resolution.
    Candidate(OwnedH1),
    /// The endpoint could not accept the claim.
    Rejected(SourceSnapshot),
}

/// A terminal result that may satisfy an acquisition waiter.
#[derive(Debug)]
pub(super) enum AcquisitionResult {
    /// An exclusive HTTP/1 sender selected from an installed source record.
    H1(H1Selection),
    /// Establishment failed before producing a dispatchable connection.
    Failed(ConnectorError),
}

/// One event observed while driving an acquisition episode.
#[derive(Debug)]
pub(super) enum AcquisitionEvent {
    /// Capacity is available and one establishment attempt may start.
    Establish(EstablishmentPermit),
    /// A returned sender or establishment reached the episode's terminal result.
    Complete(AcquisitionResult),
}

/// Optional bounded-origin capacity for one establishment attempt.
///
/// Unbounded origins carry no lease. For bounded origins, dropping this value
/// before connection installation returns the permit to admission.
#[derive(Debug)]
pub(super) struct EstablishmentPermit {
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

    /// Assigns an idle deadline from immutable pool policy.
    fn idle_deadline(&self) -> Option<SystemTime> {
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

    /// Returns this cell's stable partition-and-origin identity.
    pub(crate) fn id(&self) -> &CellId {
        &self.id
    }

    /// Returns the set of partitions eligible to use this cell's connections.
    #[cfg(test)]
    pub(crate) fn eligibility_group(&self) -> &EligibilityGroup {
        &self.eligibility_group
    }

    /// Returns this cell's origin-wide admission authority, when bounded.
    #[cfg(test)]
    pub(crate) fn admission(&self) -> Option<&Arc<OriginAdmission>> {
        self.admission.as_ref()
    }

    /// Installs a fresh H1 record selected by its launching request.
    pub(super) fn install_selected_h1(
        cell: &Arc<Self>,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
    ) -> H1Selection {
        let (installed, availability) = {
            let mut state = cell.state.lock();
            let installed = state.h1.install_selected(connection, sender);
            state.assert_consistent();
            (installed, state.take_source_update())
        };
        cell.publish_source_update(availability);
        match installed {
            Ok(owner) => H1Selection::new(cell, owner),
            Err(owner) => {
                owner
                    .connection()
                    .logical_close(CloseReason::ProtocolClosed);
                drop(owner);
                panic!("duplicate HTTP/1 connection identity installed in one cell");
            }
        }
    }

    /// Installs an H1 record whose launching acquisition already completed.
    #[cfg(test)]
    pub(super) fn install_idle_h1(
        cell: &Arc<Self>,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
    ) {
        let deadline = cell.idle_deadline();
        let (installed, availability) = {
            let mut state = cell.state.lock();
            let installed = state.h1.install_idle(connection, sender, deadline);
            state.assert_consistent();
            (installed, state.take_source_update())
        };
        cell.publish_source_update(availability);
        if let Err(owner) = installed {
            owner
                .connection()
                .logical_close(CloseReason::ProtocolClosed);
            drop(owner);
            panic!("duplicate HTTP/1 connection identity installed in one cell");
        }
        cell.notify_maintenance(deadline);
    }

    /// Selects the newest reusable H1 sender without origin-wide coordination.
    pub(super) fn select_h1(cell: &Arc<Self>) -> Option<H1Selection> {
        let (owner, availability) = {
            let mut state = cell.state.lock();
            let owner = state.h1.select_idle();
            if owner.is_some() {
                state.claims.consume_local_turn();
            }
            state.assert_consistent();
            let availability = cell
                .admission
                .as_ref()
                .and_then(|_| state.take_source_update());
            (owner, availability)
        };
        cell.publish_source_update(availability);
        let owner = owner?;
        Some(H1Selection::new(cell, owner))
    }

    /// Installs one admission-selected return claim in this source cell.
    pub(in crate::client::pool) fn install_h1_claim(
        cell: &Arc<Self>,
        origin: Arc<OriginAdmission>,
        claim: PreparedClaim,
    ) -> Option<AdmissionAction> {
        let decision = {
            let mut state = cell.state.lock();
            state.install_source_claim(claim.id)
        };

        let result = match decision {
            SourceClaimInstall::Installed => SourceInstallResult::Installed,
            SourceClaimInstall::Candidate(owner) => {
                let provisional = ProvisionalH1::new(cell, owner);
                SourceInstallResult::Candidate(ClaimCandidate::new(
                    origin.clone(),
                    claim.id,
                    cell.id.clone(),
                    provisional,
                ))
            }
            SourceClaimInstall::Rejected(availability) => {
                SourceInstallResult::Rejected(availability)
            }
        };
        OriginAdmission::finish_h1_claim_install(&origin, claim.id, cell.id.clone(), result)
    }

    /// Clears an installed or resolving claim after target cancellation.
    pub(in crate::client::pool) fn cancel_h1_claim(&self, claim: ClaimId) -> SourceSnapshot {
        self.state.lock().cancel_source_claim(claim)
    }

    /// Returns a rejected provisional sender through ordinary source handling.
    pub(in crate::client::pool) fn reject_h1_claim_candidate(
        cell: &Arc<Self>,
        claim: ClaimId,
        provisional: ProvisionalH1,
    ) -> SourceSnapshot {
        {
            let mut state = cell.state.lock();
            state.reject_source_candidate(claim);
        }
        drop(provisional);
        let mut state = cell.state.lock();
        state.assert_consistent();
        state.report_source_state()
    }

    /// Revalidates a claim and commits its provisional sender for dispatch.
    pub(in crate::client::pool) fn commit_h1_claim(
        cell: &Arc<Self>,
        claim: ClaimId,
        provisional: ProvisionalH1,
    ) -> Result<H1Selection, ProvisionalH1> {
        let (source, owner) = provisional.into_parts();
        let committed = cell.state.lock().commit_source_claim(claim, &owner);
        if committed {
            Ok(H1Selection::new(cell, owner))
        } else {
            Err(ProvisionalH1::from_parts(source, owner))
        }
    }

    /// Closes a claimed sender and records fairness only if close wins.
    pub(in crate::client::pool) fn reclaim_h1_claim(
        cell: &Arc<Self>,
        claim: ClaimId,
        provisional: ProvisionalH1,
    ) -> Result<SourceSnapshot, ProvisionalH1> {
        let (source, owner) = provisional.into_parts();
        if !cell.state.lock().claims.names(claim) {
            return Err(ProvisionalH1::from_parts(source, owner));
        }

        let connection_id = owner.id();
        let close_won = Self::retire_h1_owner(cell, owner, CloseReason::Reclaimed);
        let availability = {
            let mut state = cell.state.lock();
            let availability = state.finish_source_claim(claim, close_won);
            state.assert_consistent();
            availability
        };
        if close_won {
            tracing::trace!(
                connection_id = %connection_id,
                source_partition = ?cell.id.partition(),
                "reclaimed HTTP/1 connection capacity"
            );
        }
        Ok(availability)
    }

    /// Completes a source endpoint after a target accepted a borrowed sender.
    pub(in crate::client::pool) fn complete_h1_claim(
        &self,
        claim: ClaimId,
        transferred: bool,
    ) -> SourceSnapshot {
        self.state.lock().finish_source_claim(claim, transferred)
    }

    /// Publishes this cell's complete source state when the origin is bounded.
    fn publish_source_update(&self, availability: Option<SourceSnapshot>) {
        if let (Some(admission), Some(availability)) = (&self.admission, availability) {
            OriginAdmission::update_h1_source(
                admission,
                self.id.clone(),
                self.eligibility_group.clone(),
                availability,
            );
        }
    }

    /// Moves a selected H1 record into source-owned return arbitration.
    fn begin_h1_return(&self, id: ConnectionId) -> bool {
        let mut state = self.state.lock();
        let returning = state.h1.begin_return(id);
        state.assert_consistent();
        returning
    }

    /// Returns a reusable sender to the oldest compatible waiter or idle set.
    ///
    /// Demand publication, task wakeup, and any rejected-result fallback all
    /// run after the cell lock is released.
    fn return_h1_owner(cell: &Arc<Self>, owner: OwnedH1) {
        let connection_id = owner.id();
        let mut owner = Some(owner);
        let mut installation = None;
        let mut claimed = None;
        let mut rejected_claim = None;
        let idle_deadline = cell.idle_deadline();
        let should_retire = {
            let mut state = cell.state.lock();
            let returnable = state
                .h1
                .accepts_return(owner.as_ref().expect("HTTP/1 owner disappeared"));
            if !returnable {
                if let Some(claim) = state.claims.intercept_return() {
                    let snapshot = state.finish_source_claim(claim, false);
                    rejected_claim = Some((claim, snapshot));
                } else {
                    state.assert_consistent();
                }
                true
            } else if let Some(claim) = state.claims.intercept_return() {
                state.assert_consistent();
                claimed = Some((
                    claim,
                    ProvisionalH1::new(cell, owner.take().expect("HTTP/1 owner disappeared")),
                ));
                false
            } else if state.waiters.can_accept_h1()
                && state
                    .h1
                    .commit_return_to_waiter(owner.as_ref().expect("HTTP/1 owner disappeared"))
            {
                state.claims.consume_local_turn();
                state.assert_consistent();
                let mut returned = owner.take().expect("HTTP/1 owner disappeared");
                returned.mark_reused();
                installation = Some(state.waiters.install_returned_h1(
                    || AcquisitionResult::H1(H1Selection::new(cell, returned)),
                    &cell.eligibility_group,
                ));
                false
            } else {
                let returned = state.h1.return_idle(
                    owner.take().expect("HTTP/1 owner disappeared"),
                    idle_deadline,
                );
                match returned {
                    Ok(()) => {
                        state.assert_consistent();
                        false
                    }
                    Err(returned) => {
                        owner = Some(returned);
                        true
                    }
                }
            }
        };

        if should_retire {
            tracing::trace!(
                connection_id = %connection_id,
                source_partition = ?cell.id.partition(),
                "HTTP/1 return was rejected by its source"
            );
            if let Some((claim, snapshot)) = rejected_claim {
                let admission = cell
                    .admission
                    .as_ref()
                    .expect("an HTTP/1 return claim requires bounded admission");
                OriginAdmission::reject_returned_h1_claim(
                    admission,
                    claim,
                    cell.id.clone(),
                    snapshot,
                );
            }
            Self::retire_h1_owner(
                cell,
                owner.take().expect("retired HTTP/1 owner disappeared"),
                CloseReason::ProtocolClosed,
            );
            return;
        }

        if let Some((claim, provisional)) = claimed {
            tracing::trace!(
                connection_id = %connection_id,
                source_partition = ?cell.id.partition(),
                "HTTP/1 return was intercepted by a cross-cell claim"
            );
            let admission = cell
                .admission
                .as_ref()
                .expect("an HTTP/1 return claim requires bounded admission");
            let candidate =
                ClaimCandidate::new(admission.clone(), claim, cell.id.clone(), provisional);
            let action = OriginAdmission::resolve_h1_claim(admission, claim, candidate);
            OriginAdmission::drive(action);
            return;
        }

        let Some(installation) = installation else {
            tracing::trace!(
                connection_id = %connection_id,
                source_partition = ?cell.id.partition(),
                "HTTP/1 connection returned to idle storage"
            );
            cell.notify_maintenance(idle_deadline);
            if cell.admission.is_some() {
                let availability = cell.state.lock().take_source_update();
                cell.publish_source_update(availability);
            }
            return;
        };
        tracing::trace!(
            connection_id = %connection_id,
            source_partition = ?cell.id.partition(),
            "HTTP/1 return satisfied local demand"
        );
        if let Some(admission) = &cell.admission {
            for snapshot in installation.demand_updates.into_iter().flatten() {
                OriginAdmission::publish_demand(admission, cell.id.clone(), snapshot);
            }
        }
        drop(installation.returned_event);
        if let Some(waker) = installation.waker {
            waker.wake();
        }
        if cell.admission.is_some() {
            let availability = cell.state.lock().take_source_update();
            cell.publish_source_update(availability);
        }
    }

    /// Retires an externally owned H1 sender and removes its source record.
    ///
    /// Returns whether this path won the connection's logical-close race.
    fn retire_h1_owner(cell: &Arc<Self>, owner: OwnedH1, reason: CloseReason) -> bool {
        let should_close = {
            let mut state = cell.state.lock();
            let should_close = state.h1.close_owned(&owner);
            state.assert_consistent();
            should_close
        };

        let id = owner.id();
        let won = should_close && owner.connection().logical_close(reason);
        drop(owner);

        let mut state = cell.state.lock();
        state.h1.finish_close(id);
        state.assert_consistent();
        let availability = state.take_source_update();
        drop(state);
        cell.publish_source_update(availability);
        won
    }

    /// Begins close for an installed H1 generation named without its sender.
    ///
    /// Returns whether this signal won the connection's logical-close race.
    fn close_h1(cell: &Arc<Self>, id: ConnectionId, reason: CloseReason) -> bool {
        let Some((close, availability)) = ({
            let mut state = cell.state.lock();
            let close = state.h1.begin_close(id);
            state.assert_consistent();
            close.map(|close| (close, state.take_source_update()))
        }) else {
            return false;
        };

        cell.publish_source_update(availability);
        let remove_record = close.sender.is_some();
        let won = close.connection.logical_close(reason);
        drop(close.sender);
        if remove_record {
            let mut state = cell.state.lock();
            state.h1.finish_close(id);
            state.assert_consistent();
        }
        won
    }

    /// Closes idle records whose maintenance deadline has elapsed.
    pub(super) fn expire_idle(cell: &Arc<Self>, now: SystemTime) {
        let expired = cell.state.lock().h1.expired_idle(now);
        for id in expired {
            Self::close_h1(cell, id, CloseReason::IdleTimeout);
        }
    }

    /// Returns this cell's nearest reusable H1 deadline.
    pub(super) fn nearest_idle_deadline(&self) -> Option<SystemTime> {
        self.state.lock().h1.nearest_idle_deadline()
    }

    /// Logically closes every retained H1 record for pool shutdown.
    pub(super) fn close_all_h1(cell: &Arc<Self>, reason: CloseReason) {
        let ids = cell.state.lock().h1.connection_ids();
        for id in ids {
            Self::close_h1(cell, id, reason);
        }
    }

    /// Applies one materialized acquisition delivery after admission unlock.
    ///
    /// Source-side H1 revalidation has already completed, so reserving the
    /// target cannot be followed by another fallible source transition.
    pub(in crate::client::pool) fn receive_delivery(
        cell: &Arc<Self>,
        delivery: DeliveryGuard,
    ) -> Option<AdmissionAction> {
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
        };

        let DeliveryReservation::Reserved { waiter, successor } = reservation else {
            return delivery.reject(None);
        };

        let (event, acknowledgement) = delivery.commit(successor);
        let installation = {
            let mut state = cell.state.lock();
            match event {
                AcquisitionEvent::Establish(permit) => {
                    state.waiters.install_capacity(waiter, permit)
                }
                AcquisitionEvent::Complete(result) => {
                    state.waiters.install_borrowed_h1(waiter, result)
                }
            }
        };

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
    #[cfg(all(test, feature = "rt-tokio"))]
    pub(super) fn retained_waiters_for_test(&self) -> usize {
        self.state.lock().waiters.snapshot().retained
    }

    /// Returns H1 record counts for focused ownership tests.
    #[cfg(test)]
    fn h1_counts(&self) -> (usize, usize) {
        self.state.lock().h1.counts()
    }

    /// Returns the sole installed HTTP/1 connection for focused dispatch tests.
    #[cfg(all(test, feature = "rt-tokio"))]
    pub(super) fn only_h1_connection_for_test(&self) -> Arc<ConnectionState> {
        self.state.lock().h1.only_connection_for_test()
    }

    /// Registers one acquisition waiter in cell-local arrival order.
    ///
    /// The waiter and its demand snapshot are committed under the cell lock.
    /// A bounded cell publishes the snapshot to origin admission only after
    /// that lock is released. An unbounded cell retains only local order.
    pub(super) fn register_waiter(&self, requirement: ProtocolRequirement) -> WaiterId {
        let (waiter, snapshot) = {
            let mut state = self.state.lock();
            state.waiters.register_waiter(
                requirement,
                &self.eligibility_group,
                self.admission.is_some(),
            )
        };

        if let (Some(admission), Some(snapshot)) = (&self.admission, snapshot) {
            OriginAdmission::publish_demand(admission, self.id.clone(), snapshot);
        }
        waiter
    }

    /// Cancels a waiter and re-examines local idle H1 state atomically.
    ///
    /// A newly compatible successor may be serviceable by an H1 that an older
    /// HTTP/2-only head could not use. The local sender is selected under the
    /// same cell lock as cancellation, so it never needs a same-cell peer
    /// claim. Demand publication, fallback, and waking remain outside the lock.
    pub(super) fn cancel_waiter(cell: &Arc<Self>, waiter: WaiterId) -> bool {
        let transition = {
            let mut state = cell.state.lock();
            state
                .waiters
                .cancel_waiter(waiter, &cell.eligibility_group)
                .map(|cancelled| {
                    let local_install = if state.waiters.can_accept_h1() {
                        state.h1.select_idle().map(|owner| {
                            state.claims.consume_local_turn();
                            state.waiters.install_returned_h1(
                                || AcquisitionResult::H1(H1Selection::new(cell, owner)),
                                &cell.eligibility_group,
                            )
                        })
                    } else {
                        None
                    };
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
                OriginAdmission::publish_demand(admission, cell.id.clone(), snapshot);
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
            state.claims.clear_unused_turn(local_h1_demand);
            state.assert_consistent();
            state.take_source_update()
        };
        cell.publish_source_update(availability);
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

    /// Claims an attempt immediately before its first owner-runtime poll.
    pub(super) fn start_establishment(&self, waiter: WaiterId) -> bool {
        self.state.lock().waiters.start_establishment(waiter)
    }

    /// Commits one terminal establishment result to its launching waiter.
    ///
    /// A returned H1 may already have served or cancelled the waiter. In that
    /// case the losing result leaves the cell lock and follows its ordinary
    /// sender-return or error-drop fallback.
    pub(super) fn complete_establishment(&self, waiter: WaiterId, result: AcquisitionResult) {
        let served_with_h1 = matches!(&result, AcquisitionResult::H1(_));
        let installation = {
            let mut state = self.state.lock();
            let installation = state.waiters.install_establishment_result(waiter, result);
            if installation.accepted {
                if served_with_h1 {
                    state.claims.consume_local_turn();
                } else {
                    let local_h1_demand = state.waiters.can_accept_h1();
                    state.claims.clear_unused_turn(local_h1_demand);
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
            state.take_source_update()
        };
        self.publish_source_update(availability);
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
            AcquisitionEvent::Complete(AcquisitionResult::Failed(_)) => {
                panic!("HTTP/1 ownership test received establishment failure")
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
        let admission = OriginAdmission::new(NonZeroUsize::new(limit).unwrap());
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
        source_group: EligibilityGroup,
        target_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        let admission = OriginAdmission::new(NonZeroUsize::new(limit).unwrap());
        let origin = OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap();
        let source = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                origin.clone(),
                source_group,
                Some(admission.clone()),
                None,
            )),
        );
        let target = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(2),
                origin,
                target_group,
                Some(admission.clone()),
                None,
            )),
        );
        (admission, source, target)
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
    fn source_revision_advances_only_when_availability_changes() {
        let mut state = CellState::default();
        assert!(state.take_source_update().is_some());
        assert_eq!(1, state.source_revision);
        assert!(state.take_source_update().is_none());
        assert_eq!(1, state.source_revision);

        let (connection, _physical) = unbounded_connection(1);
        state
            .h1
            .install_idle(connection, H1Sender::test(11), None)
            .unwrap();
        assert!(state.take_source_update().is_some());
        assert_eq!(2, state.source_revision);
    }

    #[test]
    fn occupied_claim_blocks_source_advertisement() {
        let mut state = CellState::default();
        let (connection, _physical) = unbounded_connection(1);
        let owner = state
            .h1
            .install_selected(connection, H1Sender::test(11))
            .expect("fresh HTTP/1 record was rejected");
        let claim = ClaimId::for_test(1);
        assert!(state.claims.install(claim));

        assert!(state.source_availability().blocked);

        assert!(state.claims.reject(claim));
        assert!(state.h1.close_owned(&owner));
        drop(owner);
    }

    #[test]
    fn idle_h1_selection_returns_to_its_source_cell() {
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
    fn selected_h1_closes_when_its_source_cell_is_gone() {
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
    fn complete_h1_return_moves_through_returning_before_idle() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        assert!(!selection.is_reused());

        let offer = selection
            .into_exchange()
            .into_offer()
            .expect("open selection did not enter return arbitration");
        assert_eq!((1, 0), cell.h1_counts());
        offer.resolve();
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
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);

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
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let (admission, source, target) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&source, connection, H1Sender::test(11));

        let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);
        let borrowed = target
            .take_ready_h1(waiter)
            .expect("eligible peer did not receive the idle HTTP/1 sender");

        assert_eq!(11, borrowed.test_sender_id());
        assert_eq!(0, admission.available_capacity_for_test());
        assert_eq!((1, 0), source.h1_counts());
        drop(borrowed);
        assert_eq!((1, 1), source.h1_counts());
    }

    #[test]
    fn ineligible_peer_reclaims_h1_capacity_without_moving_the_sender() {
        let (admission, source, target) = bounded_peer_cells(
            1,
            EligibilityGroup::Partition(PartitionId::from_index(1)),
            EligibilityGroup::Partition(PartitionId::from_index(2)),
        );
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&source, connection.clone(), H1Sender::test(11));

        let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);
        let replacement = OriginCell::take_ready_lease(&target, waiter)
            .expect("ineligible peer did not receive reclaimed capacity");

        assert_eq!(
            Some(CloseReason::Reclaimed),
            connection.snapshot().close_reason
        );
        assert_eq!((0, 0), source.h1_counts());
        assert_eq!(0, admission.available_capacity_for_test());
        drop(replacement);
        assert_eq!(1, admission.available_capacity_for_test());
    }

    #[test]
    fn installed_peer_claim_intercepts_the_next_active_h1_return() {
        let (admission, source, target) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected = OriginCell::install_selected_h1(&source, connection, H1Sender::test(11));
        let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);

        drop(selected);

        let borrowed = target
            .take_ready_h1(waiter)
            .expect("installed claim did not intercept the active return");
        assert_eq!(11, borrowed.test_sender_id());
        drop(borrowed);
        assert_eq!((1, 1), source.h1_counts());
    }

    #[test]
    fn cancelling_a_peer_target_releases_its_installed_source_claim() {
        let (admission, source, target) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected = OriginCell::install_selected_h1(&source, connection, H1Sender::test(11));
        let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);

        assert!(OriginCell::cancel_waiter(&target, waiter));
        assert!(
            source.state.lock().claims.is_available(),
            "target cancellation did not reconcile the installed source claim"
        );
        drop(selected);

        assert_eq!((1, 1), source.h1_counts());
        assert_eq!(0, admission.ordered_demand_count_for_test());
    }

    #[test]
    fn cross_cell_borrow_owes_one_usable_turn_to_the_source() {
        let (admission, source, target) =
            bounded_peer_cells(1, EligibilityGroup::Pool, EligibilityGroup::Pool);
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        let selected = OriginCell::install_selected_h1(&source, connection, H1Sender::test(11));

        let target_waiter = target.register_waiter(ProtocolRequirement::H1Compatible);
        let source_waiter = source.register_waiter(ProtocolRequirement::H1Compatible);
        drop(selected);

        let borrowed = target
            .take_ready_h1(target_waiter)
            .expect("older peer demand did not receive the claimed sender");
        assert!(source.state.lock().claims.local_turn_owed());
        drop(borrowed);

        let local = source
            .take_ready_h1(source_waiter)
            .expect("source-local demand did not receive its owed turn");
        assert!(!source.state.lock().claims.local_turn_owed());
        drop(local);
        assert_eq!((1, 1), source.h1_counts());
    }

    #[test]
    fn cancelling_ready_h1_returns_it_after_unlock() {
        let cell = unbounded_cell();
        let (connection, _physical) = unbounded_connection(1);
        let selection = OriginCell::install_selected_h1(&cell, connection, H1Sender::test(11));
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
    fn returned_h1_and_establishment_complete_one_acquisition_episode() {
        let cell = unbounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
            .expect("acquisition episode had no terminal H1");
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

        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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

        let older = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(older)
            .expect("older waiter received no establishment capacity")
        else {
            panic!("older waiter completed before establishment started");
        };
        let attempt_lease = permit
            .into_lease()
            .expect("bounded establishment carried no capacity");

        let younger = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let claim = ClaimId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.claims.install_resolving(claim));
            assert!(state.claims.complete_transfer(claim, true));
        }
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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

        assert!(!cell.state.lock().claims.local_turn_owed());
        drop(
            cell.take_ready_h1(waiter)
                .expect("successful establishment result was not retained"),
        );
    }

    #[test]
    fn failed_establishment_preserves_a_turn_for_compatible_successor() {
        let cell = unbounded_cell();
        let claim = ClaimId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.claims.install_resolving(claim));
            assert!(state.claims.complete_transfer(claim, true));
        }
        let failed = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let AcquisitionEvent::Establish(permit) = cell
            .take_ready_event(failed)
            .expect("unbounded miss did not prepare establishment")
        else {
            panic!("unbounded miss completed before establishment was prepared");
        };
        assert!(cell.start_establishment(failed));
        drop(permit);
        let successor = cell.register_waiter(ProtocolRequirement::H1Compatible);

        cell.complete_establishment(
            failed,
            AcquisitionResult::Failed(ConnectorError::io(
                std::io::Error::other("synthetic establishment failure").into(),
            )),
        );

        assert!(cell.state.lock().claims.local_turn_owed());
        assert!(matches!(
            cell.take_ready_event(failed),
            Some(AcquisitionEvent::Complete(AcquisitionResult::Failed(_)))
        ));
        assert!(OriginCell::cancel_waiter(&cell, successor));
        assert!(!cell.state.lock().claims.local_turn_owed());
    }

    #[test]
    fn failed_establishment_clears_a_turn_after_local_demand_drains() {
        let cell = unbounded_cell();
        let claim = ClaimId::for_test(1);
        {
            let mut state = cell.state.lock();
            assert!(state.claims.install_resolving(claim));
            assert!(state.claims.complete_transfer(claim, true));
        }
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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

        assert!(!cell.state.lock().claims.local_turn_owed());
        assert!(matches!(
            cell.take_ready_event(waiter),
            Some(AcquisitionEvent::Complete(AcquisitionResult::Failed(_)))
        ));
    }

    #[test]
    fn completion_after_waiter_cancellation_returns_the_new_h1() {
        let cell = unbounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let waiter = cell.register_waiter(ProtocolRequirement::H2Required);

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
    fn cancelling_h2_head_serves_compatible_successor_locally() {
        let (admission, cell) = bounded_cell();
        let lease = OriginAdmission::lease_for_test(&admission);
        let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
        OriginCell::install_idle_h1(&cell, connection, H1Sender::test(11));
        let h2_head = cell.register_waiter(ProtocolRequirement::H2Required);
        let h1_successor = cell.register_waiter(ProtocolRequirement::H1Compatible);

        assert!(cell.take_ready_event(h2_head).is_none());
        assert!(OriginCell::cancel_waiter(&cell, h2_head));

        let selected = cell
            .take_ready_h1(h1_successor)
            .expect("local idle H1 did not serve the compatible successor");
        assert_eq!(11, selected.test_sender_id());
        assert!(cell.state.lock().claims.is_available());
        drop(selected);
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
        return_task.retire(CloseReason::Upgraded);

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
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);

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
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H2Required);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let fourth = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let demand = cell.snapshot().demand;

        assert!(OriginCell::cancel_waiter(&cell, second));
        assert!(OriginCell::cancel_waiter(&cell, fourth));
        assert_eq!(demand, cell.snapshot().demand);
        assert_eq!(2, cell.snapshot().waiting);
        let fifth = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let third = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let fourth = cell.register_waiter(ProtocolRequirement::H1Compatible);

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
        let first = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);

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
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
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
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
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
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
                .expect("published demand did not reserve capacity");
        assert!(delivery.materialize_for_test());
        let reservation = {
            let mut state = cell.state.lock();
            state
                .waiters
                .reserve_delivery_target(delivery.demand(), &cell.eligibility_group)
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
        let second = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let stale =
            OriginAdmission::publish_without_driving(&admission, cell.id().clone(), first_demand)
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
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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

        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        assert_eq!(0, cell.snapshot().waiting);
        assert_eq!(1, cell.snapshot().retained);
        assert!(OriginCell::cancel_waiter(&cell, waiter));
        assert_eq!(0, cell.snapshot().retained);
    }

    #[test]
    fn delivered_waiter_is_woken_once() {
        let (_admission, cell, held) = saturated_bounded_cell();
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let counter = StdArc::new(WakeCounter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut context = Context::from_waker(&waker);
        assert!(cell.poll_waiter(waiter, &mut context).is_pending());

        drop(held);
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
        drop(OriginCell::take_ready_lease(&cell, waiter).expect("woken waiter had no capacity"));
        assert_eq!(1, counter.0.load(Ordering::Relaxed));
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
        let admission = OriginAdmission::new(NonZeroUsize::new(limit).unwrap());
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
        source_group: EligibilityGroup,
        target_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        bounded_peer_cells_with_limit(1, source_group, target_group)
    }

    fn bounded_peer_cells_with_limit(
        limit: usize,
        source_group: EligibilityGroup,
        target_group: EligibilityGroup,
    ) -> (Arc<OriginAdmission>, Arc<OriginCell>, Arc<OriginCell>) {
        let admission = OriginAdmission::new(NonZeroUsize::new(limit).unwrap());
        let origin = OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap();
        let source = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(1),
                origin.clone(),
                source_group,
                Some(admission.clone()),
                None,
            )),
        );
        let target = OriginAdmission::register_cell(
            &admission,
            Arc::new(OriginCell::new(
                PartitionId::from_index(2),
                origin,
                target_group,
                Some(admission.clone()),
                None,
            )),
        );
        (admission, source, target)
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
            let offer = selection
                .into_exchange()
                .into_offer()
                .expect("selection did not enter return arbitration");
            let close = H1CloseHandle::new(&cell, &connection);

            let returning = loom::thread::spawn(move || offer.resolve());
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
            let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);

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
            let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
            let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
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
                OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
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
    fn peer_claim_return_and_target_cancellation_preserve_the_sender() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, source, target) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let returning =
                OriginCell::install_selected_h1(&source, connection, H1Sender::test(11));
            let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);

            let returning = loom::thread::spawn(move || drop(returning));
            let cancel_target = target.clone();
            let cancelling =
                loom::thread::spawn(move || OriginCell::cancel_waiter(&cancel_target, waiter));
            returning.join().unwrap();
            assert!(cancelling.join().unwrap());

            assert_eq!((1, 1), source.h1_counts());
            assert_eq!(0, target.snapshot().retained);
            assert_eq!(0, admission.ordered_demand_count_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn borrowed_delivery_racing_a_local_return_preserves_both_senders() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, source, target) =
                bounded_peer_cells_with_limit(2, EligibilityGroup::Pool, EligibilityGroup::Pool);
            let source_lease = OriginAdmission::lease_for_test(&admission);
            let (source_connection, _source_physical) =
                ConnectionState::bounded(connection_info(1), source_lease);
            OriginCell::install_idle_h1(&source, source_connection, H1Sender::test(11));
            let target_lease = OriginAdmission::lease_for_test(&admission);
            let (target_connection, _target_physical) =
                ConnectionState::bounded(connection_info(2), target_lease);
            let local =
                OriginCell::install_selected_h1(&target, target_connection, H1Sender::test(22));
            let (waiter, demand) =
                target.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let install = OriginAdmission::publish_action_without_driving(
                &admission,
                target.id().clone(),
                demand,
            )
            .expect("peer demand did not prepare a claim");
            let delivery = install
                .drive_once_for_test()
                .expect("claim installation did not prepare a delivery");

            let delivering = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(delivery));
            });
            let returning = loom::thread::spawn(move || drop(local));
            delivering.join().unwrap();
            returning.join().unwrap();

            let selected = target
                .take_ready_h1(waiter)
                .expect("delivery/return race did not satisfy the target waiter");
            drop(selected);
            assert_eq!(2, source.h1_counts().0 + target.h1_counts().0);
            assert_eq!(2, source.h1_counts().1 + target.h1_counts().1);
            assert!(source.state.lock().claims.is_available());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn borrow_materialization_races_source_close_without_stranding_target() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, source, target) =
                bounded_peer_cells(EligibilityGroup::Pool, EligibilityGroup::Pool);
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            OriginCell::install_idle_h1(&source, connection.clone(), H1Sender::test(11));
            let close = H1CloseHandle::new(&source, &connection);
            let (waiter, demand) =
                target.register_waiter_without_publish(ProtocolRequirement::H1Compatible);
            let install = OriginAdmission::publish_action_without_driving(
                &admission,
                target.id().clone(),
                demand,
            )
            .expect("peer demand did not prepare a claim");
            let delivery = install
                .drive_once_for_test()
                .expect("claim installation did not prepare a delivery");

            let delivering = loom::thread::spawn(move || {
                OriginAdmission::drive(Some(delivery));
            });
            let closing = loom::thread::spawn(move || close.close(CloseReason::Poisoned));
            delivering.join().unwrap();
            closing.join().unwrap();

            let event = target
                .take_ready_event(waiter)
                .expect("borrow/close race stranded its target waiter");
            match event {
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => {
                    drop(selection);
                }
                AcquisitionEvent::Establish(permit) => {
                    assert!(OriginCell::cancel_waiter(&target, waiter));
                    drop(permit);
                }
                AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) => {
                    panic!("unexpected establishment failure: {error}")
                }
            }
            assert_eq!(0, target.snapshot().retained);
            assert_eq!(1, admission.available_capacity_for_test());
            admission.clear_modeled_cells_for_test();
        });
    }

    #[test]
    fn reclaim_and_source_close_release_exactly_one_capacity_slot() {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(|| {
            let (admission, source, target) = bounded_peer_cells(
                EligibilityGroup::Partition(PartitionId::from_index(1)),
                EligibilityGroup::Partition(PartitionId::from_index(2)),
            );
            let lease = OriginAdmission::lease_for_test(&admission);
            let (connection, _physical) = ConnectionState::bounded(connection_info(1), lease);
            let returning =
                OriginCell::install_selected_h1(&source, connection.clone(), H1Sender::test(11));
            let close = H1CloseHandle::new(&source, &connection);
            let waiter = target.register_waiter(ProtocolRequirement::H1Compatible);
            let source_waiter = source.register_waiter(ProtocolRequirement::H1Compatible);

            let returning = loom::thread::spawn(move || drop(returning));
            let closing = loom::thread::spawn(move || close.close(CloseReason::Poisoned));
            returning.join().unwrap();
            let close_won = closing.join().unwrap();

            assert_eq!(
                !close_won,
                source.state.lock().claims.local_turn_owed(),
                "only a completed reclaim may earn the source a local fairness turn"
            );

            let replacement = OriginCell::take_ready_lease(&target, waiter)
                .expect("close/reclaim race did not deliver released capacity");
            assert_eq!((0, 0), source.h1_counts());
            assert_eq!(0, admission.available_capacity_for_test());
            assert!(OriginCell::cancel_waiter(&source, source_waiter));
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
                OriginAdmission::publish_without_driving(&admission, cell.id().clone(), demand)
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
}

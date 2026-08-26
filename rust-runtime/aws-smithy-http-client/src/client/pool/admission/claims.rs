/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/1 source scheduling and bounded return-claim ownership.
//!
//! Admission owns source advertisements, target selection, and the claim
//! record. A source cell owns one endpoint slot serialized with sender return.
//! Values crossing between those lock domains retain a typed fallback: an
//! install rejects its claim, a candidate returns its sender, and a borrowed
//! delivery returns the sender before restoring target demand.

use super::{
    AdmissionAction, DeliveryId, DemandId, DemandSchedule, OriginAdmission, ProtocolRequirement,
    TargetAckResult,
};
use crate::client::pool::cell::h1::{H1Selection, ProvisionalH1};
use crate::client::pool::cell::{AcquisitionEvent, CellId, OriginCell};
use crate::client::pool::partition::EligibilityGroup;
use crate::sync::Arc;
use std::collections::{HashMap, VecDeque};
use std::fmt;

/// Identity of one source-to-target HTTP/1 return claim.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::client::pool) struct ClaimId(u64);

impl ClaimId {
    #[cfg(test)]
    pub(in crate::client::pool) const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Terminal use allowed for a sender intercepted by one claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) enum ClaimMode {
    /// Dispatch the target request through the source connection.
    Borrow,
    /// Close the source connection and release its capacity.
    Reclaim,
}

/// Origin-locked HTTP/1 source advertisements and claim ownership.
#[derive(Debug, Default)]
pub(super) struct H1Coordination {
    /// Latest state for every cell that has advertised an HTTP/1 source.
    sources: HashMap<CellId, SourceRecord>,
    /// Amortized FIFO view across every advertised source.
    origin_sources: VecDeque<SourceTicket>,
    /// Amortized FIFO views restricted by reuse eligibility.
    group_sources: HashMap<EligibilityGroup, VecDeque<SourceTicket>>,
    /// Nonterminal claims indexed by their never-reused identity.
    claims: HashMap<ClaimId, ClaimRecord>,
    /// Target demand episodes that already own a claim.
    claimed_targets: HashMap<CellId, ClaimId>,
    /// Installed claims whose target became stale.
    cancellations: VecDeque<ClaimId>,
    /// Next source publication generation.
    next_source_epoch: u64,
    /// Next claim identity.
    next_claim: u64,
}

impl H1Coordination {
    /// Publishes the source's complete current availability.
    pub(super) fn update_source(
        &mut self,
        source: CellId,
        group: EligibilityGroup,
        advertised: bool,
        blocked: bool,
    ) {
        let epoch = self.take_source_epoch();
        let record = self.sources.entry(source.clone()).or_insert(SourceRecord {
            group: group.clone(),
            epoch,
            advertised,
            claim: None,
            blocked,
        });
        record.group = group;
        record.epoch = epoch;
        record.advertised = advertised;
        record.blocked = blocked;
        self.publish_source_ticket(&source);
    }

    /// Withdraws a source without disturbing a claim that already owns it.
    pub(super) fn withdraw(&mut self, source: &CellId) {
        let epoch = self.take_source_epoch();
        if let Some(record) = self.sources.get_mut(source) {
            record.epoch = epoch;
            record.advertised = false;
        }
    }

    /// Marks a target claim stale after demand publication or delivery.
    pub(super) fn reconcile_target(&mut self, target: &CellId, schedule: &DemandSchedule) {
        let Some(claim) = self.claimed_targets.get(target).copied() else {
            return;
        };
        let Some(record) = self.claims.get_mut(&claim) else {
            self.claimed_targets.remove(target);
            return;
        };
        if schedule.is_current_queued(&record.target, record.demand) {
            return;
        }
        record.cancelled = true;
        if record.phase == ClaimPhase::Installed {
            record.phase = ClaimPhase::Cancelling;
            self.cancellations.push_back(claim);
        }
    }

    /// Selects a source for the oldest origin-capacity demand.
    ///
    /// HTTP/1 claims in this implementation are demand-driven. The
    /// origin-capacity head is therefore no younger than any compatible-group
    /// head: when a source exists in the target's group the two heads name the
    /// same demand and borrowing wins, otherwise reclaim serves the older
    /// origin head. The explicit two-head merge becomes necessary for
    /// source-driven returns and HTTP/2 publication, neither of which is
    /// scheduled by this coordinator.
    pub(super) fn prepare_claim(&mut self, schedule: &DemandSchedule) -> Option<PreparedClaim> {
        let target = schedule.queued_head()?;
        if self.claimed_targets.contains_key(&target.target) {
            return None;
        }

        let (source, mode) = if target.requirement == ProtocolRequirement::H1Compatible {
            match self.take_group_source(&target.eligibility_group) {
                Some(source) => (source, ClaimMode::Borrow),
                None => (self.take_origin_source()?, ClaimMode::Reclaim),
            }
        } else {
            (self.take_origin_source()?, ClaimMode::Reclaim)
        };

        let id = self.take_claim_id();
        self.sources
            .get_mut(&source)
            .expect("selected HTTP/1 source disappeared")
            .claim = Some(id);
        self.claimed_targets.insert(target.target.clone(), id);
        self.claims.insert(
            id,
            ClaimRecord {
                source: source.clone(),
                target: target.target.clone(),
                demand: target.demand,
                mode,
                phase: ClaimPhase::Installing,
                cancelled: false,
            },
        );
        Some(PreparedClaim { id, source, mode })
    }

    /// Extracts an installed-claim cancellation, discarding stale queue items.
    pub(super) fn prepare_cancellation(&mut self) -> Option<PreparedCancellation> {
        while let Some(id) = self.cancellations.pop_front() {
            let Some(record) = self.claims.get(&id) else {
                continue;
            };
            if record.phase != ClaimPhase::Cancelling {
                continue;
            }
            return Some(PreparedCancellation {
                id,
                source: record.source.clone(),
            });
        }
        None
    }

    /// Applies an install acknowledgement and returns the retained claim.
    fn finish_install(&mut self, id: ClaimId, resolved: bool) -> Option<ClaimRecord> {
        let record = self.claims.get_mut(&id)?;
        if record.phase != ClaimPhase::Installing {
            return None;
        }
        record.phase = if resolved {
            ClaimPhase::Resolving
        } else if record.cancelled {
            ClaimPhase::Cancelling
        } else {
            ClaimPhase::Installed
        };
        Some(record.clone())
    }

    /// Moves an installed claim to provisional-candidate resolution.
    fn begin_resolution(&mut self, id: ClaimId) -> Option<ClaimRecord> {
        let record = self.claims.get_mut(&id)?;
        if !matches!(
            record.phase,
            ClaimPhase::Installed | ClaimPhase::Resolving | ClaimPhase::Cancelling
        ) {
            return None;
        }
        record.phase = ClaimPhase::Resolving;
        Some(record.clone())
    }

    /// Removes one claim and republishes its source when still available.
    fn finish_claim(
        &mut self,
        id: ClaimId,
        source_state: Option<SourceAvailability>,
    ) -> Option<ClaimRecord> {
        let record = self.claims.remove(&id)?;
        if self.claimed_targets.get(&record.target) == Some(&id) {
            self.claimed_targets.remove(&record.target);
        }
        let epoch = source_state.map(|_| self.take_source_epoch());
        if let Some(source) = self.sources.get_mut(&record.source) {
            if source.claim == Some(id) {
                source.claim = None;
            }
            if let Some(state) = source_state {
                source.advertised = state.advertised;
                source.blocked = state.blocked;
                source.epoch = epoch.expect("source update lost its publication generation");
            }
        }
        self.publish_source_ticket(&record.source);
        Some(record)
    }

    /// Returns the oldest valid source in one eligibility group.
    fn take_group_source(&mut self, group: &EligibilityGroup) -> Option<CellId> {
        loop {
            let ticket = self.group_sources.get_mut(group)?.pop_front()?;
            if self.source_ticket_is_available(&ticket, Some(group)) {
                return Some(ticket.source);
            }
        }
    }

    /// Returns the oldest valid source across the origin.
    fn take_origin_source(&mut self) -> Option<CellId> {
        loop {
            let ticket = self.origin_sources.pop_front()?;
            if self.source_ticket_is_available(&ticket, None) {
                return Some(ticket.source);
            }
        }
    }

    /// Revalidates a lazily removed source ticket.
    fn source_ticket_is_available(
        &self,
        ticket: &SourceTicket,
        group: Option<&EligibilityGroup>,
    ) -> bool {
        self.sources.get(&ticket.source).is_some_and(|record| {
            record.advertised
                && !record.blocked
                && record.claim.is_none()
                && record.epoch == ticket.epoch
                && group.is_none_or(|group| &record.group == group)
        })
    }

    /// Adds the source's current generation to both scheduling views.
    fn publish_source_ticket(&mut self, source: &CellId) {
        let Some(record) = self.sources.get(source) else {
            return;
        };
        if !record.advertised || record.blocked || record.claim.is_some() {
            return;
        }
        let ticket = SourceTicket {
            source: source.clone(),
            epoch: record.epoch,
        };
        self.origin_sources.push_back(ticket.clone());
        self.group_sources
            .entry(record.group.clone())
            .or_default()
            .push_back(ticket);
    }

    /// Allocates a source publication generation.
    fn take_source_epoch(&mut self) -> u64 {
        let value = self.next_source_epoch;
        self.next_source_epoch = value
            .checked_add(1)
            .expect("HTTP/1 source publication generation exhausted");
        value
    }

    /// Allocates a claim identity.
    fn take_claim_id(&mut self) -> ClaimId {
        let value = self.next_claim;
        self.next_claim = value
            .checked_add(1)
            .expect("HTTP/1 claim identity exhausted");
        ClaimId(value)
    }
}

/// Complete source state reported after a source-cell transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct SourceAvailability {
    /// Whether at least one reusable or active HTTP/1 record remains.
    pub(in crate::client::pool) advertised: bool,
    /// Whether a usable owed local turn temporarily excludes peer claims.
    pub(in crate::client::pool) blocked: bool,
}

/// Source result after claim installation crosses from admission.
pub(in crate::client::pool) enum SourceInstallResult {
    /// A future reusable return will satisfy the claim.
    Installed,
    /// An idle sender was extracted immediately.
    Candidate(ClaimCandidate),
    /// The source could not retain the claim.
    Rejected(SourceAvailability),
}

/// One lazily invalidated entry in a source scheduling view.
#[derive(Clone, Debug)]
struct SourceTicket {
    source: CellId,
    epoch: u64,
}

/// Admission's current view of one HTTP/1 source cell.
#[derive(Debug)]
struct SourceRecord {
    group: EligibilityGroup,
    epoch: u64,
    advertised: bool,
    claim: Option<ClaimId>,
    blocked: bool,
}

/// Admission-owned state for one nonterminal return claim.
#[derive(Clone, Debug)]
struct ClaimRecord {
    source: CellId,
    target: CellId,
    demand: DemandId,
    mode: ClaimMode,
    phase: ClaimPhase,
    cancelled: bool,
}

/// Origin-side progress of one return claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaimPhase {
    Installing,
    Installed,
    Resolving,
    Cancelling,
}

/// Claim install work extracted from the admission lock.
#[derive(Debug)]
pub(in crate::client::pool) struct PreparedClaim {
    pub(in crate::client::pool) id: ClaimId,
    pub(in crate::client::pool) source: CellId,
    pub(in crate::client::pool) mode: ClaimMode,
}

/// Source cancellation work extracted from admission.
pub(super) struct PreparedCancellation {
    id: ClaimId,
    source: CellId,
}

/// One unlocked HTTP/1 coordination step.
pub(in crate::client::pool) enum H1Action {
    Install(ClaimInstall),
    Cancel(ClaimCancel),
    Borrow(BorrowDelivery),
    Reclaim(ReclaimAction),
    CompleteSource(SourceCompletion),
}

impl H1Action {
    pub(super) fn install(origin: Arc<OriginAdmission>, claim: PreparedClaim) -> Self {
        Self::Install(ClaimInstall {
            origin,
            claim: Some(claim),
        })
    }

    pub(super) fn cancel(origin: Arc<OriginAdmission>, cancellation: PreparedCancellation) -> Self {
        Self::Cancel(ClaimCancel {
            origin,
            cancellation: Some(cancellation),
        })
    }

    pub(super) fn drive_once(self) -> Option<AdmissionAction> {
        match self {
            Self::Install(action) => action.drive_once(),
            Self::Cancel(action) => action.drive_once(),
            Self::Borrow(action) => action.deliver_once(),
            Self::Reclaim(action) => action.drive_once(),
            Self::CompleteSource(action) => action.drive_once(),
        }
    }
}

/// Claim installation crossing from admission to a source cell.
pub(in crate::client::pool) struct ClaimInstall {
    origin: Arc<OriginAdmission>,
    claim: Option<PreparedClaim>,
}

impl ClaimInstall {
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let claim = self
            .claim
            .take()
            .expect("HTTP/1 claim install consumed more than once");
        let Some(source) = self.origin.target(&claim.source) else {
            return OriginAdmission::reject_h1_claim(&self.origin, claim.id, None);
        };
        OriginCell::install_h1_claim(&source, self.origin.clone(), claim)
    }
}

impl Drop for ClaimInstall {
    fn drop(&mut self) {
        if let Some(claim) = self.claim.take() {
            let next = OriginAdmission::reject_h1_claim(&self.origin, claim.id, None);
            OriginAdmission::drive(next);
        }
    }
}

/// Installed claim cancellation crossing to its source endpoint.
pub(in crate::client::pool) struct ClaimCancel {
    origin: Arc<OriginAdmission>,
    cancellation: Option<PreparedCancellation>,
}

impl ClaimCancel {
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let cancellation = self
            .cancellation
            .take()
            .expect("HTTP/1 claim cancellation consumed more than once");
        let availability = self
            .origin
            .target(&cancellation.source)
            .map(|source| source.cancel_h1_claim(cancellation.id));
        OriginAdmission::finish_h1_claim(&self.origin, cancellation.id, availability)
    }
}

impl Drop for ClaimCancel {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            let availability = self
                .origin
                .target(&cancellation.source)
                .map(|source| source.cancel_h1_claim(cancellation.id));
            let next =
                OriginAdmission::finish_h1_claim(&self.origin, cancellation.id, availability);
            OriginAdmission::drive(next);
        }
    }
}

/// Provisional sender owned by one resolving claim.
pub(in crate::client::pool) struct ClaimCandidate {
    origin: Arc<OriginAdmission>,
    claim: ClaimId,
    source: CellId,
    provisional: Option<ProvisionalH1>,
}

impl ClaimCandidate {
    pub(in crate::client::pool) fn new(
        origin: Arc<OriginAdmission>,
        claim: ClaimId,
        source: CellId,
        provisional: ProvisionalH1,
    ) -> Self {
        Self {
            origin,
            claim,
            source,
            provisional: Some(provisional),
        }
    }

    /// Revalidates the source endpoint and turns the sender into a selection.
    pub(in crate::client::pool) fn commit(mut self) -> Option<H1Selection> {
        let source = self.origin.target(&self.source)?;
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 claim candidate consumed more than once");
        match OriginCell::commit_h1_claim(&source, self.claim, provisional) {
            Ok(selection) => Some(selection),
            Err(provisional) => {
                self.provisional = Some(provisional);
                None
            }
        }
    }

    /// Reclaims the source connection and earns any usable local turn.
    fn reclaim(mut self) -> Option<SourceAvailability> {
        let source = self.origin.target(&self.source)?;
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 claim candidate consumed more than once");
        match OriginCell::reclaim_h1_claim(&source, self.claim, provisional) {
            Ok(availability) => Some(availability),
            Err(provisional) => {
                self.provisional = Some(provisional);
                None
            }
        }
    }
}

impl fmt::Debug for ClaimCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClaimCandidate")
            .field("claim", &self.claim)
            .field("source", &self.source)
            .field("provisional", &self.provisional)
            .finish_non_exhaustive()
    }
}

impl Drop for ClaimCandidate {
    fn drop(&mut self) {
        let Some(provisional) = self.provisional.take() else {
            return;
        };
        let availability = self
            .origin
            .target(&self.source)
            .map(|source| OriginCell::reject_h1_claim_candidate(&source, self.claim, provisional));
        let next = OriginAdmission::finish_h1_claim(&self.origin, self.claim, availability);
        OriginAdmission::drive(next);
    }
}

/// Borrowed sender crossing from admission to its target cell.
pub(in crate::client::pool) struct BorrowDelivery {
    origin: Arc<OriginAdmission>,
    claim: ClaimId,
    delivery: DeliveryId,
    source: CellId,
    target: CellId,
    demand: DemandId,
    successor: Option<super::DemandSnapshot>,
    candidate: Option<ClaimCandidate>,
    active: bool,
}

impl BorrowDelivery {
    pub(in crate::client::pool) fn demand(&self) -> DemandId {
        self.demand
    }

    fn deliver_once(self) -> Option<AdmissionAction> {
        let target = self.origin.target(&self.target);
        match target {
            Some(target) => OriginCell::receive_borrowed_h1(&target, self),
            None => {
                self.reject(None);
                None
            }
        }
    }

    /// Returns the sender and retires a target revision rejected by its cell.
    pub(in crate::client::pool) fn reject(mut self, successor: Option<super::DemandSnapshot>) {
        self.active = false;
        drop(self.candidate.take());
        let next = OriginAdmission::finish_borrow_delivery(
            &self.origin,
            self.claim,
            self.delivery,
            &self.target,
            TargetAckResult::Rejected { successor },
            None,
            None,
        );
        OriginAdmission::drive(next);
    }

    /// Extracts the candidate after the target reserves its waiter.
    pub(in crate::client::pool) fn commit(
        mut self,
        successor: Option<super::DemandSnapshot>,
    ) -> (ClaimCandidate, BorrowDeliveryAck) {
        let candidate = self
            .candidate
            .take()
            .expect("borrowed HTTP/1 delivery consumed more than once");
        self.active = false;
        (
            candidate,
            BorrowDeliveryAck {
                origin: self.origin.clone(),
                claim: self.claim,
                delivery: self.delivery,
                source: self.source.clone(),
                target: self.target.clone(),
                successor: successor.or_else(|| self.successor.take()),
                active: true,
            },
        )
    }
}

impl fmt::Debug for BorrowDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BorrowDelivery")
            .field("claim", &self.claim)
            .field("delivery", &self.delivery)
            .field("source", &self.source)
            .field("target", &self.target)
            .field("demand", &self.demand)
            .finish_non_exhaustive()
    }
}

impl Drop for BorrowDelivery {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // Return the sender and clear its source slot before the target fence
        // becomes schedulable again.
        drop(self.candidate.take());
        let next = OriginAdmission::finish_borrow_delivery(
            &self.origin,
            self.claim,
            self.delivery,
            &self.target,
            TargetAckResult::RetrySameResidence,
            None,
            None,
        );
        OriginAdmission::drive(next);
    }
}

/// Target-owned completion of a committed borrowed-sender delivery.
pub(in crate::client::pool) struct BorrowDeliveryAck {
    origin: Arc<OriginAdmission>,
    claim: ClaimId,
    delivery: DeliveryId,
    source: CellId,
    target: CellId,
    successor: Option<super::DemandSnapshot>,
    active: bool,
}

impl BorrowDeliveryAck {
    /// Completes an accepted borrow and then records source fairness.
    pub(in crate::client::pool) fn accept(mut self) {
        self.active = false;
        let next = OriginAdmission::finish_borrow_delivery(
            &self.origin,
            self.claim,
            self.delivery,
            &self.target,
            TargetAckResult::Accepted {
                successor: self.successor.take(),
            },
            Some(self.source.clone()),
            None,
        );
        OriginAdmission::drive(next);
        tracing::trace!(
            source_partition = ?self.source.partition(),
            target_partition = ?self.target.partition(),
            "borrowed HTTP/1 connection across pool cells"
        );
    }

    /// Refunnels a sender rejected after source revalidation.
    pub(in crate::client::pool) fn reject_after_commit(
        mut self,
        returned_events: [Option<AcquisitionEvent>; 2],
    ) {
        self.active = false;
        let availability = self
            .origin
            .target(&self.source)
            .map(|source| source.cancel_h1_claim(self.claim));
        drop(returned_events);
        let next = OriginAdmission::finish_borrow_delivery(
            &self.origin,
            self.claim,
            self.delivery,
            &self.target,
            TargetAckResult::Rejected {
                successor: self.successor.take(),
            },
            None,
            availability,
        );
        OriginAdmission::drive(next);
    }
}

impl Drop for BorrowDeliveryAck {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let availability = self
            .origin
            .target(&self.source)
            .map(|source| source.cancel_h1_claim(self.claim));
        let next = OriginAdmission::finish_borrow_delivery(
            &self.origin,
            self.claim,
            self.delivery,
            &self.target,
            TargetAckResult::Rejected {
                successor: self.successor.take(),
            },
            None,
            availability,
        );
        OriginAdmission::drive(next);
    }
}

/// Reclaim decision carrying the claimed provisional sender.
pub(in crate::client::pool) struct ReclaimAction {
    origin: Arc<OriginAdmission>,
    claim: ClaimId,
    candidate: Option<ClaimCandidate>,
}

impl ReclaimAction {
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let candidate = self
            .candidate
            .take()
            .expect("HTTP/1 reclaim action consumed more than once");
        let availability = candidate.reclaim();
        OriginAdmission::finish_h1_claim(&self.origin, self.claim, availability)
    }
}

/// Source endpoint completion after an irreversible borrowed transfer.
pub(in crate::client::pool) struct SourceCompletion {
    origin: Arc<OriginAdmission>,
    claim: ClaimId,
    source: CellId,
    transferred: bool,
    active: bool,
}

impl SourceCompletion {
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let availability = self
            .origin
            .target(&self.source)
            .map(|source| source.complete_h1_claim(self.claim, self.transferred));
        let action = OriginAdmission::finish_h1_claim(&self.origin, self.claim, availability);
        self.active = false;
        action
    }
}

impl Drop for SourceCompletion {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let availability = self
            .origin
            .target(&self.source)
            .map(|source| source.complete_h1_claim(self.claim, self.transferred));
        let next = OriginAdmission::finish_h1_claim(&self.origin, self.claim, availability);
        OriginAdmission::drive(next);
    }
}

impl OriginAdmission {
    /// Publishes a source-cell availability change and drives bounded progress.
    pub(in crate::client::pool) fn update_h1_source(
        origin: &Arc<Self>,
        source: CellId,
        group: EligibilityGroup,
        availability: SourceAvailability,
    ) {
        let action = {
            let mut state = origin.state.lock();
            state
                .h1
                .update_source(source, group, availability.advertised, availability.blocked);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Removes a cell from source scheduling after its last reusable H1 closes.
    pub(in crate::client::pool) fn withdraw_h1_source(origin: &Arc<Self>, source: &CellId) {
        let action = {
            let mut state = origin.state.lock();
            state.h1.withdraw(source);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Applies a source-cell claim-install result.
    pub(in crate::client::pool) fn finish_h1_claim_install(
        origin: &Arc<Self>,
        id: ClaimId,
        result: SourceInstallResult,
    ) -> Option<AdmissionAction> {
        match result {
            SourceInstallResult::Rejected(availability) => {
                Self::reject_h1_claim(origin, id, Some(availability))
            }
            SourceInstallResult::Installed => {
                let action = {
                    let mut state = origin.state.lock();
                    let record = state.h1.finish_install(id, false);
                    if record.as_ref().is_some_and(|record| record.cancelled) {
                        state.h1.cancellations.push_back(id);
                    }
                    Self::prepare_action(origin, &mut state)
                };
                action
            }
            SourceInstallResult::Candidate(candidate) => {
                {
                    let mut state = origin.state.lock();
                    let record = state.h1.finish_install(id, true);
                    if record.is_none() {
                        drop(state);
                        drop(candidate);
                        return None;
                    }
                }
                Self::resolve_h1_claim(origin, id, candidate)
            }
        }
    }

    /// Resolves a provisional sender through borrow or reclaim policy.
    pub(in crate::client::pool) fn resolve_h1_claim(
        origin: &Arc<Self>,
        id: ClaimId,
        candidate: ClaimCandidate,
    ) -> Option<AdmissionAction> {
        let action = {
            let mut state = origin.state.lock();
            let Some(record) = state.h1.begin_resolution(id) else {
                drop(state);
                drop(candidate);
                return None;
            };
            if record.cancelled
                || !state
                    .demand_schedule
                    .is_current_queued(&record.target, record.demand)
            {
                drop(state);
                drop(candidate);
                return None;
            }

            match record.mode {
                ClaimMode::Borrow => {
                    let delivery = state.take_delivery_id();
                    let Some(scheduled) = state.demand_schedule.reserve_claim_target(
                        &record.target,
                        record.demand,
                        delivery,
                    ) else {
                        drop(state);
                        drop(candidate);
                        return None;
                    };
                    Some(AdmissionAction::H1(H1Action::Borrow(BorrowDelivery {
                        origin: origin.clone(),
                        claim: id,
                        delivery,
                        source: record.source,
                        target: scheduled.target,
                        demand: scheduled.demand,
                        successor: None,
                        candidate: Some(candidate),
                        active: true,
                    })))
                }
                ClaimMode::Reclaim => Some(AdmissionAction::H1(H1Action::Reclaim(ReclaimAction {
                    origin: origin.clone(),
                    claim: id,
                    candidate: Some(candidate),
                }))),
            }
        };
        action
    }

    /// Rejects a claim before a sender transfer becomes irreversible.
    fn reject_h1_claim(
        origin: &Arc<Self>,
        id: ClaimId,
        availability: Option<SourceAvailability>,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.h1.finish_claim(id, availability);
        Self::prepare_action(origin, &mut state)
    }

    /// Removes a claim after its source endpoint is terminal.
    fn finish_h1_claim(
        origin: &Arc<Self>,
        id: ClaimId,
        availability: Option<SourceAvailability>,
    ) -> Option<AdmissionAction> {
        Self::reject_h1_claim(origin, id, availability)
    }

    /// Closes a borrow delivery fence and optionally schedules source fairness.
    fn finish_borrow_delivery(
        origin: &Arc<Self>,
        claim: ClaimId,
        delivery: DeliveryId,
        target: &CellId,
        result: TargetAckResult,
        transferred_source: Option<CellId>,
        rejected_availability: Option<SourceAvailability>,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.finish_delivery(delivery, target, result);
        if let Some(source) = transferred_source {
            return Some(AdmissionAction::H1(H1Action::CompleteSource(
                SourceCompletion {
                    origin: origin.clone(),
                    claim,
                    source,
                    transferred: true,
                    active: true,
                },
            )));
        }
        state.h1.finish_claim(claim, rejected_availability);
        Self::prepare_action(origin, &mut state)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/1 source scheduling and bounded return-claim ownership.
//!
//! Admission owns source advertisements, target selection, and the claim
//! record. A source cell owns one endpoint slot serialized with sender return.
//! Values crossing between those lock domains retain a typed fallback: an
//! install rejects its claim and a candidate returns its sender before the
//! shared delivery guard restores target demand.

use super::{
    AdmissionAction, DeliveryGuard, DeliveryId, DemandId, DemandSchedule, OriginAdmission,
    ProtocolRequirement, TargetAckResult,
};
use crate::client::pool::cell::h1::{H1Selection, ProvisionalH1};
use crate::client::pool::cell::{CellId, OriginCell};
use crate::client::pool::partition::EligibilityGroup;
use crate::sync::Arc;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;

/// Identity of one source-to-target HTTP/1 return claim.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::client::pool) struct ClaimId(u64);

impl ClaimId {
    /// Creates a deterministic identity for focused transition tests.
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
    /// FIFO view across every currently available source.
    origin_sources: SourceOrderState,
    /// FIFO views restricted by reuse eligibility.
    group_sources: HashMap<EligibilityGroup, SourceOrderState>,
    /// Nonterminal claims indexed by their never-reused identity.
    claims: HashMap<ClaimId, ClaimRecord>,
    /// Target demand episodes that already own a claim.
    claimed_targets: HashMap<CellId, ClaimId>,
    /// Installed claims whose target became stale.
    cancellations: VecDeque<ClaimId>,
    /// Next claim identity.
    next_claim: u64,
}

/// Complete source state reported after a source-cell transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct SourceAvailability {
    /// Whether at least one reusable or active HTTP/1 record remains.
    pub(in crate::client::pool) advertised: bool,
    /// Whether local H1 work temporarily excludes peer claims.
    pub(in crate::client::pool) blocked: bool,
}

/// Versioned source state crossing from a cell to admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct SourceSnapshot {
    /// Monotonic revision assigned under the source-cell lock.
    revision: u64,
    /// Complete availability at this revision.
    availability: SourceAvailability,
}

impl SourceSnapshot {
    /// Creates one complete source report.
    pub(in crate::client::pool) fn new(revision: u64, availability: SourceAvailability) -> Self {
        Self {
            revision,
            availability,
        }
    }
}

/// Terminal observation of a source endpoint.
pub(in crate::client::pool) enum SourceOutcome {
    /// The source remains live and reports its complete availability.
    Reported {
        source: CellId,
        snapshot: SourceSnapshot,
    },
    /// The source cell disappeared before the crossing completed.
    Expired { source: CellId },
}

impl SourceOutcome {
    /// Wraps a complete report with the source identity it describes.
    pub(in crate::client::pool) fn reported(source: CellId, snapshot: SourceSnapshot) -> Self {
        Self::Reported { source, snapshot }
    }

    /// Records that a source no longer exists.
    pub(in crate::client::pool) fn expired(source: CellId) -> Self {
        Self::Expired { source }
    }

    /// Returns the source whose admission view must change.
    fn source(&self) -> &CellId {
        match self {
            Self::Reported { source, .. } | Self::Expired { source } => source,
        }
    }
}

/// Source result after claim installation crosses from admission.
pub(in crate::client::pool) enum SourceInstallResult {
    /// A future reusable return will satisfy the claim.
    Installed,
    /// An idle sender was extracted immediately.
    Candidate(ClaimCandidate),
    /// The source could not retain the claim.
    Rejected(SourceSnapshot),
}

/// Admission's current view of one HTTP/1 source cell.
#[derive(Debug)]
struct SourceRecord {
    /// Reuse group whose peers may borrow this source's sender.
    group: EligibilityGroup,
    /// Newest source-cell report accepted by admission.
    revision: u64,
    /// Whether the cell has an H1 record that can return or be reclaimed.
    advertised: bool,
    /// Claim currently occupying the source endpoint.
    claim: Option<ClaimId>,
    /// Whether source-local work temporarily excludes peers.
    blocked: bool,
    /// Linked scheduling residence while this source is selectable.
    residence: SourceResidence,
}

impl SourceRecord {
    /// Returns whether this source must occupy both scheduling views.
    fn is_schedulable(&self) -> bool {
        self.advertised && !self.blocked && self.claim.is_none()
    }
}

/// Whether a source is linked in both scheduling views.
#[derive(Debug, Default)]
enum SourceResidence {
    /// The source is absent from source-selection order.
    #[default]
    Unavailable,
    /// The source is linked once in origin and group order.
    Available {
        origin: SourceLinks,
        group: SourceLinks,
    },
}

impl SourceResidence {
    /// Returns whether the source is linked in both views.
    fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Returns this residence's links for one scheduling view.
    fn links(&self, view: SourceView) -> Option<&SourceLinks> {
        match (self, view) {
            (Self::Available { origin, .. }, SourceView::Origin) => Some(origin),
            (Self::Available { group, .. }, SourceView::Group) => Some(group),
            (Self::Unavailable, _) => None,
        }
    }

    /// Returns mutable links while repairing one scheduling view.
    fn links_mut(&mut self, view: SourceView) -> &mut SourceLinks {
        match (self, view) {
            (Self::Available { origin, .. }, SourceView::Origin) => origin,
            (Self::Available { group, .. }, SourceView::Group) => group,
            (Self::Unavailable, _) => panic!("unavailable source has no order links"),
        }
    }
}

/// Selects one of the two intrusive source-order link sets.
#[derive(Clone, Copy)]
enum SourceView {
    /// Origin-wide reclaim source order.
    Origin,
    /// Eligibility-group borrow source order.
    Group,
}

/// Intrusive links for one source-selection view.
#[derive(Debug)]
struct SourceLinks {
    /// Previous available source in this view.
    previous: Option<CellId>,
    /// Next available source in this view.
    next: Option<CellId>,
}

/// Endpoints and length of one source-selection FIFO.
#[derive(Debug, Default)]
enum SourceOrderState {
    /// No source is schedulable.
    #[default]
    Empty,
    /// At least one source is linked in this view.
    Active {
        /// Oldest available source.
        head: CellId,
        /// Newest available source.
        tail: CellId,
        /// Number of linked sources.
        len: NonZeroUsize,
    },
}

impl SourceOrderState {
    /// Returns the oldest available source.
    fn head(&self) -> Option<&CellId> {
        match self {
            Self::Empty => None,
            Self::Active { head, .. } => Some(head),
        }
    }

    /// Returns the newest available source.
    fn tail(&self) -> Option<&CellId> {
        match self {
            Self::Empty => None,
            Self::Active { tail, .. } => Some(tail),
        }
    }

    /// Returns the number of linked sources.
    fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Active { len, .. } => len.get(),
        }
    }

    /// Appends one newly available source.
    fn push_back(&mut self, source: CellId) {
        match self {
            order @ Self::Empty => {
                *order = Self::Active {
                    head: source.clone(),
                    tail: source,
                    len: NonZeroUsize::MIN,
                };
            }
            Self::Active { tail, len, .. } => {
                *tail = source;
                *len = len
                    .checked_add(1)
                    .expect("HTTP/1 source-order length exhausted");
            }
        }
    }

    /// Removes one source after its neighboring links were repaired.
    fn remove(&mut self, source: &CellId, links: &SourceLinks) {
        let order = std::mem::take(self);
        let Self::Active { head, tail, len } = order else {
            unreachable!("removed a source from an empty order");
        };
        debug_assert_eq!(head == *source, links.previous.is_none());
        debug_assert_eq!(tail == *source, links.next.is_none());
        if len == NonZeroUsize::MIN {
            return;
        }
        *self = Self::Active {
            head: if head == *source {
                links
                    .next
                    .clone()
                    .expect("removed source head had no successor")
            } else {
                head
            },
            tail: if tail == *source {
                links
                    .previous
                    .clone()
                    .expect("removed source tail had no predecessor")
            } else {
                tail
            },
            len: NonZeroUsize::new(
                len.get()
                    .checked_sub(1)
                    .expect("HTTP/1 source-order length underflowed"),
            )
            .expect("nonempty source order lost its length"),
        };
    }
}

/// Admission-owned state for one nonterminal return claim.
#[derive(Clone, Debug)]
struct ClaimRecord {
    /// Source cell whose endpoint is occupied by the claim.
    source: CellId,
    /// Target cell whose demand caused the claim.
    target: CellId,
    /// Exact target-demand episode fenced by the claim.
    demand: DemandId,
    /// Whether resolution borrows a sender or reclaims its capacity.
    mode: ClaimMode,
    /// Admission-side progress of the claim.
    phase: ClaimPhase,
    /// Whether target demand became stale before source resolution.
    cancelled: bool,
}

/// Origin-side progress of one return claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaimPhase {
    /// Admission selected the source but its endpoint has not acknowledged.
    Installing,
    /// The source endpoint is waiting to intercept a future return.
    Installed,
    /// A provisional sender is resolving outside admission.
    Resolving,
    /// Target cancellation must be sent to the installed source endpoint.
    Cancelling,
}

/// Claim install work extracted from the admission lock.
#[derive(Debug)]
pub(in crate::client::pool) struct PreparedClaim {
    /// Never-reused claim identity.
    pub(in crate::client::pool) id: ClaimId,
    /// Source cell selected while admission was locked.
    pub(in crate::client::pool) source: CellId,
}

/// Source cancellation work extracted from admission.
pub(super) struct PreparedCancellation {
    /// Claim whose source endpoint must be cleared.
    id: ClaimId,
    /// Source cell that owns the endpoint.
    source: CellId,
}

impl H1Coordination {
    /// Publishes the source's complete current availability.
    pub(super) fn update_source(
        &mut self,
        source: CellId,
        group: EligibilityGroup,
        snapshot: SourceSnapshot,
    ) {
        if self
            .sources
            .get(&source)
            .is_some_and(|record| record.revision >= snapshot.revision)
        {
            return;
        }
        self.unlink_source(&source);
        let record = self
            .sources
            .entry(source.clone())
            .or_insert_with(|| SourceRecord {
                group: group.clone(),
                revision: snapshot.revision,
                advertised: snapshot.availability.advertised,
                claim: None,
                blocked: snapshot.availability.blocked,
                residence: SourceResidence::Unavailable,
            });
        record.group = group;
        record.revision = snapshot.revision;
        record.advertised = snapshot.availability.advertised;
        record.blocked = snapshot.availability.blocked;
        self.enqueue_source_if_available(&source);
        self.assert_consistent();
    }

    /// Marks a target claim stale after demand publication or delivery.
    pub(super) fn reconcile_target(&mut self, target: &CellId, schedule: &DemandSchedule) {
        let Some(claim) = self.claimed_targets.get(target).copied() else {
            return;
        };
        let Some(record) = self.claims.get_mut(&claim) else {
            self.claimed_targets.remove(target);
            self.assert_consistent();
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
        self.assert_consistent();
    }

    /// Selects a peer source for the oldest origin-capacity demand.
    ///
    /// HTTP/1 claims are demand-driven. The origin-capacity head is no younger
    /// than any compatible-group head: an eligible peer lends its sender, and
    /// otherwise an origin peer is reclaimed. A cell never claims from itself;
    /// local idle and returning senders are handled under the cell lock.
    pub(super) fn prepare_claim(&mut self, schedule: &DemandSchedule) -> Option<PreparedClaim> {
        let target = schedule.queued_head()?;
        if self.claimed_targets.contains_key(&target.target) {
            return None;
        }

        let (source, mode) = if target.requirement == ProtocolRequirement::H1Compatible {
            match self.take_group_peer(&target.eligibility_group, &target.target) {
                Some(source) => (source, ClaimMode::Borrow),
                None => (self.take_origin_peer(&target.target)?, ClaimMode::Reclaim),
            }
        } else {
            (self.take_origin_peer(&target.target)?, ClaimMode::Reclaim)
        };

        let id = self.take_claim_id();
        let source_record = self
            .sources
            .get_mut(&source)
            .expect("selected HTTP/1 source disappeared");
        debug_assert!(source_record.claim.is_none());
        source_record.claim = Some(id);
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
        self.assert_consistent();
        Some(PreparedClaim { id, source })
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
        let record = record.clone();
        self.assert_consistent();
        Some(record)
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
        let record = record.clone();
        self.assert_consistent();
        Some(record)
    }

    /// Removes one claim and applies the source's terminal report.
    ///
    /// The outcome names its source explicitly so an expired cell cannot be
    /// mistaken for an unchanged source and a duplicate terminal report still
    /// refreshes admission's source view.
    fn finish_claim(&mut self, id: ClaimId, outcome: SourceOutcome) -> Option<ClaimRecord> {
        let outcome_source = outcome.source().clone();
        self.apply_source_outcome(outcome);

        let record = self.claims.remove(&id);
        if let Some(record) = record.as_ref() {
            debug_assert_eq!(record.source, outcome_source);
            if self.claimed_targets.get(&record.target) == Some(&id) {
                self.claimed_targets.remove(&record.target);
            }
            if let Some(source) = self.sources.get_mut(&record.source) {
                if source.claim == Some(id) {
                    source.claim = None;
                }
            }
        }

        self.enqueue_source_if_available(&outcome_source);
        self.assert_consistent();
        record
    }

    /// Applies a report without assuming its claim record still exists.
    fn apply_source_outcome(&mut self, outcome: SourceOutcome) {
        match outcome {
            SourceOutcome::Reported { source, snapshot } => {
                if self
                    .sources
                    .get(&source)
                    .is_some_and(|record| record.revision >= snapshot.revision)
                {
                    return;
                }
                self.unlink_source(&source);
                if let Some(record) = self.sources.get_mut(&source) {
                    record.revision = snapshot.revision;
                    record.advertised = snapshot.availability.advertised;
                    record.blocked = snapshot.availability.blocked;
                }
            }
            SourceOutcome::Expired { source } => {
                self.unlink_source(&source);
                self.sources.remove(&source);
            }
        }
    }

    /// Removes and returns the first eligible peer in one reuse group.
    fn take_group_peer(&mut self, group: &EligibilityGroup, target: &CellId) -> Option<CellId> {
        let source = {
            let order = self.group_sources.get(group)?;
            self.first_peer(order, target, SourceView::Group)?
        };
        self.unlink_source(&source);
        Some(source)
    }

    /// Removes and returns the first origin-wide peer.
    fn take_origin_peer(&mut self, target: &CellId) -> Option<CellId> {
        let source = self.first_peer(&self.origin_sources, target, SourceView::Origin)?;
        self.unlink_source(&source);
        Some(source)
    }

    /// Returns the head, or its successor when the head is the target itself.
    fn first_peer(
        &self,
        order: &SourceOrderState,
        target: &CellId,
        view: SourceView,
    ) -> Option<CellId> {
        let head = order.head()?.clone();
        if head != *target {
            return Some(head);
        }
        let record = self
            .sources
            .get(&head)
            .expect("source-order head disappeared");
        record
            .residence
            .links(view)
            .and_then(|links| links.next.clone())
    }

    /// Appends an unclaimed advertised source to both scheduling views.
    fn enqueue_source_if_available(&mut self, source: &CellId) {
        let Some(record) = self.sources.get(source) else {
            return;
        };
        if !record.is_schedulable() || record.residence.is_available() {
            return;
        }
        let group = record.group.clone();
        let origin_previous = self.origin_sources.tail().cloned();
        let group_previous = self
            .group_sources
            .get(&group)
            .and_then(SourceOrderState::tail)
            .cloned();

        if let Some(previous) = origin_previous.as_ref() {
            self.sources
                .get_mut(previous)
                .expect("origin source-order tail disappeared")
                .residence
                .links_mut(SourceView::Origin)
                .next = Some(source.clone());
        }
        if let Some(previous) = group_previous.as_ref() {
            self.sources
                .get_mut(previous)
                .expect("group source-order tail disappeared")
                .residence
                .links_mut(SourceView::Group)
                .next = Some(source.clone());
        }

        self.sources
            .get_mut(source)
            .expect("enqueued source disappeared")
            .residence = SourceResidence::Available {
            origin: SourceLinks {
                previous: origin_previous,
                next: None,
            },
            group: SourceLinks {
                previous: group_previous,
                next: None,
            },
        };
        self.origin_sources.push_back(source.clone());
        self.group_sources
            .entry(group)
            .or_default()
            .push_back(source.clone());
    }

    /// Unlinks a source eagerly from both scheduling views.
    fn unlink_source(&mut self, source: &CellId) {
        let Some(record) = self.sources.get_mut(source) else {
            return;
        };
        let residence = std::mem::take(&mut record.residence);
        let SourceResidence::Available { origin, group } = residence else {
            return;
        };
        let group_key = record.group.clone();

        Self::repair_links(&mut self.sources, source, &origin, SourceView::Origin);
        self.origin_sources.remove(source, &origin);

        Self::repair_links(&mut self.sources, source, &group, SourceView::Group);
        self.group_sources
            .get_mut(&group_key)
            .expect("available source lost its group order")
            .remove(source, &group);
    }

    /// Repairs neighboring links after one source leaves an order.
    fn repair_links(
        sources: &mut HashMap<CellId, SourceRecord>,
        source: &CellId,
        links: &SourceLinks,
        view: SourceView,
    ) {
        if let Some(previous) = links.previous.as_ref() {
            sources
                .get_mut(previous)
                .expect("previous source disappeared")
                .residence
                .links_mut(view)
                .next = links.next.clone();
        }
        if let Some(next) = links.next.as_ref() {
            sources
                .get_mut(next)
                .expect("next source disappeared")
                .residence
                .links_mut(view)
                .previous = links.previous.clone();
        }
        debug_assert_ne!(links.previous.as_ref(), Some(source));
        debug_assert_ne!(links.next.as_ref(), Some(source));
    }

    /// Allocates a claim identity.
    fn take_claim_id(&mut self) -> ClaimId {
        let value = self.next_claim;
        self.next_claim = value
            .checked_add(1)
            .expect("HTTP/1 claim identity exhausted");
        ClaimId(value)
    }

    /// Checks claim indexes and both source orders after every mutation.
    fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        self.assert_consistent_debug();
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        for (id, claim) in &self.claims {
            assert_eq!(
                self.claimed_targets.get(&claim.target),
                Some(id),
                "claim target index did not name its claim"
            );
            assert_eq!(
                self.sources
                    .get(&claim.source)
                    .and_then(|source| source.claim),
                Some(*id),
                "claim source index did not name its claim"
            );
        }
        for (target, id) in &self.claimed_targets {
            assert_eq!(
                self.claims.get(id).map(|claim| &claim.target),
                Some(target),
                "target index named a missing claim"
            );
        }
        for (source, record) in &self.sources {
            if let Some(id) = record.claim {
                assert_eq!(
                    self.claims.get(&id).map(|claim| &claim.source),
                    Some(source),
                    "source index named a missing claim"
                );
            }
            assert_eq!(
                record.residence.is_available(),
                record.is_schedulable(),
                "source scheduling residence did not match availability"
            );
        }
        self.assert_order(&self.origin_sources, None, SourceView::Origin);
        for (group, order) in &self.group_sources {
            self.assert_order(order, Some(group), SourceView::Group);
        }
    }

    #[cfg(debug_assertions)]
    fn assert_order(
        &self,
        order: &SourceOrderState,
        expected_group: Option<&EligibilityGroup>,
        view: SourceView,
    ) {
        let expected = self
            .sources
            .values()
            .filter(|record| {
                record.residence.is_available()
                    && expected_group.is_none_or(|group| &record.group == group)
            })
            .count();
        let mut current = order.head().cloned();
        let mut previous = None;
        let mut traversed = 0;
        while let Some(source) = current {
            assert!(
                traversed < self.sources.len(),
                "HTTP/1 source order contains a cycle"
            );
            let record = self
                .sources
                .get(&source)
                .expect("ordered HTTP/1 source disappeared");
            if let Some(group) = expected_group {
                assert_eq!(
                    &record.group, group,
                    "source appeared in the wrong eligibility order"
                );
            }
            let links = record
                .residence
                .links(view)
                .expect("ordered source lost its links");
            assert_eq!(
                links.previous, previous,
                "source order contains inconsistent backward links"
            );
            previous = Some(source);
            current = links.next.clone();
            traversed += 1;
        }
        assert_eq!(
            expected, traversed,
            "source order omitted available records"
        );
        assert_eq!(order.len(), traversed, "source order length was incorrect");
        assert_eq!(
            order.tail().cloned(),
            previous,
            "source-order tail was not reachable"
        );
    }
}

/// One unlocked HTTP/1 coordination step.
pub(in crate::client::pool) enum H1Action {
    /// Install a prepared claim in its source cell.
    Install(ClaimInstall),
    /// Cancel an installed source endpoint.
    Cancel(ClaimCancel),
    /// Close a claimed source connection and release capacity.
    Reclaim(ReclaimAction),
    /// Complete a source endpoint after target transfer.
    CompleteSource(SourceCompletion),
}

impl H1Action {
    /// Creates an unlocked source-install crossing.
    pub(super) fn install(origin: Arc<OriginAdmission>, claim: PreparedClaim) -> Self {
        Self::Install(ClaimInstall {
            origin,
            claim: Some(claim),
        })
    }

    /// Creates an unlocked source-cancellation crossing.
    pub(super) fn cancel(origin: Arc<OriginAdmission>, cancellation: PreparedCancellation) -> Self {
        Self::Cancel(ClaimCancel {
            origin,
            cancellation: Some(cancellation),
        })
    }

    /// Executes one crossing and returns the next admission action.
    pub(super) fn drive_once(self) -> Option<AdmissionAction> {
        match self {
            Self::Install(action) => action.drive_once(),
            Self::Cancel(action) => action.drive_once(),
            Self::Reclaim(action) => action.drive_once(),
            Self::CompleteSource(action) => action.drive_once(),
        }
    }
}

/// Claim installation crossing from admission to a source cell.
pub(in crate::client::pool) struct ClaimInstall {
    /// Admission authority that prepared and owns the claim record.
    origin: Arc<OriginAdmission>,
    /// Source install work still owned by this fallback.
    claim: Option<PreparedClaim>,
}

impl ClaimInstall {
    /// Installs the prepared endpoint without holding the admission lock.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let claim = self
            .claim
            .take()
            .expect("HTTP/1 claim install consumed more than once");
        let Some(source) = self.origin.target(&claim.source) else {
            return OriginAdmission::finish_h1_claim_source(
                &self.origin,
                claim.id,
                SourceOutcome::expired(claim.source),
            );
        };
        OriginCell::install_h1_claim(&source, self.origin.clone(), claim)
    }
}

impl Drop for ClaimInstall {
    fn drop(&mut self) {
        if let Some(claim) = self.claim.take() {
            let outcome = source_outcome(&self.origin, &claim.source, |source| {
                source.cancel_h1_claim(claim.id)
            });
            let next = OriginAdmission::finish_h1_claim_source(&self.origin, claim.id, outcome);
            OriginAdmission::drive(next);
        }
    }
}

/// Installed claim cancellation crossing to its source endpoint.
pub(in crate::client::pool) struct ClaimCancel {
    /// Admission authority that owns the claim record.
    origin: Arc<OriginAdmission>,
    /// Cancellation work still owned by this guard.
    cancellation: Option<PreparedCancellation>,
}

impl ClaimCancel {
    /// Clears the source endpoint and completes the admission claim.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let cancellation = self
            .cancellation
            .take()
            .expect("HTTP/1 claim cancellation consumed more than once");
        let outcome = source_outcome(&self.origin, &cancellation.source, |source| {
            source.cancel_h1_claim(cancellation.id)
        });
        OriginAdmission::finish_h1_claim_source(&self.origin, cancellation.id, outcome)
    }
}

impl Drop for ClaimCancel {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            let outcome = source_outcome(&self.origin, &cancellation.source, |source| {
                source.cancel_h1_claim(cancellation.id)
            });
            let next =
                OriginAdmission::finish_h1_claim_source(&self.origin, cancellation.id, outcome);
            OriginAdmission::drive(next);
        }
    }
}

/// Provisional sender owned by one resolving claim.
pub(in crate::client::pool) struct ClaimCandidate {
    /// Admission authority that owns the other claim endpoint.
    origin: Arc<OriginAdmission>,
    /// Claim whose source endpoint owns the provisional sender.
    claim: ClaimId,
    /// Source cell that must revalidate or receive the sender back.
    source: CellId,
    /// Sender fallback while source resolution is incomplete.
    provisional: Option<ProvisionalH1>,
}

impl ClaimCandidate {
    /// Takes provisional sender ownership for one resolving source claim.
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
    ///
    /// Failure returns this guard intact so dropping it restores the sender to
    /// ordinary source handling and completes the claim exactly once.
    pub(in crate::client::pool) fn commit(mut self) -> Result<H1Selection, Self> {
        let Some(source) = self.origin.target(&self.source) else {
            return Err(self);
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 claim candidate consumed more than once");
        match OriginCell::commit_h1_claim(&source, self.claim, provisional) {
            Ok(selection) => Ok(selection),
            Err(provisional) => {
                self.provisional = Some(provisional);
                Err(self)
            }
        }
    }

    /// Attempts reclaim and returns the source's explicit terminal outcome.
    fn reclaim(mut self) -> SourceOutcome {
        let Some(source) = self.origin.target(&self.source) else {
            drop(self.provisional.take());
            return SourceOutcome::expired(self.source.clone());
        };
        let provisional = self
            .provisional
            .take()
            .expect("HTTP/1 claim candidate consumed more than once");
        let availability = match OriginCell::reclaim_h1_claim(&source, self.claim, provisional) {
            Ok(availability) => availability,
            Err(provisional) => {
                OriginCell::reject_h1_claim_candidate(&source, self.claim, provisional)
            }
        };
        SourceOutcome::reported(self.source.clone(), availability)
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
        let outcome = match self.origin.target(&self.source) {
            Some(source) => SourceOutcome::reported(
                self.source.clone(),
                OriginCell::reject_h1_claim_candidate(&source, self.claim, provisional),
            ),
            None => {
                drop(provisional);
                SourceOutcome::expired(self.source.clone())
            }
        };
        let next = OriginAdmission::finish_h1_claim_source(&self.origin, self.claim, outcome);
        OriginAdmission::drive(next);
    }
}

/// Reclaim decision carrying the claimed provisional sender.
pub(in crate::client::pool) struct ReclaimAction {
    /// Admission authority that selected reclaim.
    origin: Arc<OriginAdmission>,
    /// Claim completed by the reclaim attempt.
    claim: ClaimId,
    /// Candidate whose `Drop` is the fallback before execution.
    candidate: Option<ClaimCandidate>,
}

impl ReclaimAction {
    /// Attempts logical close outside admission and reports the source result.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let candidate = self
            .candidate
            .take()
            .expect("HTTP/1 reclaim action consumed more than once");
        let outcome = candidate.reclaim();
        OriginAdmission::finish_h1_claim_source(&self.origin, self.claim, outcome)
    }
}

/// Source endpoint completion after an irreversible borrowed transfer.
pub(in crate::client::pool) struct SourceCompletion {
    /// Admission authority that owns the remaining claim endpoint.
    origin: Arc<OriginAdmission>,
    /// Claim completed at the source.
    claim: ClaimId,
    /// Source whose endpoint must be released.
    source: CellId,
    /// Whether the target accepted and owns the sender.
    transferred: bool,
    /// Whether `Drop` still owns endpoint completion.
    active: bool,
}

impl SourceCompletion {
    /// Completes the source endpoint outside admission.
    fn drive_once(mut self) -> Option<AdmissionAction> {
        let outcome = source_outcome(&self.origin, &self.source, |source| {
            source.complete_h1_claim(self.claim, self.transferred)
        });
        let action = OriginAdmission::finish_h1_claim_source(&self.origin, self.claim, outcome);
        self.active = false;
        action
    }
}

impl Drop for SourceCompletion {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let outcome = source_outcome(&self.origin, &self.source, |source| {
            source.complete_h1_claim(self.claim, self.transferred)
        });
        let next = OriginAdmission::finish_h1_claim_source(&self.origin, self.claim, outcome);
        OriginAdmission::drive(next);
    }
}

/// Produces a complete source outcome without overloading absence as no-op.
fn source_outcome(
    origin: &OriginAdmission,
    source: &CellId,
    report: impl FnOnce(&Arc<OriginCell>) -> SourceSnapshot,
) -> SourceOutcome {
    match origin.target(source) {
        Some(cell) => SourceOutcome::reported(source.clone(), report(&cell)),
        None => SourceOutcome::expired(source.clone()),
    }
}

impl OriginAdmission {
    /// Publishes a source-cell availability change and drives bounded progress.
    pub(in crate::client::pool) fn update_h1_source(
        origin: &Arc<Self>,
        source: CellId,
        group: EligibilityGroup,
        snapshot: SourceSnapshot,
    ) {
        let action = {
            let mut state = origin.state.lock();
            state.h1.update_source(source, group, snapshot);
            Self::prepare_action(origin, &mut state)
        };
        Self::drive(action);
    }

    /// Completes a claim whose returning sender was no longer reusable.
    pub(in crate::client::pool) fn reject_returned_h1_claim(
        origin: &Arc<Self>,
        id: ClaimId,
        source: CellId,
        snapshot: SourceSnapshot,
    ) {
        let action =
            Self::finish_h1_claim_source(origin, id, SourceOutcome::reported(source, snapshot));
        Self::drive(action);
    }

    /// Applies a source-cell claim-install result.
    pub(in crate::client::pool) fn finish_h1_claim_install(
        origin: &Arc<Self>,
        id: ClaimId,
        source: CellId,
        result: SourceInstallResult,
    ) -> Option<AdmissionAction> {
        match result {
            SourceInstallResult::Rejected(availability) => Self::finish_h1_claim_source(
                origin,
                id,
                SourceOutcome::reported(source, availability),
            ),
            SourceInstallResult::Installed => {
                let mut state = origin.state.lock();
                let record = state.h1.finish_install(id, false);
                if record.as_ref().is_some_and(|record| record.cancelled) {
                    state.h1.cancellations.push_back(id);
                }
                Self::prepare_action(origin, &mut state)
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
                    Some(AdmissionAction::Delivery(DeliveryGuard::borrowed_h1(
                        origin.clone(),
                        delivery,
                        scheduled.target,
                        scheduled.demand,
                        id,
                        record.source,
                        candidate,
                    )))
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

    /// Applies a terminal source outcome and schedules the next admission action.
    fn finish_h1_claim_source(
        origin: &Arc<Self>,
        id: ClaimId,
        outcome: SourceOutcome,
    ) -> Option<AdmissionAction> {
        let mut state = origin.state.lock();
        state.h1.finish_claim(id, outcome);
        Self::prepare_action(origin, &mut state)
    }

    /// Closes a borrow delivery fence and schedules source completion.
    pub(super) fn finish_borrow_delivery(
        origin: &Arc<Self>,
        claim: ClaimId,
        delivery: DeliveryId,
        target: &CellId,
        result: TargetAckResult,
        transferred_source: Option<CellId>,
        rejected_outcome: Option<SourceOutcome>,
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
        state.h1.finish_claim(
            claim,
            rejected_outcome.expect("rejected borrow had no source outcome"),
        );
        Self::prepare_action(origin, &mut state)
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::{DemandSnapshot, SnapshotVersion};
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::PartitionId;
    use http_1x::uri::Scheme;

    fn cell(index: usize) -> CellId {
        CellId::new(
            PartitionId::from_index(index),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
        )
    }

    fn schedule(target: CellId, group: EligibilityGroup) -> DemandSchedule {
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            target,
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H1Compatible,
                group,
            ),
        );
        schedule
    }

    fn source_snapshot(revision: u64, advertised: bool, blocked: bool) -> SourceSnapshot {
        SourceSnapshot::new(
            revision,
            SourceAvailability {
                advertised,
                blocked,
            },
        )
    }

    #[test]
    fn source_publication_keeps_one_bounded_residence() {
        let source = cell(1);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Coordination::default();

        for revision in 1..=500 {
            coordination.update_source(
                source.clone(),
                group.clone(),
                source_snapshot(revision, true, false),
            );
        }

        assert_eq!(1, coordination.sources.len());
        assert_eq!(1, coordination.origin_sources.len());
        assert_eq!(
            1,
            coordination
                .group_sources
                .get(&group)
                .expect("source group was not published")
                .len()
        );

        coordination.update_source(
            source.clone(),
            group.clone(),
            source_snapshot(501, false, false),
        );
        assert_eq!(0, coordination.origin_sources.len());
        assert_eq!(
            0,
            coordination
                .group_sources
                .get(&group)
                .expect("source group disappeared")
                .len()
        );
    }

    #[test]
    fn claim_selection_skips_the_target_cell() {
        let target = cell(1);
        let peer = cell(2);
        let group = EligibilityGroup::Pool;
        let mut coordination = H1Coordination::default();
        coordination.update_source(
            target.clone(),
            group.clone(),
            source_snapshot(1, true, false),
        );
        coordination.update_source(peer.clone(), group.clone(), source_snapshot(1, true, false));

        let claim = coordination
            .prepare_claim(&schedule(target.clone(), group))
            .expect("peer source was not selected");
        assert_eq!(peer, claim.source);
        assert_ne!(target, claim.source);
    }

    #[test]
    fn expired_source_terminates_its_claim_without_republication() {
        let source = cell(1);
        let target = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(target, group.clone());
        let mut coordination = H1Coordination::default();
        coordination.update_source(source.clone(), group, source_snapshot(1, true, false));
        let claim = coordination
            .prepare_claim(&schedule)
            .expect("source did not produce a claim");

        coordination.finish_claim(claim.id, SourceOutcome::expired(source.clone()));

        assert!(!coordination.sources.contains_key(&source));
        assert!(coordination.prepare_claim(&schedule).is_none());
        assert!(coordination.claims.is_empty());
    }

    #[test]
    fn stale_terminal_report_cannot_hide_a_newer_available_source() {
        let source = cell(1);
        let target = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(target, group.clone());
        let mut coordination = H1Coordination::default();
        coordination.update_source(source.clone(), group, source_snapshot(1, true, false));
        let claim = coordination
            .prepare_claim(&schedule)
            .expect("source did not produce a claim");

        coordination.finish_claim(
            claim.id,
            SourceOutcome::reported(source.clone(), source_snapshot(3, true, false)),
        );
        coordination.finish_claim(
            claim.id,
            SourceOutcome::reported(source.clone(), source_snapshot(2, false, false)),
        );

        let source = coordination
            .sources
            .get(&source)
            .expect("stale report removed the source");
        assert!(source.advertised);
        assert!(source.residence.is_available());
        assert_eq!(3, source.revision);
    }

    #[test]
    fn duplicate_terminal_report_still_refreshes_source_state() {
        let source = cell(1);
        let target = cell(2);
        let group = EligibilityGroup::Pool;
        let schedule = schedule(target, group.clone());
        let mut coordination = H1Coordination::default();
        coordination.update_source(source.clone(), group, source_snapshot(1, true, false));
        let claim = coordination
            .prepare_claim(&schedule)
            .expect("source did not produce a claim");

        coordination.finish_claim(
            claim.id,
            SourceOutcome::reported(source.clone(), source_snapshot(2, true, false)),
        );
        coordination.finish_claim(
            claim.id,
            SourceOutcome::reported(source.clone(), source_snapshot(3, false, false)),
        );

        let source = coordination
            .sources
            .get(&source)
            .expect("live source report removed the source");
        assert!(!source.advertised);
        assert!(!source.residence.is_available());
    }
}

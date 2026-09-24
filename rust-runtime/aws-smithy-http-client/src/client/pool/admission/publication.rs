/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cross-cell HTTP/2 generation publication for bounded origins.
//!
//! A connection cell advertises only its exact accepting generation. Admission
//! pairs that identity with the oldest demand in the same eligibility group.
//! The requesting cell stores an identity-only route; the connection-owning
//! cell retains its sender, driver, socket, and capacity lease.
//!
//! ```text
//! connection-cell report -> advertisement indexed by eligibility group
//! group demand + peer advertisement -> publication fence
//! connection generation validation -> requesting-cell route visibility
//! requesting-cell visibility -> admission acknowledgement
//! stale connection or requesting cell -> retry or retire fenced demand
//! idle generation + H1-required demand -> exact-generation reclaim
//! ```
//!
//! An accepting generation with no prospective or accepted requests also
//! enters an origin-wide reclaim order. H1-required demand may reserve one
//! exact idle generation, revalidate it under the connection-cell lock, and
//! close it to return bounded capacity. Busy generations never enter that
//! order.
//!
//! The connection-owning and requesting cell locks never nest.
//! [`H2PublicationGuard`] owns the admission fence between those scopes and
//! submits a terminal acknowledgement on drop. [`H2ReclaimAction`] owns an
//! idle-generation reservation and repairs admission's view if the crossing is
//! rejected or dropped.

use super::{
    AdmissionAction, DeliveryAckResult, DeliveryId, DemandId, DemandSchedule, IntrusiveLinks,
    IntrusiveOrder, OriginAdmission,
};
use crate::client::pool::cell::h2::{H2GenerationId, H2Route};
use crate::client::pool::cell::OriginCell;
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use crate::sync::Arc;
use std::collections::{BTreeSet, HashMap};
use std::ops::Bound::{Excluded, Unbounded};

/// Complete connection-cell advertisement at one monotonic revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct H2AdvertisementSnapshot {
    /// Connection-cell revision used to reject stale reports.
    revision: u64,
    /// Exact accepting generation, or `None` after withdrawal.
    generation: Option<H2GenerationId>,
    /// Whether the accepting generation has no prospective or accepted requests.
    idle: bool,
}

impl H2AdvertisementSnapshot {
    /// Reports one accepting generation.
    pub(in crate::client::pool) fn accepting(revision: u64, generation: H2GenerationId) -> Self {
        Self {
            revision,
            generation: Some(generation),
            idle: false,
        }
    }

    /// Reports one accepting generation with no request work.
    pub(in crate::client::pool) fn idle(revision: u64, generation: H2GenerationId) -> Self {
        Self {
            revision,
            generation: Some(generation),
            idle: true,
        }
    }

    /// Reports that the connection cell has no accepting generation.
    pub(in crate::client::pool) fn unavailable(revision: u64) -> Self {
        Self {
            revision,
            generation: None,
            idle: false,
        }
    }
}

/// Admission-owned H2 advertisements and publication-ready groups.
///
/// At every completed transition:
///
/// - each connection cell has at most one retained advertisement record;
/// - each advertised connection cell is linked once in its eligibility-group order;
/// - each idle advertised generation is linked once in the origin reclaim
///   order unless it owns the active reclaim reservation;
/// - each linked identity names the newest reported generation;
/// - `ready_groups` contains exactly the groups with H2-compatible queued
///   demand and an advertised peer connection;
/// - at most one reclaim reservation crosses to a connection cell; and
/// - advertisements, prepared publications, and reclaim reservations own no
///   connection capacity or protocol sender.
#[derive(Debug, Default)]
pub(super) struct H2Publication {
    /// Latest report retained for each connection cell.
    advertisements: HashMap<PartitionId, AdvertisementRecord>,
    /// Advertised connection cells ordered within each reuse group.
    group_orders: HashMap<EligibilityGroup, IntrusiveOrder<PartitionId>>,
    /// Idle accepting generations ordered across the origin for reclaim.
    idle_order: IntrusiveOrder<PartitionId>,
    /// Groups that have both compatible demand and a peer advertisement.
    ready_groups: BTreeSet<EligibilityGroup>,
    /// Group selected by the previous publication turn.
    last_ready_group: Option<EligibilityGroup>,
    /// Exact idle generation currently crossing to its connection cell.
    reclaiming: Option<PreparedH2Reclaim>,
}

/// Admission's latest complete report for one connection cell.
#[derive(Debug)]
struct AdvertisementRecord {
    /// Reuse group in which this connection may be published.
    group: EligibilityGroup,
    /// Newest connection-cell report retained by admission.
    revision: u64,
    /// Exact accepting generation named by the report.
    generation: Option<H2GenerationId>,
    /// Intrusive-order residence for an available generation.
    residence: AdvertisementResidence,
    /// Whether the accepting generation has no request work.
    idle: bool,
    /// Links in the origin-wide idle-generation order.
    idle_links: Option<IntrusiveLinks<PartitionId>>,
}

/// Whether one connection cell occupies its advertisement order.
#[derive(Debug, Default)]
enum AdvertisementResidence {
    /// This connection cell has no generation available to peers.
    #[default]
    Unavailable,
    /// The cell is linked in its eligibility-group advertisement order.
    Advertised {
        /// Intrusive links owned by this residence.
        links: IntrusiveLinks<PartitionId>,
    },
}

impl AdvertisementResidence {
    fn links(&self) -> Option<&IntrusiveLinks<PartitionId>> {
        match self {
            Self::Unavailable => None,
            Self::Advertised { links } => Some(links),
        }
    }

    fn links_mut(&mut self) -> &mut IntrusiveLinks<PartitionId> {
        match self {
            Self::Unavailable => panic!("unavailable H2 connection had advertisement links"),
            Self::Advertised { links } => links,
        }
    }
}

/// Identity selected before connection and requesting cell validation.
#[derive(Clone, Debug)]
pub(super) struct PreparedH2Publication {
    /// Admission fence identity.
    pub(super) delivery: DeliveryId,
    /// Cell whose demand is fenced.
    pub(super) requesting_partition: PartitionId,
    /// Exact demand generation selected for publication.
    pub(super) demand: DemandId,
    /// Cell that owns the advertised connection.
    pub(super) connection_partition: PartitionId,
    /// Exact accepting generation selected from the advertisement.
    pub(super) generation: H2GenerationId,
    /// Reuse group shared by demand and advertisement.
    pub(super) group: EligibilityGroup,
}

/// Exact idle generation selected to release bounded origin capacity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PreparedH2Reclaim {
    /// Cell whose H1-required demand caused reclaim.
    pub(super) requesting_partition: PartitionId,
    /// Exact demand generation that caused reclaim.
    pub(super) demand: DemandId,
    /// Cell that owns the idle H2 generation.
    pub(super) connection_partition: PartitionId,
    /// Exact accepting generation observed idle by admission.
    pub(super) generation: H2GenerationId,
    /// Reuse group retained for advertisement repair.
    pub(super) group: EligibilityGroup,
}

/// Candidate selected from stored demand and advertisement heads.
struct PublicationCandidate {
    /// Requesting cell selected from the group demand head.
    requesting_partition: PartitionId,
    /// Demand generation at that head.
    demand: DemandId,
    /// Peer connection cell selected from the advertisement head.
    connection_partition: PartitionId,
    /// Advertised generation in that connection cell.
    generation: H2GenerationId,
    /// Eligibility group whose turn was selected.
    group: EligibilityGroup,
}

impl H2Publication {
    /// Returns whether some eligibility group may start a publication turn.
    pub(super) fn has_ready_group(&self) -> bool {
        !self.ready_groups.is_empty()
    }

    /// Replaces one connection cell's advertisement and repairs its index.
    pub(super) fn update(
        &mut self,
        connection_partition: PartitionId,
        group: EligibilityGroup,
        snapshot: H2AdvertisementSnapshot,
        demand: &DemandSchedule,
    ) {
        if self
            .advertisements
            .get(&connection_partition)
            .is_some_and(|record| record.revision >= snapshot.revision)
        {
            return;
        }

        let old_group = self
            .advertisements
            .get(&connection_partition)
            .map(|record| record.group.clone());
        self.unlink(&connection_partition);
        let record = self
            .advertisements
            .entry(connection_partition)
            .or_insert_with(|| AdvertisementRecord {
                group: group.clone(),
                revision: snapshot.revision,
                generation: snapshot.generation,
                residence: AdvertisementResidence::Unavailable,
                idle: snapshot.idle,
                idle_links: None,
            });
        record.group = group.clone();
        record.revision = snapshot.revision;
        record.generation = snapshot.generation;
        record.idle = snapshot.idle;
        if record.generation.is_some() {
            self.enqueue(connection_partition);
        }
        self.enqueue_idle_if_available(connection_partition);

        if let Some(old_group) = old_group {
            self.reconcile_group(&old_group, demand);
        }
        self.reconcile_group(&group, demand);
        self.assert_consistent(demand);
    }

    /// Removes one stale generation observed during connection-cell validation.
    pub(super) fn remove_if_exact(
        &mut self,
        connection_partition: &PartitionId,
        generation: H2GenerationId,
        demand: &DemandSchedule,
    ) {
        let Some(record) = self.advertisements.get(connection_partition) else {
            return;
        };
        if record.generation != Some(generation) {
            return;
        }
        let group = record.group.clone();
        self.unlink(connection_partition);
        let record = self
            .advertisements
            .get_mut(connection_partition)
            .expect("validated H2 advertisement disappeared");
        record.generation = None;
        record.idle = false;
        self.reconcile_group(&group, demand);
        self.assert_consistent(demand);
    }

    /// Recomputes whether one group can start a publication.
    pub(super) fn reconcile_group(&mut self, group: &EligibilityGroup, demand: &DemandSchedule) {
        let ready = demand
            .queued_group_head(group)
            .and_then(|queued| {
                if !queued.requirement.accepts_h2() {
                    return None;
                }
                self.first_peer(group, &queued.requesting_partition)
                    .map(|_| queued)
            })
            .is_some();
        if ready {
            self.ready_groups.insert(group.clone());
        } else {
            self.ready_groups.remove(group);
        }
    }

    /// Selects one group head and one peer advertisement without scanning cells.
    fn candidate(&mut self, demand: &DemandSchedule) -> Option<PublicationCandidate> {
        let group = self
            .last_ready_group
            .as_ref()
            .and_then(|last| self.ready_groups.range((Excluded(last), Unbounded)).next())
            .or_else(|| self.ready_groups.first())?
            .clone();
        let Some(queued) = demand.queued_group_head(&group) else {
            self.ready_groups.remove(&group);
            return None;
        };
        if !queued.requirement.accepts_h2() {
            self.ready_groups.remove(&group);
            return None;
        }
        let Some(connection_partition) = self.first_peer(&group, &queued.requesting_partition)
        else {
            self.ready_groups.remove(&group);
            return None;
        };
        let record = self
            .advertisements
            .get(&connection_partition)
            .expect("selected H2 advertisement disappeared");
        let generation = record
            .generation
            .expect("selected H2 advertisement had no generation");
        Some(PublicationCandidate {
            requesting_partition: queued.requesting_partition,
            demand: queued.demand,
            connection_partition,
            generation,
            group,
        })
    }

    /// Reserves one group demand and creates its unlocked publication identity.
    pub(super) fn prepare(
        &mut self,
        demand: &mut DemandSchedule,
        delivery: DeliveryId,
    ) -> Option<PreparedH2Publication> {
        let candidate = self.candidate(demand)?;
        demand.reserve_group_head(
            &candidate.group,
            &candidate.requesting_partition,
            candidate.demand,
            delivery,
        )?;
        self.last_ready_group = Some(candidate.group.clone());
        self.reconcile_group(&candidate.group, demand);
        self.assert_consistent(demand);
        Some(PreparedH2Publication {
            delivery,
            requesting_partition: candidate.requesting_partition,
            demand: candidate.demand,
            connection_partition: candidate.connection_partition,
            generation: candidate.generation,
            group: candidate.group,
        })
    }

    /// Reserves the oldest idle H2 generation for H1-required origin demand.
    pub(super) fn prepare_reclaim(&mut self, demand: &DemandSchedule) -> Option<PreparedH2Reclaim> {
        if self.reclaiming.is_some() {
            return None;
        }
        let queued = demand.queued_head()?;
        if queued.requirement.accepts_h2() {
            return None;
        }
        let connection_partition = self.idle_order.head()?;
        let record = self
            .advertisements
            .get(&connection_partition)
            .expect("idle HTTP/2 advertisement disappeared");
        let prepared = PreparedH2Reclaim {
            requesting_partition: queued.requesting_partition,
            demand: queued.demand,
            connection_partition,
            generation: record
                .generation
                .expect("idle HTTP/2 advertisement had no generation"),
            group: record.group.clone(),
        };
        self.reclaiming = Some(prepared.clone());
        self.unlink_idle(&connection_partition);
        self.assert_consistent(demand);
        Some(prepared)
    }

    /// Completes one reclaim crossing and repairs the exact advertisement.
    pub(super) fn finish_reclaim(
        &mut self,
        prepared: &PreparedH2Reclaim,
        snapshot: Option<H2AdvertisementSnapshot>,
        demand: &DemandSchedule,
    ) {
        assert_eq!(
            self.reclaiming.as_ref(),
            Some(prepared),
            "HTTP/2 reclaim completion did not match its reservation"
        );
        self.reclaiming = None;
        match snapshot {
            Some(snapshot) => {
                self.update(
                    prepared.connection_partition,
                    prepared.group.clone(),
                    snapshot,
                    demand,
                );
            }
            None => {
                self.unlink(&prepared.connection_partition);
                self.advertisements.remove(&prepared.connection_partition);
                self.reconcile_group(&prepared.group, demand);
            }
        }
        self.enqueue_idle_if_available(prepared.connection_partition);
        self.assert_consistent(demand);
    }

    /// Appends one accepting connection cell to its group order.
    fn enqueue(&mut self, connection_partition: PartitionId) {
        let group = self
            .advertisements
            .get(&connection_partition)
            .expect("enqueued H2 connection disappeared")
            .group
            .clone();
        let links = self
            .group_orders
            .entry(group)
            .or_default()
            .push_back(connection_partition);
        if let Some(previous) = links.previous {
            self.advertisements
                .get_mut(&previous)
                .expect("previous H2 advertisement disappeared")
                .residence
                .links_mut()
                .next = Some(connection_partition);
        }
        self.advertisements
            .get_mut(&connection_partition)
            .expect("enqueued H2 connection disappeared")
            .residence = AdvertisementResidence::Advertised { links };
    }

    /// Appends one unreserved idle generation to origin reclaim order.
    fn enqueue_idle_if_available(&mut self, connection_partition: PartitionId) {
        let reserved = self
            .reclaiming
            .as_ref()
            .is_some_and(|reclaim| reclaim.connection_partition == connection_partition);
        let Some(record) = self.advertisements.get(&connection_partition) else {
            return;
        };
        if record.generation.is_none() || !record.idle || reserved || record.idle_links.is_some() {
            return;
        }
        let links = self.idle_order.push_back(connection_partition);
        if let Some(previous) = links.previous {
            self.advertisements
                .get_mut(&previous)
                .expect("previous idle HTTP/2 advertisement disappeared")
                .idle_links
                .as_mut()
                .expect("previous idle HTTP/2 advertisement lost its links")
                .next = Some(connection_partition);
        }
        self.advertisements
            .get_mut(&connection_partition)
            .expect("enqueued idle HTTP/2 advertisement disappeared")
            .idle_links = Some(links);
    }

    /// Removes one generation from origin reclaim order.
    fn unlink_idle(&mut self, connection_partition: &PartitionId) {
        let Some(record) = self.advertisements.get_mut(connection_partition) else {
            return;
        };
        let Some(links) = record.idle_links.take() else {
            return;
        };
        if let Some(previous) = links.previous {
            self.advertisements
                .get_mut(&previous)
                .expect("previous idle HTTP/2 advertisement disappeared")
                .idle_links
                .as_mut()
                .expect("previous idle HTTP/2 advertisement lost its links")
                .next = links.next;
        }
        if let Some(next) = links.next {
            self.advertisements
                .get_mut(&next)
                .expect("next idle HTTP/2 advertisement disappeared")
                .idle_links
                .as_mut()
                .expect("next idle HTTP/2 advertisement lost its links")
                .previous = links.previous;
        }
        self.idle_order.remove(*connection_partition, links);
    }

    /// Removes one connection cell and repairs its group order.
    fn unlink(&mut self, connection_partition: &PartitionId) {
        self.unlink_idle(connection_partition);
        let Some(record) = self.advertisements.get_mut(connection_partition) else {
            return;
        };
        let residence = std::mem::take(&mut record.residence);
        let AdvertisementResidence::Advertised { links } = residence else {
            return;
        };
        let group = record.group.clone();
        if let Some(previous) = links.previous {
            self.advertisements
                .get_mut(&previous)
                .expect("previous H2 advertisement disappeared")
                .residence
                .links_mut()
                .next = links.next;
        }
        if let Some(next) = links.next {
            self.advertisements
                .get_mut(&next)
                .expect("next H2 advertisement disappeared")
                .residence
                .links_mut()
                .previous = links.previous;
        }
        let order = self
            .group_orders
            .get_mut(&group)
            .expect("advertised H2 connection lost its group order");
        order.remove(*connection_partition, links);
        if order.len() == 0 {
            self.group_orders.remove(&group);
        }
    }

    /// Returns the group head, or its successor when the requesting cell owns it.
    fn first_peer(
        &self,
        group: &EligibilityGroup,
        requesting_partition: &PartitionId,
    ) -> Option<PartitionId> {
        let head = self.group_orders.get(group)?.head()?;
        if head != *requesting_partition {
            return Some(head);
        }
        self.advertisements
            .get(&head)
            .expect("H2 advertisement head disappeared")
            .residence
            .links()
            .and_then(|links| links.next)
    }

    fn assert_consistent(&self, demand: &DemandSchedule) {
        #[cfg(not(any(debug_assertions, test)))]
        let _ = demand;

        #[cfg(any(debug_assertions, test))]
        {
            if std::thread::panicking() {
                return;
            }
            for (partition, record) in &self.advertisements {
                assert_eq!(
                    record.generation.is_some(),
                    record.residence.links().is_some(),
                    "H2 advertisement residence did not match connection availability"
                );
                if record.residence.links().is_some() {
                    assert!(
                        self.group_orders.contains_key(&record.group),
                        "H2 advertisement lost its group order"
                    );
                }
                let reserved = self
                    .reclaiming
                    .as_ref()
                    .is_some_and(|reclaim| reclaim.connection_partition == *partition);
                assert_eq!(
                    record.generation.is_some() && record.idle && !reserved,
                    record.idle_links.is_some(),
                    "H2 reclaim residence did not match idle availability"
                );
                debug_assert_ne!(
                    record.idle_links.as_ref().and_then(|links| links.next),
                    Some(*partition)
                );
                debug_assert_ne!(
                    record.residence.links().and_then(|links| links.next),
                    Some(*partition)
                );
            }

            for (group, order) in &self.group_orders {
                let expected = self
                    .advertisements
                    .values()
                    .filter(|record| record.group == *group && record.residence.links().is_some())
                    .count();
                order.assert_consistent(
                    expected,
                    self.advertisements.len(),
                    "HTTP/2 advertisement order",
                    |partition| {
                        *self
                            .advertisements
                            .get(&partition)
                            .expect("ordered H2 advertisement disappeared")
                            .residence
                            .links()
                            .expect("ordered H2 advertisement lost its links")
                    },
                );
            }

            let expected_idle = self
                .advertisements
                .values()
                .filter(|record| record.idle_links.is_some())
                .count();
            self.idle_order.assert_consistent(
                expected_idle,
                self.advertisements.len(),
                "idle HTTP/2 reclaim order",
                |partition| {
                    self.advertisements
                        .get(&partition)
                        .expect("ordered idle HTTP/2 advertisement disappeared")
                        .idle_links
                        .expect("ordered idle HTTP/2 advertisement lost its links")
                },
            );

            let expected_ready = self
                .group_orders
                .keys()
                .filter(|group| {
                    demand.queued_group_head(group).is_some_and(|queued| {
                        queued.requirement.accepts_h2()
                            && self
                                .first_peer(group, &queued.requesting_partition)
                                .is_some()
                    })
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            assert_eq!(
                expected_ready, self.ready_groups,
                "HTTP/2 publication-ready groups did not match demand and advertisements"
            );
        }
    }
}

/// Identity-only publication crossing with a terminal admission fallback.
pub(in crate::client::pool) struct H2PublicationGuard {
    /// Admission owner of the fenced demand and advertisement.
    origin: Arc<OriginAdmission>,
    /// Identities revalidated across the unlocked cell transitions.
    prepared: PreparedH2Publication,
    /// Terminal acknowledgement submitted if the crossing unwinds.
    on_drop: Option<DeliveryAckResult>,
}

impl H2PublicationGuard {
    pub(super) fn new(origin: Arc<OriginAdmission>, prepared: PreparedH2Publication) -> Self {
        Self {
            origin,
            prepared,
            on_drop: Some(DeliveryAckResult::RetrySameResidence),
        }
    }

    /// Crosses connection and requesting cells, then acknowledges the fence.
    ///
    /// A missing connection cell or stale generation withdraws that exact
    /// advertisement and retries the same demand residence. A missing
    /// requesting cell retires the demand. Requesting-cell rejection retries
    /// only when the original demand and route installation are still useful.
    pub(super) fn publish_once(self) -> Option<AdmissionAction> {
        let generation = self.prepared.generation;
        let Some(connection_cell) = self.origin.cell(&self.prepared.connection_partition) else {
            return self.finish(DeliveryAckResult::RetrySameResidence, Some(generation));
        };
        if !OriginCell::h2_generation_is_accepting(&connection_cell, self.prepared.generation) {
            return self.finish(DeliveryAckResult::RetrySameResidence, Some(generation));
        }

        let Some(requesting_cell) = self.origin.cell(&self.prepared.requesting_partition) else {
            return self.finish(DeliveryAckResult::Rejected { successor: None }, None);
        };
        let route = H2Route::new(&connection_cell, self.prepared.generation);
        if !OriginCell::install_h2_route(
            &requesting_cell,
            route,
            &self.prepared.group,
            self.prepared.demand,
        ) {
            return self.finish(DeliveryAckResult::RetrySameResidence, None);
        }

        let next = self.finish(DeliveryAckResult::Accepted { successor: None }, None);
        OriginCell::service_h2_waiters(&requesting_cell);
        OriginCell::service_peer_h2_waiters(&requesting_cell);
        next
    }

    fn finish(
        mut self,
        result: DeliveryAckResult,
        stale_generation: Option<H2GenerationId>,
    ) -> Option<AdmissionAction> {
        let outcome = match &result {
            DeliveryAckResult::Accepted { .. } => "accepted",
            DeliveryAckResult::RetrySameResidence => "retry",
            DeliveryAckResult::Rejected { .. } => "rejected",
        };
        // The unlocked cell crossing is complete. Admission owns the fence
        // again, so unwinding must not replay a failed acknowledgement.
        self.on_drop = None;
        let next = OriginAdmission::finish_h2_publication(
            &self.origin,
            &self.prepared,
            stale_generation,
            result,
        );
        self.trace(outcome);
        next
    }

    fn trace(&self, outcome: &str) {
        tracing::trace!(
            request_partition = ?self.prepared.requesting_partition,
            connection_partition = ?self.prepared.connection_partition,
            origin_scheme = %self.origin.origin().scheme(),
            origin_host = self.origin.origin().host(),
            origin_port = ?self.origin.origin().port(),
            h2_generation = ?self.prepared.generation,
            demand = ?self.prepared.demand,
            outcome,
            "HTTP/2 peer publication completed"
        );
    }
}

impl Drop for H2PublicationGuard {
    fn drop(&mut self) {
        let Some(result) = self.on_drop.take() else {
            return;
        };
        let next =
            OriginAdmission::finish_h2_publication(&self.origin, &self.prepared, None, result);
        self.trace("guard_drop");
        OriginAdmission::drive(next);
    }
}

/// Exact idle-generation crossing with an admission repair fallback.
pub(in crate::client::pool) struct H2ReclaimAction {
    /// Admission authority that owns the reclaim reservation.
    origin: Arc<OriginAdmission>,
    /// Prepared reclaim still owned by this fallback.
    prepared: Option<PreparedH2Reclaim>,
}

impl H2ReclaimAction {
    /// Creates one unlocked exact-generation reclaim crossing.
    pub(super) fn new(origin: Arc<OriginAdmission>, prepared: PreparedH2Reclaim) -> Self {
        Self {
            origin,
            prepared: Some(prepared),
        }
    }

    /// Attempts idle reclaim and returns the next bounded-origin action.
    pub(super) fn drive_once(mut self) -> Option<AdmissionAction> {
        let prepared = self
            .prepared
            .as_ref()
            .expect("HTTP/2 reclaim action consumed more than once")
            .clone();
        let (snapshot, reclaimed) = match self.origin.cell(&prepared.connection_partition) {
            Some(cell) => {
                let (snapshot, connection_id) =
                    OriginCell::reclaim_idle_h2(&cell, prepared.generation);
                (Some(snapshot), connection_id)
            }
            None => (None, None),
        };
        self.prepared = None;
        let next = OriginAdmission::finish_h2_reclaim(&self.origin, &prepared, snapshot);
        tracing::trace!(
            connection_id = ?reclaimed,
            request_partition = ?prepared.requesting_partition,
            connection_partition = ?prepared.connection_partition,
            origin_scheme = %self.origin.origin().scheme(),
            origin_host = self.origin.origin().host(),
            origin_port = ?self.origin.origin().port(),
            h2_generation = ?prepared.generation,
            demand = ?prepared.demand,
            outcome = if reclaimed.is_some() { "reclaimed" } else { "rejected" },
            "HTTP/2 idle reclaim completed"
        );
        next
    }
}

impl Drop for H2ReclaimAction {
    fn drop(&mut self) {
        let Some(prepared) = self.prepared.take() else {
            return;
        };
        let snapshot = self
            .origin
            .cell(&prepared.connection_partition)
            .map(|cell| cell.report_h2_advertisement());
        let next = OriginAdmission::finish_h2_reclaim(&self.origin, &prepared, snapshot);
        tracing::trace!(
            request_partition = ?prepared.requesting_partition,
            connection_partition = ?prepared.connection_partition,
            origin_scheme = %self.origin.origin().scheme(),
            origin_host = self.origin.origin().host(),
            origin_port = ?self.origin.origin().port(),
            h2_generation = ?prepared.generation,
            demand = ?prepared.demand,
            outcome = "guard_drop",
            "HTTP/2 idle reclaim completed"
        );
        OriginAdmission::drive(next);
    }
}
#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::{DemandSnapshot, ProtocolRequirement, SnapshotVersion};

    fn partition(index: usize) -> PartitionId {
        PartitionId::from_index(index)
    }

    fn generation(value: u64) -> H2GenerationId {
        H2GenerationId::for_test(value)
    }

    fn demand(id: u64, group: EligibilityGroup) -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(id),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H2Required,
            group,
        )
    }

    fn h1_demand(id: u64, group: EligibilityGroup) -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(id),
            SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Required,
            group,
        )
    }

    #[test]
    fn exact_generation_replacement_rejects_stale_removal() {
        let group = EligibilityGroup::Pool;
        let connection_partition = partition(1);
        let requesting_partition = partition(2);
        let first = generation(10);
        let second = generation(11);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group.clone(),
            H2AdvertisementSnapshot::accepting(1, first),
            &schedule,
        );
        publication.update(
            connection_partition,
            group.clone(),
            H2AdvertisementSnapshot::accepting(2, second),
            &schedule,
        );

        publication.remove_if_exact(&connection_partition, first, &schedule);
        let prepared = publication
            .prepare(&mut schedule, DeliveryId(1))
            .expect("newer advertisement should remain publishable");
        assert_eq!(second, prepared.generation);
        assert_eq!(connection_partition, prepared.connection_partition);
        assert_eq!(requesting_partition, prepared.requesting_partition);
    }

    #[test]
    fn current_generation_removal_repairs_the_connection_index() {
        let group = EligibilityGroup::Pool;
        let connection_partition = partition(1);
        let requesting_partition = partition(2);
        let current = generation(10);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::accepting(1, current),
            &schedule,
        );

        publication.remove_if_exact(&connection_partition, current, &schedule);

        assert!(!publication.has_ready_group());
        assert!(publication.group_orders.is_empty());
        assert!(matches!(
            publication.advertisements[&connection_partition].residence,
            AdvertisementResidence::Unavailable
        ));
    }

    #[test]
    fn publication_skips_the_requesting_cell_and_selects_a_peer_in_its_group() {
        let pool = EligibilityGroup::Pool;
        let isolated = EligibilityGroup::Partition(partition(3));
        let requesting_partition = partition(1);
        let peer = partition(2);
        let other_group = partition(3);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, demand(1, pool.clone()));
        schedule.publish(other_group, demand(2, isolated.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            requesting_partition,
            pool.clone(),
            H2AdvertisementSnapshot::accepting(1, generation(1)),
            &schedule,
        );
        publication.update(
            peer,
            pool,
            H2AdvertisementSnapshot::accepting(1, generation(2)),
            &schedule,
        );
        publication.update(
            other_group,
            isolated,
            H2AdvertisementSnapshot::accepting(1, generation(3)),
            &schedule,
        );

        let prepared = publication
            .prepare(&mut schedule, DeliveryId(1))
            .expect("pool demand should find its peer connection");
        assert_eq!(requesting_partition, prepared.requesting_partition);
        assert_eq!(peer, prepared.connection_partition);
        assert_eq!(generation(2), prepared.generation);
    }

    #[test]
    fn older_advertisement_cannot_replace_the_current_generation() {
        let group = EligibilityGroup::Pool;
        let connection_partition = partition(1);
        let requesting_partition = partition(2);
        let current = generation(2);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group.clone(),
            H2AdvertisementSnapshot::accepting(2, current),
            &schedule,
        );
        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::accepting(1, generation(1)),
            &schedule,
        );

        let prepared = publication
            .prepare(&mut schedule, DeliveryId(1))
            .expect("current advertisement should remain publishable");
        assert_eq!(current, prepared.generation);
    }

    #[test]
    fn h1_required_demand_is_not_publication_ready() {
        let group = EligibilityGroup::Pool;
        let requesting_partition = partition(1);
        let connection_partition = partition(2);
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            requesting_partition,
            DemandSnapshot::active(
                DemandId::from_u64(1),
                SnapshotVersion::INITIAL,
                ProtocolRequirement::H1Required,
                group.clone(),
            ),
        );
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::accepting(1, generation(1)),
            &schedule,
        );

        assert!(!publication.has_ready_group());
        assert!(publication.prepare(&mut schedule, DeliveryId(1)).is_none());
    }

    #[test]
    fn publication_turns_rotate_across_ready_groups() {
        let first_group = EligibilityGroup::Partition(partition(10));
        let second_group = EligibilityGroup::Partition(partition(20));
        let first_request = partition(1);
        let first_connection = partition(2);
        let second_request = partition(3);
        let second_connection = partition(4);
        let mut schedule = DemandSchedule::default();
        schedule.publish(first_request, demand(1, first_group.clone()));
        schedule.publish(second_request, demand(2, second_group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            first_connection,
            first_group.clone(),
            H2AdvertisementSnapshot::accepting(1, generation(1)),
            &schedule,
        );
        publication.update(
            second_connection,
            second_group.clone(),
            H2AdvertisementSnapshot::accepting(1, generation(2)),
            &schedule,
        );

        let first = publication
            .prepare(&mut schedule, DeliveryId(1))
            .expect("first ready group was not selected");
        schedule.finish_delivery(
            first.delivery,
            &first.requesting_partition,
            DeliveryAckResult::RetrySameResidence,
        );
        publication.reconcile_group(&first.group, &schedule);

        let second = publication
            .prepare(&mut schedule, DeliveryId(2))
            .expect("second ready group was not selected");
        assert_ne!(first.group, second.group);
    }

    #[test]
    #[should_panic(
        expected = "HTTP/2 publication-ready groups did not match demand and advertisements"
    )]
    fn consistency_check_rejects_a_missing_ready_group() {
        let group = EligibilityGroup::Pool;
        let connection_partition = partition(1);
        let requesting_partition = partition(2);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::accepting(1, generation(1)),
            &schedule,
        );

        publication.ready_groups.clear();

        publication.assert_consistent(&schedule);
    }

    #[test]
    fn only_idle_h2_is_selected_for_h1_required_reclaim() {
        let group = EligibilityGroup::Pool;
        let requesting_partition = partition(1);
        let connection_partition = partition(2);
        let current = generation(10);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, h1_demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group.clone(),
            H2AdvertisementSnapshot::accepting(1, current),
            &schedule,
        );

        assert!(
            publication.prepare_reclaim(&schedule).is_none(),
            "busy HTTP/2 generation entered reclaim order"
        );

        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::idle(2, current),
            &schedule,
        );
        let prepared = publication
            .prepare_reclaim(&schedule)
            .expect("idle HTTP/2 generation was not selected");
        assert_eq!(requesting_partition, prepared.requesting_partition);
        assert_eq!(connection_partition, prepared.connection_partition);
        assert_eq!(current, prepared.generation);
        assert!(publication.idle_order.head().is_none());

        publication.finish_reclaim(
            &prepared,
            Some(H2AdvertisementSnapshot::idle(2, current)),
            &schedule,
        );
        assert_eq!(Some(connection_partition), publication.idle_order.head());
    }

    #[test]
    fn reclaim_completion_cannot_restore_a_replaced_generation() {
        let group = EligibilityGroup::Pool;
        let requesting_partition = partition(1);
        let connection_partition = partition(2);
        let old = generation(10);
        let replacement = generation(11);
        let mut schedule = DemandSchedule::default();
        schedule.publish(requesting_partition, h1_demand(1, group.clone()));
        let mut publication = H2Publication::default();
        publication.update(
            connection_partition,
            group.clone(),
            H2AdvertisementSnapshot::idle(1, old),
            &schedule,
        );
        let prepared = publication
            .prepare_reclaim(&schedule)
            .expect("idle HTTP/2 generation was not selected");

        publication.update(
            connection_partition,
            group,
            H2AdvertisementSnapshot::idle(2, replacement),
            &schedule,
        );
        publication.finish_reclaim(
            &prepared,
            Some(H2AdvertisementSnapshot::idle(1, old)),
            &schedule,
        );

        let next = publication
            .prepare_reclaim(&schedule)
            .expect("replacement generation was not restored to reclaim order");
        assert_eq!(replacement, next.generation);
    }
}

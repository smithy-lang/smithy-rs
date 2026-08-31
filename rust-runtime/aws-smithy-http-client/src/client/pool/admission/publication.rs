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
//! ```
//!
//! The connection-owning and requesting cell locks never nest.
//! [`H2PublicationGuard`] owns the admission fence between those scopes and
//! submits a terminal acknowledgement on drop.

use super::{
    AdmissionAction, DeliveryAckResult, DeliveryId, DemandId, DemandSchedule, IntrusiveLinks,
    IntrusiveOrder, OriginAdmission,
};
use crate::client::pool::cell::h2::{H2GenerationId, H2Route};
use crate::client::pool::cell::OriginCell;
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use crate::sync::Arc;
use std::collections::{BTreeSet, HashMap};

/// Complete connection-cell advertisement at one monotonic revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool) struct H2AdvertisementSnapshot {
    revision: u64,
    generation: Option<H2GenerationId>,
}

impl H2AdvertisementSnapshot {
    /// Reports one accepting generation.
    pub(in crate::client::pool) fn accepting(revision: u64, generation: H2GenerationId) -> Self {
        Self {
            revision,
            generation: Some(generation),
        }
    }

    /// Reports that the connection cell has no accepting generation.
    pub(in crate::client::pool) fn unavailable(revision: u64) -> Self {
        Self {
            revision,
            generation: None,
        }
    }
}

/// Admission-owned H2 advertisements and publication-ready groups.
///
/// At every completed transition:
///
/// - each connection cell has at most one retained advertisement record;
/// - each advertised connection cell is linked once in its eligibility-group order;
/// - each linked identity names the newest reported generation;
/// - `ready_groups` contains exactly the groups with queued demand and an
///   advertised peer connection; and
/// - neither advertisements nor prepared publications own connection
///   capacity or a protocol sender.
#[derive(Debug, Default)]
pub(super) struct H2Publication {
    advertisements: HashMap<PartitionId, AdvertisementRecord>,
    group_orders: HashMap<EligibilityGroup, IntrusiveOrder<PartitionId>>,
    ready_groups: BTreeSet<EligibilityGroup>,
}

/// Admission's latest complete report for one connection cell.
#[derive(Debug)]
struct AdvertisementRecord {
    group: EligibilityGroup,
    revision: u64,
    generation: Option<H2GenerationId>,
    residence: AdvertisementResidence,
}

/// Whether one connection cell occupies its advertisement order.
#[derive(Debug, Default)]
enum AdvertisementResidence {
    #[default]
    Unavailable,
    Advertised {
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
    pub(super) delivery: DeliveryId,
    pub(super) requesting_partition: PartitionId,
    pub(super) demand: DemandId,
    pub(super) connection_partition: PartitionId,
    pub(super) generation: H2GenerationId,
    pub(super) group: EligibilityGroup,
}

/// Candidate selected from stored demand and advertisement heads.
struct PublicationCandidate {
    requesting_partition: PartitionId,
    demand: DemandId,
    connection_partition: PartitionId,
    generation: H2GenerationId,
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
            });
        record.group = group.clone();
        record.revision = snapshot.revision;
        record.generation = snapshot.generation;
        if record.generation.is_some() {
            self.enqueue(connection_partition);
        }

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
        self.advertisements
            .get_mut(connection_partition)
            .expect("validated H2 advertisement disappeared")
            .generation = None;
        self.reconcile_group(&group, demand);
        self.assert_consistent(demand);
    }

    /// Recomputes whether one group can start a publication.
    pub(super) fn reconcile_group(&mut self, group: &EligibilityGroup, demand: &DemandSchedule) {
        let ready = demand
            .queued_group_head(group)
            .and_then(|queued| {
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
        let group = self.ready_groups.first()?.clone();
        let Some(queued) = demand.queued_group_head(&group) else {
            self.ready_groups.remove(&group);
            return None;
        };
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

    /// Removes one connection cell and repairs its group order.
    fn unlink(&mut self, connection_partition: &PartitionId) {
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

            let expected_ready = self
                .group_orders
                .keys()
                .filter(|group| {
                    demand.queued_group_head(group).is_some_and(|queued| {
                        self.first_peer(group, &queued.requesting_partition)
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
    origin: Arc<OriginAdmission>,
    prepared: PreparedH2Publication,
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

    /// Validates the connection cell, installs requesting-cell visibility, and acknowledges it.
    pub(super) fn publish_once(mut self) -> Option<AdmissionAction> {
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

        // Visibility and the requesting cell's suppressed demand are authoritative
        // before a drop may acknowledge acceptance.
        self.on_drop = Some(DeliveryAckResult::Accepted { successor: None });
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
        self.on_drop = None;
        let outcome = match &result {
            DeliveryAckResult::Accepted { .. } => "accepted",
            DeliveryAckResult::RetrySameResidence => "retry",
            DeliveryAckResult::Rejected { .. } => "rejected",
        };
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
    fn publication_uses_a_peer_connection_from_the_same_group() {
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
}

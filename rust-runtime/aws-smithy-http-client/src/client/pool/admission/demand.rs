/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Versioned cross-cell demand scheduling for one bounded origin.
//!
//! Each cell publishes at most one active demand: its acquisition queue's
//! current head. That demand may be satisfied either by origin-wide connection
//! capacity or by an existing HTTP/2 connection in its eligibility group.
//! These resources require different orderings.
//!
//! For example, the origin order may contain `A(group X), B(group Y)` while
//! only group Y has a reusable HTTP/2 connection. A remains first for the next
//! available connection permit, while the group-Y connection can serve B
//! without consuming that permit or delaying A. The group order finds B
//! directly instead of scanning every partition.
//!
//! The same demand occupies both orders because capacity and HTTP/2 publication
//! can race to satisfy it. Reserving either position retains a fence in both
//! orders until the requesting cell accepts or rejects the handoff. The other
//! resource therefore cannot serve the same demand while the first handoff is
//! running outside the admission lock.

use super::{
    DeliveryAckResult, DeliveryId, DemandId, DemandSnapshot, DemandState, IntrusiveLinks,
    IntrusiveOrder, Permit, ProtocolRequirement,
};
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use std::collections::HashMap;

/// Cross-cell demand records and their scheduling orders.
///
/// At every completed transition:
///
/// - `records` owns the newest snapshot for every retained cell.
/// - the origin order and the record's eligibility-group order contain exactly
///   the `Queued` and `Delivering` records;
/// - links for both views live inside those ordered residence variants;
/// - an origin delivery fences the origin head; and
/// - an HTTP/2 publication fences its eligibility-group head.
///
/// Admission coordinates capacity extraction with this schedule while holding
/// the same origin lock.
#[derive(Debug, Default)]
pub(super) struct DemandSchedule {
    /// Latest demand and scheduling residence for each retained cell.
    records: HashMap<PartitionId, DemandRecord>,
    /// Origin-wide order used by one-to-one capacity and HTTP/1 reuse.
    origin_order: IntrusiveOrder<PartitionId>,
    /// Reusable-HTTP/2 demand order for each connection-reuse group.
    group_orders: HashMap<EligibilityGroup, IntrusiveOrder<PartitionId>>,
}

/// Complete origin-order head used to choose one HTTP/1 reuse action.
#[derive(Clone, Debug)]
pub(super) struct QueuedDemand {
    /// Cell whose oldest waiter owns this demand generation.
    pub(super) requesting_partition: PartitionId,
    /// Demand identity revalidated at each crossing.
    pub(super) demand: DemandId,
    /// Protocol capability required by the requesting cell waiter.
    pub(super) requirement: ProtocolRequirement,
    /// Connection-owning cells whose H1 senders may satisfy this demand.
    pub(super) eligibility_group: EligibilityGroup,
}

/// Latest snapshot and scheduling residence for one stable cell.
#[derive(Debug)]
struct DemandRecord {
    /// Newest complete publication observed for the cell.
    pub(super) latest: DemandSnapshot,
    /// Stable group retained while an inactive replacement crosses a fence.
    group: Option<EligibilityGroup>,
    /// Scheduling residence, including links while ordered.
    residence: DemandResidence,
}

/// Links retained by one demand in both scheduling views.
#[derive(Clone, Debug)]
struct DemandLinks {
    /// Position in origin-wide capacity and HTTP/1 order.
    origin: IntrusiveLinks<PartitionId>,
    /// Position in the demand's all-protocol eligibility-group order.
    group: IntrusiveLinks<PartitionId>,
}

/// Order whose head is fenced by one delivery crossing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeliveryView {
    /// Capacity or HTTP/1 delivery selected from origin order.
    Origin,
    /// HTTP/2 publication selected from eligibility-group order.
    Group,
}

/// Selects one of a demand residence's intrusive link sets.
#[derive(Clone, Copy)]
enum DemandOrderView {
    /// Origin-wide capacity and HTTP/1 order.
    Origin,
    /// Eligibility-group all-protocol order.
    Group,
}

/// Admission residence for one partition's latest demand.
///
/// ```text
/// Idle -- active publication ------------------------------> Queued
/// Queued -- reserve head ----------------------------------> Delivering
/// Queued -- inactive or replacement publication ----------> Idle
/// Delivering -- retry unchanged demand --------------------> Queued
/// Delivering -- accept, reject, or replacement retry ------> Idle
/// Idle -- active successor remains ------------------------> Queued
/// ```
#[derive(Clone, Debug)]
enum DemandResidence {
    /// The cell has no demand represented in scheduling.
    Idle,
    /// The demand is waiting in origin order.
    Queued {
        /// Demand generation represented by this residence.
        demand: DemandId,
        /// Origin-wide and eligibility-group scheduling links.
        links: DemandLinks,
    },
    /// One delivery or publication guard fences this demand.
    Delivering {
        /// Demand generation fenced at one scheduling head.
        demand: DemandId,
        /// Snapshot version current when admission created the fence.
        version: super::SnapshotVersion,
        /// Delivery allowed to complete this fence.
        delivery: DeliveryId,
        /// Scheduling view whose head owns this crossing.
        #[cfg_attr(
            not(debug_assertions),
            allow(
                dead_code,
                reason = "the scheduling view is retained for debug invariant checks"
            )
        )]
        view: DeliveryView,
        /// Both scheduling positions retained until acknowledgement.
        links: DemandLinks,
    },
}

impl DemandResidence {
    #[cfg(any(debug_assertions, test))]
    /// Borrows one view's links while this record is ordered.
    fn links(&self, view: DemandOrderView) -> Option<&IntrusiveLinks<PartitionId>> {
        let links = match self {
            Self::Idle => return None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => links,
        };
        match view {
            DemandOrderView::Origin => Some(&links.origin),
            DemandOrderView::Group => Some(&links.group),
        }
    }

    /// Mutably borrows one view's links from an ordered record.
    ///
    /// # Panics
    ///
    /// Panics when called for an idle record.
    fn links_mut(&mut self, view: DemandOrderView) -> &mut IntrusiveLinks<PartitionId> {
        let links = match self {
            Self::Idle => panic!("idle demand has no scheduling links"),
            Self::Queued { links, .. } | Self::Delivering { links, .. } => links,
        };
        match view {
            DemandOrderView::Origin => &mut links.origin,
            DemandOrderView::Group => &mut links.group,
        }
    }

    /// Detaches both link sets while moving a record out of scheduling.
    fn into_links(self) -> Option<DemandLinks> {
        match self {
            Self::Idle => None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => Some(links),
        }
    }
}

/// Demand reserved at the order head for one capacity delivery.
pub(super) struct ScheduledDemand {
    /// Selected destination cell.
    pub(super) requesting_partition: PartitionId,
    /// Demand identity current when the crossing was prepared.
    pub(super) demand: DemandId,
}

/// Connection capacity extracted under admission lock before crossing to a cell.
#[derive(Debug)]
pub(super) struct PreparedCapacityDelivery {
    /// Permit transferred into the delivery guard.
    pub(super) permit: Permit,
    /// Fence identity allocated for this crossing.
    pub(super) delivery: DeliveryId,
    /// Selected destination cell.
    pub(super) requesting_partition: PartitionId,
    /// Demand identity current when the crossing was prepared.
    pub(super) demand: DemandId,
}

impl DemandSchedule {
    /// Applies a complete snapshot and updates its cell's scheduling residence.
    pub(super) fn publish(&mut self, requesting_partition: PartitionId, snapshot: DemandSnapshot) {
        if let Some(current) = self.records.get(&requesting_partition) {
            if !snapshot.is_newer_than(&current.latest) {
                return;
            }
        } else {
            self.records.insert(
                requesting_partition,
                DemandRecord {
                    latest: snapshot.clone(),
                    group: None,
                    residence: DemandResidence::Idle,
                },
            );
        }

        let should_remove = matches!(
            &self
                .records
                .get(&requesting_partition)
                .expect("published demand record disappeared")
                .residence,
            DemandResidence::Queued { demand, .. }
                if *demand != snapshot.id || !snapshot.is_active()
        );
        if should_remove {
            self.remove_from_order(&requesting_partition);
        }

        if let DemandState::Active {
            eligibility_group, ..
        } = &snapshot.state
        {
            self.records
                .get_mut(&requesting_partition)
                .expect("published demand record disappeared")
                .group = Some(eligibility_group.clone());
        }
        self.records
            .get_mut(&requesting_partition)
            .expect("published demand record disappeared")
            .latest = snapshot;

        let record = self
            .records
            .get(&requesting_partition)
            .expect("published demand record disappeared");
        if record.latest.is_active() && matches!(&record.residence, DemandResidence::Idle) {
            self.enqueue(requesting_partition);
        }
        self.assert_consistent();
    }

    /// Appends an idle active demand to both scheduling orders.
    fn enqueue(&mut self, requesting_partition: PartitionId) {
        let group = self
            .group_for(&requesting_partition)
            .expect("enqueued demand was inactive");
        let origin = self.origin_order.push_back(requesting_partition);
        let group_links = self
            .group_orders
            .entry(group)
            .or_default()
            .push_back(requesting_partition);
        if let Some(previous) = origin.previous {
            self.records
                .get_mut(&previous)
                .expect("origin demand tail disappeared")
                .residence
                .links_mut(DemandOrderView::Origin)
                .next = Some(requesting_partition);
        }
        if let Some(previous) = group_links.previous {
            self.records
                .get_mut(&previous)
                .expect("group demand tail disappeared")
                .residence
                .links_mut(DemandOrderView::Group)
                .next = Some(requesting_partition);
        }

        let record = self
            .records
            .get_mut(&requesting_partition)
            .expect("queued demand record disappeared");
        debug_assert!(matches!(record.residence, DemandResidence::Idle));
        let demand = record.latest.id;
        record.residence = DemandResidence::Queued {
            demand,
            links: DemandLinks {
                origin,
                group: group_links,
            },
        };
    }

    /// Removes an ordered demand from both views and leaves its record idle.
    fn remove_from_order(&mut self, requesting_partition: &PartitionId) {
        let group = self
            .group_for(requesting_partition)
            .expect("removed demand had no eligibility group");
        let residence = {
            let record = self
                .records
                .get_mut(requesting_partition)
                .expect("removed demand record disappeared");
            std::mem::replace(&mut record.residence, DemandResidence::Idle)
        };
        let links = residence
            .into_links()
            .expect("removed demand had no scheduling links");

        if let Some(previous) = links.origin.previous {
            self.records
                .get_mut(&previous)
                .expect("previous origin demand disappeared")
                .residence
                .links_mut(DemandOrderView::Origin)
                .next = links.origin.next;
        }
        if let Some(next) = links.origin.next {
            self.records
                .get_mut(&next)
                .expect("next origin demand disappeared")
                .residence
                .links_mut(DemandOrderView::Origin)
                .previous = links.origin.previous;
        }
        self.origin_order
            .remove(*requesting_partition, links.origin);

        if let Some(previous) = links.group.previous {
            self.records
                .get_mut(&previous)
                .expect("previous group demand disappeared")
                .residence
                .links_mut(DemandOrderView::Group)
                .next = links.group.next;
        }
        if let Some(next) = links.group.next {
            self.records
                .get_mut(&next)
                .expect("next group demand disappeared")
                .residence
                .links_mut(DemandOrderView::Group)
                .previous = links.group.previous;
        }
        let order = self
            .group_orders
            .get_mut(&group)
            .expect("ordered demand lost its eligibility-group order");
        order.remove(*requesting_partition, links.group);
        if order.len() == 0 {
            self.group_orders.remove(&group);
        }
    }

    /// Returns whether the head can begin a new one-to-one delivery.
    pub(super) fn head_is_queued(&self) -> bool {
        let Some(head) = self.origin_order.head() else {
            return false;
        };
        matches!(
            &self
                .records
                .get(&head)
                .expect("order head disappeared")
                .residence,
            DemandResidence::Queued { .. }
        )
    }

    /// Returns the complete origin-order head when it may begin reuse.
    pub(super) fn queued_head(&self) -> Option<QueuedDemand> {
        let head = self.origin_order.head()?;
        let record = self.records.get(&head).expect("order head disappeared");
        let DemandResidence::Queued { demand, .. } = &record.residence else {
            return None;
        };
        let DemandState::Active {
            head: requirement,
            eligibility_group,
        } = &record.latest.state
        else {
            unreachable!("queued demand became inactive");
        };
        Some(QueuedDemand {
            requesting_partition: head,
            demand: *demand,
            requirement: *requirement,
            eligibility_group: eligibility_group.clone(),
        })
    }

    /// Returns one eligibility-group head when it may receive H2 visibility.
    pub(super) fn queued_group_head(&self, group: &EligibilityGroup) -> Option<QueuedDemand> {
        let head = self.group_orders.get(group)?.head()?;
        let record = self
            .records
            .get(&head)
            .expect("group demand head disappeared");
        let DemandResidence::Queued { demand, .. } = &record.residence else {
            return None;
        };
        let DemandState::Active {
            head: requirement,
            eligibility_group,
        } = &record.latest.state
        else {
            unreachable!("queued group demand became inactive");
        };
        debug_assert_eq!(eligibility_group, group);
        Some(QueuedDemand {
            requesting_partition: head,
            demand: *demand,
            requirement: *requirement,
            eligibility_group: eligibility_group.clone(),
        })
    }

    /// Returns the latest eligibility group retained for one cell.
    pub(super) fn group_for(&self, requesting_partition: &PartitionId) -> Option<EligibilityGroup> {
        self.records
            .get(requesting_partition)
            .and_then(|record| record.group.clone())
    }

    /// Returns whether `requesting_partition` still has this demand queued for a new action.
    pub(super) fn is_current_queued(
        &self,
        requesting_partition: &PartitionId,
        demand: DemandId,
    ) -> bool {
        self.records
            .get(requesting_partition)
            .is_some_and(|record| {
                record.latest.id == demand
                    && record.latest.is_active()
                    && matches!(
                        record.residence,
                        DemandResidence::Queued {
                            demand: current,
                            ..
                        } if current == demand
                    )
            })
    }

    /// Fences one reuse operation's demand while it remains the origin-order head.
    pub(super) fn reserve_reuse_demand(
        &mut self,
        requesting_partition: &PartitionId,
        demand: DemandId,
        delivery: DeliveryId,
    ) -> Option<ScheduledDemand> {
        if !self.is_current_queued(requesting_partition, demand) {
            return None;
        }
        let head = self.origin_order.head()?;
        if head != *requesting_partition {
            return None;
        }
        self.reserve_origin_head(delivery)
    }

    /// Fences one eligibility-group head for an H2 publication.
    pub(super) fn reserve_group_head(
        &mut self,
        group: &EligibilityGroup,
        requesting_partition: &PartitionId,
        demand: DemandId,
        delivery: DeliveryId,
    ) -> Option<ScheduledDemand> {
        if !self.is_current_queued(requesting_partition, demand)
            || self.group_orders.get(group)?.head() != Some(*requesting_partition)
        {
            return None;
        }
        self.reserve(*requesting_partition, delivery, DeliveryView::Group)
    }

    /// Changes the origin head into a delivery fence at the same positions.
    pub(super) fn reserve_origin_head(&mut self, delivery: DeliveryId) -> Option<ScheduledDemand> {
        let head = self.origin_order.head()?;
        self.reserve(head, delivery, DeliveryView::Origin)
    }

    /// Changes one queued demand into a delivery fence.
    fn reserve(
        &mut self,
        requesting_partition: PartitionId,
        delivery: DeliveryId,
        view: DeliveryView,
    ) -> Option<ScheduledDemand> {
        let record = self
            .records
            .get_mut(&requesting_partition)
            .expect("order head disappeared");
        let residence = std::mem::replace(&mut record.residence, DemandResidence::Idle);
        match residence {
            DemandResidence::Queued { demand, links } => {
                debug_assert_eq!(record.latest.id, demand);
                debug_assert!(record.latest.is_active());
                record.residence = DemandResidence::Delivering {
                    demand,
                    version: record.latest.version,
                    delivery,
                    view,
                    links,
                };
                self.assert_consistent();
                Some(ScheduledDemand {
                    requesting_partition,
                    demand,
                })
            }
            residence => {
                record.residence = residence;
                None
            }
        }
    }

    /// Returns whether the test-observed delivery fence still names this demand.
    ///
    /// Production reservation revalidates the demand identity through
    /// `AcquisitionQueue::reserve_delivery_waiter`; this helper only exposes
    /// admission-side fence state to focused tests.
    #[cfg(test)]
    pub(super) fn delivery_is_current(
        &self,
        delivery: DeliveryId,
        requesting_partition: &PartitionId,
        demand: DemandId,
    ) -> bool {
        let Some(record) = self.records.get(requesting_partition) else {
            return false;
        };
        matches!(
            &record.residence,
            DemandResidence::Delivering {
                demand: current_demand,
                delivery: current_delivery,
                ..
            } if *current_delivery == delivery
                && *current_demand == demand
                && record.latest.id == demand
                && record.latest.is_active()
        )
    }

    /// Closes one delivery fence and resolves the demand's next residence.
    ///
    /// `Accepted` consumes the delivered generation and installs a newer
    /// successor when the requesting cell supplied one. `RetrySameResidence`
    /// restores the same generation at its existing order position when it
    /// remains live; if it was replaced, the latest active generation is
    /// appended as new demand. `Rejected` removes the fenced generation after
    /// its permit has been refunnelled and applies the same successor
    /// arbitration as acceptance.
    pub(super) fn finish_delivery(
        &mut self,
        delivery: DeliveryId,
        requesting_partition: &PartitionId,
        result: DeliveryAckResult,
    ) {
        let Some(record) = self.records.get(requesting_partition) else {
            return;
        };
        let delivered_demand = match &record.residence {
            DemandResidence::Delivering {
                demand,
                version,
                delivery: current,
                ..
            } if *current == delivery => (*demand, *version),
            _ => return,
        };
        let (delivered_demand, delivered_version) = delivered_demand;

        match result {
            DeliveryAckResult::RetrySameResidence => {
                let record = self
                    .records
                    .get(requesting_partition)
                    .expect("delivery demand record disappeared");
                if record.latest.id == delivered_demand && record.latest.is_active() {
                    let record = self
                        .records
                        .get_mut(requesting_partition)
                        .expect("delivery demand record disappeared");
                    let residence = std::mem::replace(&mut record.residence, DemandResidence::Idle);
                    let DemandResidence::Delivering {
                        demand,
                        version: _,
                        delivery: current,
                        view: _,
                        links,
                    } = residence
                    else {
                        unreachable!("delivery fence disappeared");
                    };
                    debug_assert_eq!(current, delivery);
                    record.residence = DemandResidence::Queued { demand, links };
                    self.assert_consistent();
                    return;
                }

                self.remove_from_order(requesting_partition);
                if self
                    .records
                    .get(requesting_partition)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(*requesting_partition);
                }
            }
            DeliveryAckResult::Accepted { successor }
            | DeliveryAckResult::Rejected { successor } => {
                self.remove_from_order(requesting_partition);

                let install_successor = successor.as_ref().is_some_and(|successor| {
                    successor.id > delivered_demand
                        && successor.is_newer_than(
                            &self
                                .records
                                .get(requesting_partition)
                                .expect("delivery demand record disappeared")
                                .latest,
                        )
                });
                if install_successor {
                    self.records
                        .get_mut(requesting_partition)
                        .expect("delivery demand record disappeared")
                        .latest = successor.expect("validated successor disappeared");
                } else {
                    let retirement =
                        DemandSnapshot::inactive(delivered_demand, delivered_version.next());
                    let record = self
                        .records
                        .get_mut(requesting_partition)
                        .expect("delivery demand record disappeared");
                    if retirement.is_newer_than(&record.latest) {
                        record.latest = retirement;
                    }
                }

                if self
                    .records
                    .get(requesting_partition)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(*requesting_partition);
                }
            }
        }
        self.assert_consistent();
    }

    /// Returns the latest complete snapshot retained for `requesting_partition`.
    #[cfg(test)]
    pub(super) fn latest_for_test(
        &self,
        requesting_partition: &PartitionId,
    ) -> Option<&DemandSnapshot> {
        self.records
            .get(requesting_partition)
            .map(|record| &record.latest)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.origin_order.len()
    }

    #[cfg(test)]
    pub(super) fn queued_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| matches!(&record.residence, DemandResidence::Queued { .. }))
            .count()
    }

    #[cfg(test)]
    pub(super) fn delivering_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| matches!(&record.residence, DemandResidence::Delivering { .. }))
            .count()
    }

    /// Checks residence, link, length, group, and fence relationships.
    fn assert_consistent(&self) {
        #[cfg(any(debug_assertions, test))]
        {
            if std::thread::panicking() {
                return;
            }
            self.assert_consistent_debug();
        }
    }

    #[cfg(any(debug_assertions, test))]
    fn assert_consistent_debug(&self) {
        let ordered_records = self
            .records
            .values()
            .filter(|record| record.residence.links(DemandOrderView::Origin).is_some())
            .count();
        let origin_deliveries = self
            .records
            .values()
            .filter(|record| {
                matches!(
                    record.residence,
                    DemandResidence::Delivering {
                        view: DeliveryView::Origin,
                        ..
                    }
                )
            })
            .count();
        assert!(
            origin_deliveries <= 1,
            "more than one origin delivery fence was active"
        );
        self.origin_order.assert_consistent(
            ordered_records,
            self.records.len(),
            "origin demand order",
            |requesting_partition| {
                let record = self
                    .records
                    .get(&requesting_partition)
                    .expect("origin-ordered demand disappeared");
                match &record.residence {
                    DemandResidence::Idle => {
                        unreachable!("origin-ordered demand became idle")
                    }
                    DemandResidence::Queued { demand, .. } => {
                        assert!(record.latest.is_active(), "queued demand became inactive");
                        assert_eq!(
                            record.latest.id, *demand,
                            "queued residence did not match its latest demand"
                        );
                    }
                    DemandResidence::Delivering { demand, view, .. } => {
                        if *view == DeliveryView::Origin {
                            assert_eq!(
                                Some(requesting_partition),
                                self.origin_order.head(),
                                "origin delivery fence moved away from its head"
                            );
                        }
                        assert!(
                            record.latest.id >= *demand,
                            "delivery fence named a future demand"
                        );
                    }
                }
                *record
                    .residence
                    .links(DemandOrderView::Origin)
                    .expect("origin-ordered demand lost its links")
            },
        );

        let group_ordered_records = self
            .records
            .values()
            .filter(|record| record.residence.links(DemandOrderView::Group).is_some())
            .count();
        for record in self
            .records
            .values()
            .filter(|record| record.residence.links(DemandOrderView::Group).is_some())
        {
            let group = record
                .group
                .as_ref()
                .expect("group-ordered demand lost its eligibility group");
            assert!(
                self.group_orders.contains_key(group),
                "ordered demand lost its eligibility-group order"
            );
        }
        assert_eq!(
            group_ordered_records,
            self.group_orders
                .values()
                .map(IntrusiveOrder::len)
                .sum::<usize>(),
            "eligibility-group orders did not contain every ordered demand"
        );

        for (group, order) in &self.group_orders {
            let group_deliveries = self
                .records
                .values()
                .filter(|record| {
                    record.group.as_ref() == Some(group)
                        && matches!(
                            record.residence,
                            DemandResidence::Delivering {
                                view: DeliveryView::Group,
                                ..
                            }
                        )
                })
                .count();
            assert!(
                group_deliveries <= 1,
                "more than one group delivery fence was active"
            );
            let expected = self
                .records
                .values()
                .filter(|record| {
                    record.residence.links(DemandOrderView::Group).is_some()
                        && record.group.as_ref() == Some(group)
                })
                .count();
            order.assert_consistent(
                expected,
                self.records.len(),
                "eligibility-group demand order",
                |requesting_partition| {
                    let record = self
                        .records
                        .get(&requesting_partition)
                        .expect("group-ordered demand disappeared");
                    assert_eq!(
                        record.group.as_ref(),
                        Some(group),
                        "demand occupied the wrong eligibility-group order"
                    );
                    if matches!(
                        record.residence,
                        DemandResidence::Delivering {
                            view: DeliveryView::Group,
                            ..
                        }
                    ) {
                        assert_eq!(
                            Some(requesting_partition),
                            order.head(),
                            "group publication fence moved away from its head"
                        );
                    }
                    *record
                        .residence
                        .links(DemandOrderView::Group)
                        .expect("group-ordered demand lost its links")
                },
            );
        }
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;

    fn partition(index: usize) -> PartitionId {
        PartitionId::from_index(index)
    }

    fn active(id: u64, group: EligibilityGroup) -> DemandSnapshot {
        DemandSnapshot::active(
            DemandId::from_u64(id),
            super::super::SnapshotVersion::INITIAL,
            ProtocolRequirement::H1Compatible,
            group,
        )
    }

    #[test]
    fn origin_and_group_orders_repair_independently() {
        let pool = EligibilityGroup::Pool;
        let isolated = EligibilityGroup::Partition(partition(2));
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition(1), active(1, pool.clone()));
        schedule.publish(partition(2), active(2, isolated.clone()));
        schedule.publish(partition(3), active(3, pool.clone()));

        assert_eq!(
            Some(partition(1)),
            schedule.queued_head().map(|head| head.requesting_partition)
        );
        assert_eq!(
            Some(partition(1)),
            schedule
                .queued_group_head(&pool)
                .map(|head| head.requesting_partition)
        );
        assert_eq!(
            Some(partition(2)),
            schedule
                .queued_group_head(&isolated)
                .map(|head| head.requesting_partition)
        );

        let delivery = DeliveryId(7);
        schedule
            .reserve_group_head(&pool, &partition(1), DemandId::from_u64(1), delivery)
            .expect("pool group head should reserve");
        assert!(schedule.queued_group_head(&pool).is_none());
        assert_eq!(
            Some(partition(2)),
            schedule
                .queued_group_head(&isolated)
                .map(|head| head.requesting_partition)
        );
        assert!(schedule.queued_head().is_none());

        schedule.finish_delivery(
            delivery,
            &partition(1),
            DeliveryAckResult::Accepted { successor: None },
        );
        assert_eq!(
            Some(partition(2)),
            schedule.queued_head().map(|head| head.requesting_partition)
        );
        assert_eq!(
            Some(partition(3)),
            schedule
                .queued_group_head(&pool)
                .map(|head| head.requesting_partition)
        );
    }

    #[test]
    fn retry_preserves_both_order_positions() {
        let group = EligibilityGroup::Pool;
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition(1), active(1, group.clone()));
        schedule.publish(partition(2), active(2, group.clone()));

        let delivery = DeliveryId(9);
        schedule
            .reserve_group_head(&group, &partition(1), DemandId::from_u64(1), delivery)
            .expect("group head should reserve");
        schedule.finish_delivery(
            delivery,
            &partition(1),
            DeliveryAckResult::RetrySameResidence,
        );

        assert_eq!(
            Some(partition(1)),
            schedule.queued_head().map(|head| head.requesting_partition)
        );
        assert_eq!(
            Some(partition(1)),
            schedule
                .queued_group_head(&group)
                .map(|head| head.requesting_partition)
        );
    }

    #[test]
    fn inactive_snapshot_during_group_fence_retires_without_losing_group() {
        let group = EligibilityGroup::Pool;
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition(1), active(1, group.clone()));
        let delivery = DeliveryId(11);
        schedule
            .reserve_group_head(&group, &partition(1), DemandId::from_u64(1), delivery)
            .expect("group head should reserve");
        schedule.publish(
            partition(1),
            DemandSnapshot::inactive(
                DemandId::from_u64(1),
                super::super::SnapshotVersion::INITIAL.next(),
            ),
        );

        schedule.finish_delivery(
            delivery,
            &partition(1),
            DeliveryAckResult::RetrySameResidence,
        );
        assert_eq!(0, schedule.len());
        assert!(schedule.queued_group_head(&group).is_none());
    }

    #[test]
    fn accepted_fence_does_not_retire_a_newer_active_publication() {
        let group = EligibilityGroup::Pool;
        let partition = partition(1);
        let demand = DemandId::from_u64(1);
        let initial = super::super::SnapshotVersion::INITIAL;
        let mut schedule = DemandSchedule::default();
        schedule.publish(
            partition,
            DemandSnapshot::active(
                demand,
                initial,
                ProtocolRequirement::H2Required,
                group.clone(),
            ),
        );
        let delivery = DeliveryId(12);
        schedule
            .reserve_group_head(&group, &partition, demand, delivery)
            .expect("group head should reserve");
        let republished = DemandSnapshot::active(
            demand,
            initial.next().next(),
            ProtocolRequirement::H2Required,
            group.clone(),
        );
        schedule.publish(partition, republished.clone());

        schedule.finish_delivery(
            delivery,
            &partition,
            DeliveryAckResult::Accepted { successor: None },
        );

        assert_eq!(Some(&republished), schedule.latest_for_test(&partition));
        assert_eq!(
            Some(partition),
            schedule.queued_head().map(|head| head.requesting_partition)
        );
        assert_eq!(
            Some(partition),
            schedule
                .queued_group_head(&group)
                .map(|head| head.requesting_partition)
        );
    }

    #[test]
    fn group_reservation_rejects_a_stale_demand_identity() {
        let group = EligibilityGroup::Pool;
        let partition = partition(1);
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition, active(1, group.clone()));
        schedule.publish(partition, active(2, group.clone()));

        assert!(schedule
            .reserve_group_head(&group, &partition, DemandId::from_u64(1), DeliveryId(13),)
            .is_none());
        assert_eq!(
            Some(DemandId::from_u64(2)),
            schedule
                .queued_group_head(&group)
                .map(|queued| queued.demand)
        );
    }

    #[test]
    fn stale_acknowledgement_does_not_close_a_newer_delivery_fence() {
        let group = EligibilityGroup::Pool;
        let partition = partition(1);
        let demand = DemandId::from_u64(1);
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition, active(1, group.clone()));
        schedule
            .reserve_group_head(&group, &partition, demand, DeliveryId(14))
            .expect("group head should reserve");

        schedule.finish_delivery(
            DeliveryId(15),
            &partition,
            DeliveryAckResult::Accepted { successor: None },
        );

        assert!(schedule.delivery_is_current(DeliveryId(14), &partition, demand));
        assert!(!schedule.delivery_is_current(DeliveryId(15), &partition, demand));
        assert_eq!(
            Some(&active(1, group)),
            schedule.latest_for_test(&partition)
        );
    }

    #[test]
    #[should_panic(expected = "ordered demand lost its eligibility-group order")]
    fn consistency_check_rejects_an_orphaned_group_link() {
        let group = EligibilityGroup::Pool;
        let mut schedule = DemandSchedule::default();
        schedule.publish(partition(1), active(1, group.clone()));

        schedule.group_orders.remove(&group);
        schedule.assert_consistent();
    }
}

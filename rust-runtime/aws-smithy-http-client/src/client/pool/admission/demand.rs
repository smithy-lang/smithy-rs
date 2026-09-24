/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Versioned cross-cell demand scheduling for one bounded origin.
//!
//! Each retained cell publishes a complete demand snapshot. Active snapshots
//! occupy one origin-wide FIFO, and a delivery remains at the head as a fence
//! until its requesting cell acknowledges ownership or rejection.

use super::{
    DeliveryAckResult, DeliveryId, DemandId, DemandSnapshot, DemandState, IntrusiveLinks,
    IntrusiveOrder, Permit, ProtocolRequirement,
};
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use std::collections::HashMap;

/// Cross-cell demand records and their origin-wide scheduling order.
///
/// At every completed transition:
///
/// - `records` owns the newest snapshot for every retained cell.
/// - `IntrusiveOrder::Active` contains exactly the `Queued` and `Delivering`
///   records.
/// - Queue links live inside those ordered residence variants.
/// - A `Delivering` record remains at the head as a scheduling fence.
///
/// Admission coordinates capacity extraction with this schedule while holding
/// the same origin lock.
#[derive(Debug, Default)]
pub(super) struct DemandSchedule {
    /// Latest demand and scheduling residence for each retained cell.
    records: HashMap<PartitionId, DemandRecord>,
    /// Origin-wide order, including an outstanding delivery fence.
    order: IntrusiveOrder<PartitionId>,
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
    /// Scheduling residence, including links while ordered.
    residence: DemandResidence,
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
        /// Origin-wide scheduling links and arrival sequence.
        links: IntrusiveLinks<PartitionId>,
    },
    /// One delivery guard owns capacity for this demand.
    Delivering {
        /// Demand generation fenced at the order head.
        demand: DemandId,
        /// Delivery allowed to complete this fence.
        delivery: DeliveryId,
        /// Origin-wide scheduling links retained until acknowledgement.
        links: IntrusiveLinks<PartitionId>,
    },
}

impl DemandResidence {
    #[cfg(debug_assertions)]
    /// Borrows order links while this record is queued or delivering.
    fn links(&self) -> Option<&IntrusiveLinks<PartitionId>> {
        match self {
            Self::Idle => None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => Some(links),
        }
    }

    /// Mutably borrows links from a record known to be ordered.
    ///
    /// # Panics
    ///
    /// Panics when called for an idle record.
    fn links_mut(&mut self) -> &mut IntrusiveLinks<PartitionId> {
        match self {
            Self::Idle => panic!("idle demand has no order links"),
            Self::Queued { links, .. } | Self::Delivering { links, .. } => links,
        }
    }

    /// Detaches links while moving a record out of origin-wide order.
    fn into_links(self) -> Option<IntrusiveLinks<PartitionId>> {
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

    /// Appends an idle active demand to the origin-wide order.
    fn enqueue(&mut self, requesting_partition: PartitionId) {
        let links = self.order.push_back(requesting_partition);
        if let Some(previous) = links.previous {
            self.records
                .get_mut(&previous)
                .expect("order tail disappeared")
                .residence
                .links_mut()
                .next = Some(requesting_partition);
        }

        let record = self
            .records
            .get_mut(&requesting_partition)
            .expect("queued demand record disappeared");
        debug_assert!(matches!(record.residence, DemandResidence::Idle));
        let demand = record.latest.id;
        record.residence = DemandResidence::Queued { demand, links };
    }

    /// Removes an ordered demand and leaves its retained record idle.
    fn remove_from_order(&mut self, requesting_partition: &PartitionId) {
        let residence = {
            let record = self
                .records
                .get_mut(requesting_partition)
                .expect("removed demand record disappeared");
            std::mem::replace(&mut record.residence, DemandResidence::Idle)
        };
        let links = residence
            .into_links()
            .expect("removed demand had no order links");

        if let Some(previous) = links.previous {
            self.records
                .get_mut(&previous)
                .expect("previous demand disappeared")
                .residence
                .links_mut()
                .next = links.next;
        }
        if let Some(next) = links.next {
            self.records
                .get_mut(&next)
                .expect("next demand disappeared")
                .residence
                .links_mut()
                .previous = links.previous;
        }

        self.order.remove(*requesting_partition, links);
    }

    /// Returns whether the head can begin a new one-to-one delivery.
    pub(super) fn head_is_queued(&self) -> bool {
        let Some(head) = self.order.head() else {
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
        let head = self.order.head()?;
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
        let head = self.order.head()?;
        if head != *requesting_partition {
            return None;
        }
        self.reserve_head(delivery)
    }

    /// Changes the queued head into a delivery fence at the same order position.
    pub(super) fn reserve_head(&mut self, delivery: DeliveryId) -> Option<ScheduledDemand> {
        let head = self.order.head()?;
        let requesting_partition = head;
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
                    delivery,
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
                delivery: current,
                ..
            } if *current == delivery => *demand,
            _ => return,
        };

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
                        delivery: current,
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
                } else if self
                    .records
                    .get(requesting_partition)
                    .expect("delivery demand record disappeared")
                    .latest
                    .id
                    == delivered_demand
                {
                    let record = self
                        .records
                        .get_mut(requesting_partition)
                        .expect("delivery demand record disappeared");
                    record.latest =
                        DemandSnapshot::inactive(delivered_demand, record.latest.version.next());
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
        self.order.len()
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

    /// Checks record residence, FIFO links, length, and head-fence relationships.
    fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        self.assert_consistent_debug();
    }

    #[cfg(debug_assertions)]
    fn assert_consistent_debug(&self) {
        let ordered_records = self
            .records
            .values()
            .filter(|record| record.residence.links().is_some())
            .count();
        let head = self.order.head();
        let mut delivering = 0;
        self.order.assert_consistent(
            ordered_records,
            self.records.len(),
            "demand order",
            |requesting_partition| {
                let record = self
                    .records
                    .get(&requesting_partition)
                    .expect("ordered demand disappeared");
                match &record.residence {
                    DemandResidence::Idle => unreachable!("ordered demand became idle"),
                    DemandResidence::Queued { demand, .. } => {
                        assert!(record.latest.is_active(), "queued demand became inactive");
                        assert_eq!(
                            record.latest.id, *demand,
                            "queued residence did not match its latest demand"
                        );
                    }
                    DemandResidence::Delivering { demand, .. } => {
                        delivering += 1;
                        assert_eq!(
                            Some(requesting_partition),
                            head,
                            "delivery fence moved away from the order head"
                        );
                        assert!(
                            record.latest.id >= *demand,
                            "delivery fence named a future demand"
                        );
                    }
                }
                *record
                    .residence
                    .links()
                    .expect("ordered demand lost its links")
            },
        );
        assert!(delivering <= 1, "more than one delivery fence was active");
    }
}

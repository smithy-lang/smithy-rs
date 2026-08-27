/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Versioned cross-cell demand scheduling for one bounded origin.
//!
//! Each retained cell publishes a complete demand snapshot. Active snapshots
//! occupy one origin-wide FIFO, and a delivery remains at the head as a fence
//! until its target acknowledges ownership or rejection.

use super::{
    DeliveryId, DemandId, DemandSnapshot, DemandState, PermitId, ProtocolRequirement,
    TargetAckResult,
};
use crate::client::pool::cell::CellId;
use crate::client::pool::partition::EligibilityGroup;
use std::collections::HashMap;
use std::num::NonZeroUsize;

/// Cross-cell demand records and their origin-wide scheduling order.
///
/// At every completed transition:
///
/// - `records` owns the newest snapshot for every retained cell.
/// - `DemandOrderState::Active` contains exactly the `Queued` and `Delivering`
///   records.
/// - Queue links live inside those ordered residence variants.
/// - A `Delivering` record remains at the head as a scheduling fence.
///
/// Admission coordinates permit extraction with this schedule while holding
/// the same origin lock.
#[derive(Debug, Default)]
pub(super) struct DemandSchedule {
    /// Latest demand and scheduling residence for each retained cell.
    records: HashMap<CellId, DemandRecord>,
    /// Origin-wide order, including an outstanding delivery fence.
    order: DemandOrderState,
}

/// Complete origin-order head used to choose one HTTP/1 source action.
#[derive(Clone, Debug)]
pub(super) struct QueuedDemand {
    /// Cell whose oldest waiter owns this demand episode.
    pub(super) target: CellId,
    /// Demand identity revalidated at each crossing.
    pub(super) demand: DemandId,
    /// Protocol capability required by the target waiter.
    pub(super) requirement: ProtocolRequirement,
    /// Sources whose H1 senders may be borrowed by the target.
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

/// Admission residence for one cell's latest demand.
#[derive(Clone, Debug)]
enum DemandResidence {
    /// The cell has no demand represented in scheduling.
    Idle,
    /// The demand is waiting in origin order.
    Queued {
        /// Demand episode represented by this residence.
        demand: DemandId,
        /// Origin-wide scheduling links and arrival sequence.
        links: OrderLinks,
    },
    /// One delivery guard owns capacity for this demand.
    Delivering {
        /// Demand episode fenced at the order head.
        demand: DemandId,
        /// Delivery allowed to complete this fence.
        delivery: DeliveryId,
        /// Origin-wide scheduling links retained until acknowledgement.
        links: OrderLinks,
    },
}

impl DemandResidence {
    #[cfg(debug_assertions)]
    /// Borrows order links while this record is queued or delivering.
    fn links(&self) -> Option<&OrderLinks> {
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
    fn links_mut(&mut self) -> &mut OrderLinks {
        match self {
            Self::Idle => panic!("idle demand has no order links"),
            Self::Queued { links, .. } | Self::Delivering { links, .. } => links,
        }
    }

    /// Detaches links while moving a record out of origin-wide order.
    fn into_links(self) -> Option<OrderLinks> {
        match self {
            Self::Idle => None,
            Self::Queued { links, .. } | Self::Delivering { links, .. } => Some(links),
        }
    }
}

/// Whether origin-wide scheduling currently retains any demand.
#[derive(Debug, Default)]
enum DemandOrderState {
    /// No demand is ordered.
    #[default]
    Empty,
    /// A nonempty origin-wide order, possibly headed by a delivery fence.
    Active {
        /// Oldest ordered demand.
        head: CellId,
        /// Youngest ordered demand.
        tail: CellId,
        /// Number of records linked from `head` through `tail`.
        len: NonZeroUsize,
    },
}

/// Intrusive links retaining one live demand in origin order.
#[derive(Clone, Debug)]
struct OrderLinks {
    /// Older demand in origin order.
    previous: Option<CellId>,
    /// Newer demand in origin order.
    next: Option<CellId>,
}

/// Demand reserved at the order head for one capacity delivery.
pub(super) struct ScheduledDemand {
    /// Selected destination cell.
    pub(super) target: CellId,
    /// Demand identity current when the crossing was prepared.
    pub(super) demand: DemandId,
}

/// Connection capacity extracted under admission lock before crossing to a cell.
#[derive(Debug)]
pub(super) struct PreparedCapacityDelivery {
    /// Permit transferred into the delivery guard.
    pub(super) permit: PermitId,
    /// Fence identity allocated for this crossing.
    pub(super) delivery: DeliveryId,
    /// Selected destination cell.
    pub(super) target: CellId,
    /// Demand identity current when the crossing was prepared.
    pub(super) demand: DemandId,
}

impl DemandSchedule {
    /// Applies a complete snapshot and updates its cell's scheduling residence.
    pub(super) fn publish(&mut self, target: CellId, snapshot: DemandSnapshot) {
        if let Some(current) = self.records.get(&target) {
            if !snapshot.is_newer_than(&current.latest) {
                return;
            }
        } else {
            self.records.insert(
                target.clone(),
                DemandRecord {
                    latest: snapshot.clone(),
                    residence: DemandResidence::Idle,
                },
            );
        }

        let should_remove = matches!(
            &self
                .records
                .get(&target)
                .expect("published demand record disappeared")
                .residence,
            DemandResidence::Queued { demand, .. }
                if *demand != snapshot.id || !snapshot.is_active()
        );
        if should_remove {
            self.remove_from_order(&target);
        }

        self.records
            .get_mut(&target)
            .expect("published demand record disappeared")
            .latest = snapshot;

        let record = self
            .records
            .get(&target)
            .expect("published demand record disappeared");
        if record.latest.is_active() && matches!(&record.residence, DemandResidence::Idle) {
            self.enqueue(target);
        }
        self.assert_consistent();
    }

    /// Appends an idle active demand to the origin-wide order.
    fn enqueue(&mut self, target: CellId) {
        let previous = match &self.order {
            DemandOrderState::Empty => None,
            DemandOrderState::Active { tail, .. } => Some(tail.clone()),
        };
        if let Some(previous) = previous.as_ref() {
            self.records
                .get_mut(previous)
                .expect("order tail disappeared")
                .residence
                .links_mut()
                .next = Some(target.clone());
        }

        let record = self
            .records
            .get_mut(&target)
            .expect("queued demand record disappeared");
        debug_assert!(matches!(record.residence, DemandResidence::Idle));
        let demand = record.latest.id;
        record.residence = DemandResidence::Queued {
            demand,
            links: OrderLinks {
                previous,
                next: None,
            },
        };

        match &mut self.order {
            order @ DemandOrderState::Empty => {
                *order = DemandOrderState::Active {
                    head: target.clone(),
                    tail: target,
                    len: NonZeroUsize::MIN,
                };
            }
            DemandOrderState::Active { tail, len, .. } => {
                *tail = target;
                *len = len.checked_add(1).expect("demand order length exhausted");
            }
        }
    }

    /// Removes an ordered demand and leaves its retained record idle.
    fn remove_from_order(&mut self, target: &CellId) {
        let residence = {
            let record = self
                .records
                .get_mut(target)
                .expect("removed demand record disappeared");
            std::mem::replace(&mut record.residence, DemandResidence::Idle)
        };
        let links = residence
            .into_links()
            .expect("removed demand had no order links");

        if let Some(previous) = links.previous.as_ref() {
            self.records
                .get_mut(previous)
                .expect("previous demand disappeared")
                .residence
                .links_mut()
                .next = links.next.clone();
        }
        if let Some(next) = links.next.as_ref() {
            self.records
                .get_mut(next)
                .expect("next demand disappeared")
                .residence
                .links_mut()
                .previous = links.previous.clone();
        }

        let order = std::mem::take(&mut self.order);
        let DemandOrderState::Active { head, tail, len } = order else {
            unreachable!("removed a demand from an empty order");
        };
        debug_assert_eq!(head == *target, links.previous.is_none());
        debug_assert_eq!(tail == *target, links.next.is_none());

        if len == NonZeroUsize::MIN {
            debug_assert!(links.previous.is_none());
            debug_assert!(links.next.is_none());
            return;
        }

        self.order = DemandOrderState::Active {
            head: if head == *target {
                links.next.clone().expect("removed head had no successor")
            } else {
                head
            },
            tail: if tail == *target {
                links
                    .previous
                    .clone()
                    .expect("removed tail had no predecessor")
            } else {
                tail
            },
            len: NonZeroUsize::new(
                len.get()
                    .checked_sub(1)
                    .expect("demand order length underflowed"),
            )
            .expect("nonempty demand order lost its length"),
        };
    }

    /// Returns whether the head can begin a new one-to-one delivery.
    pub(super) fn head_is_queued(&self) -> bool {
        let DemandOrderState::Active { head, .. } = &self.order else {
            return false;
        };
        matches!(
            &self
                .records
                .get(head)
                .expect("order head disappeared")
                .residence,
            DemandResidence::Queued { .. }
        )
    }

    /// Returns the complete origin-order head when it may receive a claim.
    pub(super) fn queued_head(&self) -> Option<QueuedDemand> {
        let DemandOrderState::Active { head, .. } = &self.order else {
            return None;
        };
        let record = self.records.get(head).expect("order head disappeared");
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
            target: head.clone(),
            demand: *demand,
            requirement: *requirement,
            eligibility_group: eligibility_group.clone(),
        })
    }

    /// Returns whether `target` still has this demand queued for a new action.
    pub(super) fn is_current_queued(&self, target: &CellId, demand: DemandId) -> bool {
        self.records.get(target).is_some_and(|record| {
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

    /// Fences a claim's exact target when it is still the origin-order head.
    pub(super) fn reserve_claim_target(
        &mut self,
        target: &CellId,
        demand: DemandId,
        delivery: DeliveryId,
    ) -> Option<ScheduledDemand> {
        if !self.is_current_queued(target, demand) {
            return None;
        }
        let DemandOrderState::Active { head, .. } = &self.order else {
            return None;
        };
        if head != target {
            return None;
        }
        self.reserve_head(delivery)
    }

    /// Changes the queued head into a delivery fence at the same order position.
    pub(super) fn reserve_head(&mut self, delivery: DeliveryId) -> Option<ScheduledDemand> {
        let DemandOrderState::Active { head, .. } = &self.order else {
            return None;
        };
        let target = head.clone();
        let record = self
            .records
            .get_mut(&target)
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
                Some(ScheduledDemand { target, demand })
            }
            residence => {
                record.residence = residence;
                None
            }
        }
    }

    /// Returns whether a delivery still owns the target demand's head fence.
    ///
    /// Cell-side reservation uses this identity check to reject a crossing
    /// prepared for demand that was replaced while no pool lock was held.
    #[cfg(test)]
    pub(super) fn delivery_is_current(
        &self,
        delivery: DeliveryId,
        target: &CellId,
        demand: DemandId,
    ) -> bool {
        let Some(record) = self.records.get(target) else {
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
    /// `Accepted` consumes the delivered episode and installs a newer
    /// successor when the target supplied one. `RetrySameResidence` restores
    /// the same episode at its existing order position when it remains live;
    /// if it was replaced, the latest active episode is appended as new
    /// demand. `Rejected` removes the fenced episode after its permit has
    /// already been refunnelled and applies the same successor arbitration as
    /// acceptance.
    pub(super) fn finish_delivery(
        &mut self,
        delivery: DeliveryId,
        target: &CellId,
        result: TargetAckResult,
    ) {
        let Some(record) = self.records.get(target) else {
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
            TargetAckResult::RetrySameResidence => {
                let record = self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared");
                if record.latest.id == delivered_demand && record.latest.is_active() {
                    let record = self
                        .records
                        .get_mut(target)
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

                self.remove_from_order(target);
                if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(target.clone());
                }
            }
            TargetAckResult::Accepted { successor } | TargetAckResult::Rejected { successor } => {
                self.remove_from_order(target);

                let install_successor = successor.as_ref().is_some_and(|successor| {
                    successor.id > delivered_demand
                        && successor.is_newer_than(
                            &self
                                .records
                                .get(target)
                                .expect("delivery demand record disappeared")
                                .latest,
                        )
                });
                if install_successor {
                    self.records
                        .get_mut(target)
                        .expect("delivery demand record disappeared")
                        .latest = successor.expect("validated successor disappeared");
                } else if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .id
                    == delivered_demand
                {
                    let record = self
                        .records
                        .get_mut(target)
                        .expect("delivery demand record disappeared");
                    record.latest =
                        DemandSnapshot::inactive(delivered_demand, record.latest.version.next());
                }

                if self
                    .records
                    .get(target)
                    .expect("delivery demand record disappeared")
                    .latest
                    .is_active()
                {
                    self.enqueue(target.clone());
                }
            }
        }
        self.assert_consistent();
    }

    /// Returns the latest complete snapshot retained for `target`.
    #[cfg(test)]
    pub(super) fn latest_for_test(&self, target: &CellId) -> Option<&DemandSnapshot> {
        self.records.get(target).map(|record| &record.latest)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        match &self.order {
            DemandOrderState::Empty => 0,
            DemandOrderState::Active { len, .. } => len.get(),
        }
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
        match &self.order {
            DemandOrderState::Empty => {
                assert_eq!(0, ordered_records, "empty order retained ordered demand");
            }
            DemandOrderState::Active { head, tail, len } => {
                let mut current = Some(head.clone());
                let mut previous = None;
                let mut traversed = 0;
                let mut delivering = 0;
                while let Some(target) = current {
                    assert!(
                        traversed < self.records.len(),
                        "demand order contains a cycle"
                    );
                    let record = self
                        .records
                        .get(&target)
                        .expect("ordered demand disappeared");
                    let links = record
                        .residence
                        .links()
                        .expect("ordered demand lost its links");
                    assert_eq!(
                        previous, links.previous,
                        "demand order contains inconsistent backward links"
                    );
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
                                target, *head,
                                "delivery fence moved away from the order head"
                            );
                            assert!(
                                record.latest.id >= *demand,
                                "delivery fence named a future demand"
                            );
                        }
                    }

                    traversed += 1;
                    previous = Some(target);
                    current = links.next.clone();
                }

                assert!(delivering <= 1, "more than one delivery fence was active");
                assert_eq!(Some(tail.clone()), previous, "order tail was not reachable");
                assert_eq!(
                    len.get(),
                    traversed,
                    "demand order length did not match its links"
                );
                assert_eq!(
                    ordered_records, traversed,
                    "ordered demand was not reachable from the order head"
                );
            }
        }
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-to-one acquisition payload delivery across admission and cell locks.
//!
//! Admission reserves one demand generation and moves either bounded capacity
//! or a provisional HTTP/1 sender into a [`DeliveryGuard`]. The guard
//! commits any connection cell transition before taking the requesting
//! cell lock, then owns the payload until that cell accepts it. Its
//! acknowledgement keeps the demand fence installed until requesting-cell
//! state is authoritative.

use super::reuse::{H1AvailabilityOutcome, ReuseCandidate, ReuseId};
use super::{
    AdmissionAction, DeliveryAckResult, DeliveryId, DemandId, DemandSnapshot, OriginAdmission,
    Permit,
};
use crate::client::pool::cell::h1::H1Selection;
use crate::client::pool::cell::{
    AcquisitionEvent, AcquisitionResult, EstablishmentPermit, OriginCell,
};
use crate::client::pool::partition::PartitionId;
use crate::sync::Arc;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::fmt;

/// Capacity or a borrowed HTTP/1 sender crossing to waiting demand.
enum AcquisitionPayload {
    /// Permit removed from admission but not yet represented by a lease.
    Capacity(Permit),
    /// Provisional sender whose owning-cell reservation must revalidate.
    BorrowedH1 {
        /// Reuse operation that selected this sender.
        reuse_id: ReuseId,
        /// Cell that owns the sender and local reservation.
        connection_partition: PartitionId,
        /// Sender owner with cancellation fallback.
        candidate: ReuseCandidate,
    },
}

/// Payload after every connection cell-side fallible transition has completed.
enum MaterializedPayload {
    /// Establishment authority ready to move into the requesting cell.
    Capacity(EstablishmentPermit),
    /// Selected sender ready to move into the requesting cell.
    BorrowedH1 {
        /// Reuse operation that selected this sender.
        reuse_id: ReuseId,
        /// Cell that owns the installed connection record.
        connection_partition: PartitionId,
        /// Dispatch-ready sender ownership.
        selection: H1Selection,
    },
}

/// One acquisition payload and its admission-owned delivery fence.
///
/// Dropping an undelivered guard refunnels its payload before making the
/// requesting cell's demand schedulable again. Once committed, [`DeliveryAck`]
/// owns fence completion and the requesting cell owns the acquisition event.
pub(in crate::client::pool) struct DeliveryGuard {
    /// Admission authority that owns the delivery fence and fallback.
    origin: Arc<OriginAdmission>,
    /// Never-reused identity of this one crossing.
    delivery: DeliveryId,
    /// Cell selected while admission was locked.
    requesting_partition: PartitionId,
    /// Exact requesting-cell demand generation that must revalidate the
    /// payload.
    demand: DemandId,
    /// Payload ownership before materialization, after materialization, or after transfer.
    state: DeliveryGuardState,
}

/// Ownership phase of one admission-to-cell delivery.
///
/// ```text
/// Undelivered -- materialize connection-cell work ----------> Materialized
/// Undelivered -- peer sender rejection or drop ------------------> admission fallback
/// Materialized -- commit to requesting cell ----------------> Disarmed + DeliveryAck
/// Materialized -- drop -------------------------------------> admission fallback
/// ```
///
/// `Disarmed` owns no payload. After commit, `DeliveryAck` owns fence and
/// connection-cell completion.
enum DeliveryGuardState {
    /// Admission removed the payload, but connection cell-side work may still fail.
    Undelivered {
        /// Payload still owned by this fallback.
        payload: Option<AcquisitionPayload>,
        /// Demand acknowledgement used if the guard is dropped.
        on_drop: DeliveryAckResult,
    },
    /// Connection-owning-cell work completed; the requesting cell may reserve
    /// its waiter.
    Materialized {
        /// Requesting-cell-ready payload still owned by this fallback.
        payload: Option<MaterializedPayload>,
        /// Demand acknowledgement used if the guard is dropped.
        on_drop: DeliveryAckResult,
    },
    /// Payload and fallback responsibility moved to requesting cell-owned state.
    Disarmed,
}

impl DeliveryGuard {
    /// Creates a delivery for one permit removed from admission.
    pub(super) fn capacity(
        origin: Arc<OriginAdmission>,
        delivery: DeliveryId,
        requesting_partition: PartitionId,
        demand: DemandId,
        permit: Permit,
    ) -> Self {
        Self {
            origin,
            delivery,
            requesting_partition,
            demand,
            state: DeliveryGuardState::Undelivered {
                payload: Some(AcquisitionPayload::Capacity(permit)),
                on_drop: DeliveryAckResult::RetrySameResidence,
            },
        }
    }

    /// Creates a delivery for one provisional sender selected for borrowing.
    pub(super) fn borrowed_h1(
        origin: Arc<OriginAdmission>,
        delivery: DeliveryId,
        requesting_partition: PartitionId,
        demand: DemandId,
        reuse_id: ReuseId,
        connection_partition: PartitionId,
        candidate: ReuseCandidate,
    ) -> Self {
        Self {
            origin,
            delivery,
            requesting_partition,
            demand,
            state: DeliveryGuardState::Undelivered {
                payload: Some(AcquisitionPayload::BorrowedH1 {
                    reuse_id,
                    connection_partition,
                    candidate,
                }),
                on_drop: DeliveryAckResult::RetrySameResidence,
            },
        }
    }

    /// Returns the demand generation fenced by this delivery.
    pub(in crate::client::pool) fn demand(&self) -> DemandId {
        self.demand
    }

    /// Returns whether admission still recognizes this delivery fence.
    #[cfg(test)]
    pub(in crate::client::pool) fn is_current(&self) -> bool {
        self.origin
            .delivery_is_current(self.delivery, &self.requesting_partition, self.demand)
    }

    /// Materializes owning-cell state and attempts one requesting-cell delivery.
    pub(super) fn deliver_once(mut self) -> Option<AdmissionAction> {
        if !self.materialize() {
            return None;
        }
        match self.origin.cell(&self.requesting_partition) {
            Some(requesting_cell) => OriginCell::receive_delivery(&requesting_cell, self),
            None => self.reject(None),
        }
    }

    /// Completes fallible owning-cell work before reserving requesting-cell state.
    fn materialize(&mut self) -> bool {
        let state = std::mem::replace(&mut self.state, DeliveryGuardState::Disarmed);
        let DeliveryGuardState::Undelivered {
            mut payload,
            on_drop,
        } = state
        else {
            unreachable!("delivery payload materialized more than once");
        };
        let payload = payload
            .take()
            .expect("undelivered acquisition payload disappeared");
        let materialized = match payload {
            AcquisitionPayload::Capacity(permit) => {
                MaterializedPayload::Capacity(EstablishmentPermit::bounded(
                    super::CapacityLease::new(self.origin.clone(), permit),
                ))
            }
            AcquisitionPayload::BorrowedH1 {
                reuse_id,
                connection_partition,
                candidate,
            } => match candidate.commit() {
                Ok(selection) => MaterializedPayload::BorrowedH1 {
                    reuse_id,
                    connection_partition,
                    selection,
                },
                Err(candidate) => {
                    drop(candidate);
                    let next = OriginAdmission::finish_delivery(
                        &self.origin,
                        self.delivery,
                        &self.requesting_partition,
                        None,
                        on_drop,
                    );
                    OriginAdmission::drive(next);
                    return false;
                }
            },
        };
        self.state = DeliveryGuardState::Materialized {
            payload: Some(materialized),
            on_drop,
        };
        true
    }

    /// Materializes this guard before tests manually split the requesting cell
    /// reservation and installation transitions.
    #[cfg(test)]
    pub(in crate::client::pool) fn materialize_for_test(&mut self) -> bool {
        self.materialize()
    }

    /// Moves the materialized payload into a requesting cell-owned event.
    pub(in crate::client::pool) fn commit(
        mut self,
        successor: Option<DemandSnapshot>,
    ) -> (AcquisitionEvent, DeliveryAck) {
        let DeliveryGuardState::Materialized { payload, .. } = &mut self.state else {
            unreachable!("delivery committed before payload materialization");
        };
        let payload = payload
            .take()
            .expect("materialized acquisition payload moved more than once");
        let (event, kind) = match payload {
            MaterializedPayload::Capacity(permit) => {
                (AcquisitionEvent::Establish(permit), DeliveryKind::Capacity)
            }
            MaterializedPayload::BorrowedH1 {
                reuse_id,
                connection_partition,
                selection,
            } => {
                let connection_id = selection.connection_id();
                (
                    AcquisitionEvent::Complete(AcquisitionResult::H1(selection)),
                    DeliveryKind::BorrowedH1 {
                        connection_id,
                        reuse_id,
                        connection_partition,
                    },
                )
            }
        };
        self.state = DeliveryGuardState::Disarmed;
        (
            event,
            DeliveryAck {
                origin: self.origin.clone(),
                delivery: self.delivery,
                requesting_partition: self.requesting_partition,
                successor,
                kind: Some(kind),
            },
        )
    }

    /// Refunnels this delivery after requesting cell revalidation rejects it.
    pub(in crate::client::pool) fn reject(
        mut self,
        successor: Option<DemandSnapshot>,
    ) -> Option<AdmissionAction> {
        let state = std::mem::replace(&mut self.state, DeliveryGuardState::Disarmed);
        self.finish_state(state, DeliveryAckResult::Rejected { successor })
    }

    /// Resolves payload fallback before closing the admission-owned fence.
    fn finish_state(
        &self,
        state: DeliveryGuardState,
        result: DeliveryAckResult,
    ) -> Option<AdmissionAction> {
        match state {
            DeliveryGuardState::Undelivered { mut payload, .. } => {
                let permit = match payload.take() {
                    Some(AcquisitionPayload::Capacity(permit)) => Some(permit),
                    Some(AcquisitionPayload::BorrowedH1 { candidate, .. }) => {
                        drop(candidate);
                        None
                    }
                    None => None,
                };
                OriginAdmission::finish_delivery(
                    &self.origin,
                    self.delivery,
                    &self.requesting_partition,
                    permit,
                    result,
                )
            }
            DeliveryGuardState::Materialized { mut payload, .. } => match payload.take() {
                Some(MaterializedPayload::Capacity(permit)) => {
                    drop(permit);
                    OriginAdmission::finish_delivery(
                        &self.origin,
                        self.delivery,
                        &self.requesting_partition,
                        None,
                        result,
                    )
                }
                Some(MaterializedPayload::BorrowedH1 {
                    reuse_id,
                    connection_partition,
                    selection,
                }) => {
                    let owning_cell = self.origin.cell(&connection_partition);
                    drop(selection);
                    let outcome = match owning_cell {
                        Some(owning_cell) => H1AvailabilityOutcome::reported(
                            connection_partition,
                            owning_cell.cancel_h1_reuse(reuse_id),
                        ),
                        None => H1AvailabilityOutcome::expired(connection_partition),
                    };
                    OriginAdmission::finish_borrow_delivery(
                        &self.origin,
                        reuse_id,
                        self.delivery,
                        &self.requesting_partition,
                        result,
                        None,
                        Some(outcome),
                    )
                }

                None => None,
            },
            DeliveryGuardState::Disarmed => None,
        }
    }
}

impl fmt::Debug for DeliveryGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliveryGuard")
            .field("delivery", &self.delivery)
            .field("requesting_partition", &self.requesting_partition)
            .field("demand", &self.demand)
            .finish_non_exhaustive()
    }
}

impl Drop for DeliveryGuard {
    fn drop(&mut self) {
        let state = std::mem::replace(&mut self.state, DeliveryGuardState::Disarmed);
        let result = match &state {
            DeliveryGuardState::Undelivered { on_drop, .. }
            | DeliveryGuardState::Materialized { on_drop, .. } => on_drop.clone(),
            DeliveryGuardState::Disarmed => return,
        };
        let next = self.finish_state(state, result);
        OriginAdmission::drive(next);
    }
}

/// Acknowledgement after an event becomes owned by the requesting cell.
///
/// Explicit rejection returns the event before closing the delivery fence.
/// Once the requesting cell owns the event, dropping this value acknowledges
/// success and completes any borrowed connection's reuse operation.
pub(in crate::client::pool) struct DeliveryAck {
    /// Admission authority that owns the outstanding fence.
    origin: Arc<OriginAdmission>,
    /// Delivery fence closed by this acknowledgement.
    delivery: DeliveryId,
    /// Cell that now owns or rejected the acquisition event.
    requesting_partition: PartitionId,
    /// Successor demand to publish after the old generation finishes.
    successor: Option<DemandSnapshot>,
    /// Payload-specific connection cell completion still owed by this guard.
    kind: Option<DeliveryKind>,
}

/// Terminal work required after the requesting cell takes ownership.
enum DeliveryKind {
    /// Only the admission delivery fence remains to acknowledge.
    Capacity,
    /// The connection cell must learn whether sender transfer succeeded.
    BorrowedH1 {
        /// Connection whose sender crossed to the requesting cell.
        connection_id: ConnectionId,
        /// Reuse operation completed by this acknowledgement.
        reuse_id: ReuseId,
        /// Cell whose local reuse reservation must complete.
        connection_partition: PartitionId,
    },
}

impl DeliveryAck {
    /// Avoids republishing a successor already covered by visible HTTP/2 state.
    ///
    /// Capacity or HTTP/1 delivery may reveal another waiter after the
    /// requesting cell already gained a local generation or peer route. That
    /// visible HTTP/2 state can serve an H2-compatible successor directly. If
    /// it later closes, its close path publishes the cell's current demand.
    pub(in crate::client::pool) fn suppress_h2_successor(&mut self) {
        if self
            .successor
            .as_ref()
            .is_some_and(DemandSnapshot::accepts_h2)
        {
            self.successor = None;
        }
    }

    /// Acknowledges that requesting cell state accepted the acquisition event.
    pub(in crate::client::pool) fn accept(mut self) -> Option<AdmissionAction> {
        let kind = self
            .kind
            .take()
            .expect("delivery acknowledgement completed more than once");
        let successor = self.successor.take();
        self.finish(kind, DeliveryAckResult::Accepted { successor }, None)
    }

    /// Refunnels events rejected after requesting cell reservation.
    pub(in crate::client::pool) fn reject(
        mut self,
        returned_events: [Option<AcquisitionEvent>; 2],
    ) -> Option<AdmissionAction> {
        let kind = self
            .kind
            .take()
            .expect("delivery acknowledgement completed more than once");
        let successor = self.successor.take();
        self.finish(
            kind,
            DeliveryAckResult::Rejected { successor },
            Some(returned_events),
        )
    }

    /// Completes the payload-specific reuse operation and delivery fence.
    fn finish(
        &self,
        kind: DeliveryKind,
        result: DeliveryAckResult,
        returned_events: Option<[Option<AcquisitionEvent>; 2]>,
    ) -> Option<AdmissionAction> {
        match kind {
            DeliveryKind::Capacity => {
                drop(returned_events);
                OriginAdmission::finish_delivery(
                    &self.origin,
                    self.delivery,
                    &self.requesting_partition,
                    None,
                    result,
                )
            }
            DeliveryKind::BorrowedH1 {
                connection_id,
                reuse_id,
                connection_partition,
            } => {
                let rejected = returned_events.is_some();
                let owning_cell = rejected
                    .then(|| self.origin.cell(&connection_partition))
                    .flatten();
                drop(returned_events);
                let rejected_outcome = rejected.then(|| match owning_cell {
                    Some(owning_cell) => H1AvailabilityOutcome::reported(
                        connection_partition,
                        owning_cell.cancel_h1_reuse(reuse_id),
                    ),
                    None => H1AvailabilityOutcome::expired(connection_partition),
                });
                let transferred_connection_cell = (!rejected).then_some(connection_partition);
                let action = OriginAdmission::finish_borrow_delivery(
                    &self.origin,
                    reuse_id,
                    self.delivery,
                    &self.requesting_partition,
                    result,
                    transferred_connection_cell,
                    rejected_outcome,
                );
                if !rejected {
                    tracing::trace!(
                        connection_id = %connection_id,
                        request_partition = ?self.requesting_partition,
                        connection_partition = ?connection_partition,
                        origin_scheme = %self.origin.origin().scheme(),
                        origin_host = self.origin.origin().host(),
                        origin_port = ?self.origin.origin().port(),
                        "HTTP/1 connection borrowed for peer demand"
                    );
                }
                action
            }
        }
    }
}

impl fmt::Debug for DeliveryAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliveryAck")
            .field("delivery", &self.delivery)
            .field("requesting_partition", &self.requesting_partition)
            .finish_non_exhaustive()
    }
}

impl Drop for DeliveryAck {
    fn drop(&mut self) {
        let Some(kind) = self.kind.take() else {
            return;
        };
        let successor = self.successor.take();
        let next = self.finish(kind, DeliveryAckResult::Accepted { successor }, None);
        OriginAdmission::drive(next);
    }
}

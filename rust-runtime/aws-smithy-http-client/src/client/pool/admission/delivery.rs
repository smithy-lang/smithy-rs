/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-to-one acquisition payload delivery across admission and cell locks.
//!
//! Admission reserves one demand generation and moves either bounded capacity
//! or a provisional HTTP/1 sender into a [`DeliveryGuard`]. The guard
//! commits any connection-owning cell transition before taking the requesting
//! cell lock, then owns the payload until that cell accepts it. Its
//! acknowledgement keeps the demand fence installed until requesting-cell
//! state is authoritative.

use super::reuse::{H1AvailabilityOutcome, ReuseCandidate, ReuseId};
use super::{
    AdmissionAction, DeliveryAckResult, DeliveryId, DemandId, DemandSnapshot, OriginAdmission,
    PermitId,
};
use crate::client::pool::cell::h1::H1Selection;
use crate::client::pool::cell::{
    AcquisitionEvent, AcquisitionResult, CellId, EstablishmentPermit, OriginCell,
};
use crate::sync::Arc;
use std::fmt;

/// Capacity or a borrowed HTTP/1 sender crossing to waiting demand.
enum AcquisitionPayload {
    /// Permit removed from admission but not yet represented by a lease.
    Capacity(PermitId),
    /// Provisional sender whose owning-cell reservation must revalidate.
    BorrowedH1 {
        /// Reuse operation that selected this sender.
        reuse_id: ReuseId,
        /// Cell that owns the sender and local reservation.
        connection_cell: CellId,
        /// Sender owner with cancellation fallback.
        candidate: ReuseCandidate,
    },
}

/// Payload after every connection-owning cell-side fallible transition has completed.
enum MaterializedPayload {
    /// Establishment authority ready to move into the requesting cell.
    Capacity(EstablishmentPermit),
    /// Selected sender ready to move into the requesting cell.
    BorrowedH1 {
        /// Reuse operation that selected this sender.
        reuse_id: ReuseId,
        /// Cell that owns the installed connection record.
        connection_cell: CellId,
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
    requesting_cell: CellId,
    /// Exact requesting-cell demand generation that must revalidate the
    /// payload.
    demand: DemandId,
    /// Payload ownership before materialization, after materialization, or after transfer.
    state: DeliveryGuardState,
}

/// Ownership phase of one admission-to-cell delivery.
enum DeliveryGuardState {
    /// Admission removed the payload, but connection-owning cell-side work may still fail.
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
        requesting_cell: CellId,
        demand: DemandId,
        permit: PermitId,
    ) -> Self {
        Self {
            origin,
            delivery,
            requesting_cell,
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
        requesting_cell: CellId,
        demand: DemandId,
        reuse_id: ReuseId,
        connection_cell: CellId,
        candidate: ReuseCandidate,
    ) -> Self {
        Self {
            origin,
            delivery,
            requesting_cell,
            demand,
            state: DeliveryGuardState::Undelivered {
                payload: Some(AcquisitionPayload::BorrowedH1 {
                    reuse_id,
                    connection_cell,
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
            .delivery_is_current(self.delivery, &self.requesting_cell, self.demand)
    }

    /// Materializes owning-cell state and attempts one requesting-cell delivery.
    pub(super) fn deliver_once(mut self) -> Option<AdmissionAction> {
        if !self.materialize() {
            return None;
        }
        match self.origin.cell(&self.requesting_cell) {
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
                connection_cell,
                candidate,
            } => match candidate.commit() {
                Ok(selection) => MaterializedPayload::BorrowedH1 {
                    reuse_id,
                    connection_cell,
                    selection,
                },
                Err(candidate) => {
                    drop(candidate);
                    let next = OriginAdmission::finish_delivery(
                        &self.origin,
                        self.delivery,
                        &self.requesting_cell,
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
                connection_cell,
                selection,
            } => (
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)),
                DeliveryKind::BorrowedH1 {
                    reuse_id,
                    connection_cell,
                },
            ),
        };
        self.state = DeliveryGuardState::Disarmed;
        (
            event,
            DeliveryAck {
                origin: self.origin.clone(),
                delivery: self.delivery,
                requesting_cell: self.requesting_cell.clone(),
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
                    &self.requesting_cell,
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
                        &self.requesting_cell,
                        None,
                        result,
                    )
                }
                Some(MaterializedPayload::BorrowedH1 {
                    reuse_id,
                    connection_cell,
                    selection,
                }) => {
                    let owning_cell = self.origin.cell(&connection_cell);
                    drop(selection);
                    let outcome = match owning_cell {
                        Some(owning_cell) => H1AvailabilityOutcome::reported(
                            connection_cell,
                            owning_cell.cancel_h1_reuse(reuse_id),
                        ),
                        None => H1AvailabilityOutcome::expired(connection_cell),
                    };
                    OriginAdmission::finish_borrow_delivery(
                        &self.origin,
                        reuse_id,
                        self.delivery,
                        &self.requesting_cell,
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
            .field("requesting_cell", &self.requesting_cell)
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
    requesting_cell: CellId,
    /// Successor demand to publish after the old generation finishes.
    successor: Option<DemandSnapshot>,
    /// Payload-specific connection-owning cell completion still owed by this guard.
    kind: Option<DeliveryKind>,
}

/// Terminal work required after the requesting cell takes ownership.
enum DeliveryKind {
    /// Only the admission delivery fence remains to acknowledge.
    Capacity,
    /// The connection-owning cell must learn whether sender transfer succeeded.
    BorrowedH1 {
        /// Reuse operation completed by this acknowledgement.
        reuse_id: ReuseId,
        /// Cell whose local reuse reservation must complete.
        connection_cell: CellId,
    },
}

impl DeliveryAck {
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
                    &self.requesting_cell,
                    None,
                    result,
                )
            }
            DeliveryKind::BorrowedH1 {
                reuse_id,
                connection_cell,
            } => {
                let rejected = returned_events.is_some();
                let owning_cell = rejected
                    .then(|| self.origin.cell(&connection_cell))
                    .flatten();
                drop(returned_events);
                let rejected_outcome = rejected.then(|| match owning_cell {
                    Some(owning_cell) => H1AvailabilityOutcome::reported(
                        connection_cell.clone(),
                        owning_cell.cancel_h1_reuse(reuse_id),
                    ),
                    None => H1AvailabilityOutcome::expired(connection_cell.clone()),
                });
                OriginAdmission::finish_borrow_delivery(
                    &self.origin,
                    reuse_id,
                    self.delivery,
                    &self.requesting_cell,
                    result,
                    (!rejected).then_some(connection_cell),
                    rejected_outcome,
                )
            }
        }
    }
}

impl fmt::Debug for DeliveryAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliveryAck")
            .field("delivery", &self.delivery)
            .field("requesting_cell", &self.requesting_cell)
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

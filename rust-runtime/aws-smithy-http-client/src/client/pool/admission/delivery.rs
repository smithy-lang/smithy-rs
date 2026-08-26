/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-to-one acquisition payload delivery across admission and cell locks.
//!
//! Admission reserves one demand episode and moves either bounded capacity or
//! a provisional HTTP/1 sender into a [`DeliveryGuard`]. The guard
//! materializes source-owned state before the target lock is taken, then owns
//! the payload until the target accepts it. Its acknowledgement keeps the
//! demand fence installed until target state is authoritative.

use super::claims::{ClaimCandidate, ClaimId, SourceOutcome};
use super::{
    AdmissionAction, DeliveryId, DemandId, DemandSnapshot, OriginAdmission, PermitId,
    TargetAckResult,
};
use crate::client::pool::cell::h1::H1Selection;
use crate::client::pool::cell::{
    AcquisitionEvent, AcquisitionResult, CellId, EstablishmentPermit, OriginCell,
};
use crate::sync::Arc;
use std::fmt;

/// Capacity or a source-claimed H1 sender crossing to one target demand.
enum AcquisitionPayload {
    /// Permit removed from admission but not yet represented by a lease.
    Capacity(PermitId),
    /// Provisional sender whose source claim must revalidate before delivery.
    BorrowedH1 {
        claim: ClaimId,
        source: CellId,
        candidate: ClaimCandidate,
    },
}

/// Payload after every source-side fallible transition has completed.
enum MaterializedPayload {
    /// Establishment authority ready to move into target state.
    Capacity(EstablishmentPermit),
    /// Selected sender and claim endpoint ready to move into target state.
    BorrowedH1 {
        claim: ClaimId,
        source: CellId,
        selection: H1Selection,
    },
}

/// One acquisition payload and its admission-owned delivery fence.
///
/// Dropping an undelivered guard refunnels its payload before making the
/// target demand schedulable again. Once committed, [`DeliveryAck`] owns fence
/// completion and the target cell owns the acquisition event.
pub(in crate::client::pool) struct DeliveryGuard {
    /// Admission authority that owns the delivery fence and fallback.
    origin: Arc<OriginAdmission>,
    /// Never-reused identity of this one crossing.
    delivery: DeliveryId,
    /// Cell selected while admission was locked.
    target: CellId,
    /// Exact target-demand episode that must revalidate the payload.
    demand: DemandId,
    /// Payload ownership before materialization, after materialization, or after transfer.
    state: DeliveryGuardState,
}

/// Ownership phase of one admission-to-cell delivery.
enum DeliveryGuardState {
    /// Admission removed the payload, but source-side work may still fail.
    Undelivered {
        /// Payload still owned by this fallback.
        payload: Option<AcquisitionPayload>,
        /// Demand acknowledgement used if the guard is dropped.
        on_drop: TargetAckResult,
    },
    /// Source-side work completed; the target may now reserve its waiter.
    Materialized {
        /// Target-ready payload still owned by this fallback.
        payload: Option<MaterializedPayload>,
        /// Demand acknowledgement used if the guard is dropped.
        on_drop: TargetAckResult,
    },
    /// Payload and fallback responsibility moved to target-owned state.
    Disarmed,
}

impl DeliveryGuard {
    /// Creates a delivery for one permit removed from admission.
    pub(super) fn capacity(
        origin: Arc<OriginAdmission>,
        delivery: DeliveryId,
        target: CellId,
        demand: DemandId,
        permit: PermitId,
    ) -> Self {
        Self {
            origin,
            delivery,
            target,
            demand,
            state: DeliveryGuardState::Undelivered {
                payload: Some(AcquisitionPayload::Capacity(permit)),
                on_drop: TargetAckResult::RetrySameResidence,
            },
        }
    }

    /// Creates a delivery for one source-claimed provisional sender.
    pub(super) fn borrowed_h1(
        origin: Arc<OriginAdmission>,
        delivery: DeliveryId,
        target: CellId,
        demand: DemandId,
        claim: ClaimId,
        source: CellId,
        candidate: ClaimCandidate,
    ) -> Self {
        Self {
            origin,
            delivery,
            target,
            demand,
            state: DeliveryGuardState::Undelivered {
                payload: Some(AcquisitionPayload::BorrowedH1 {
                    claim,
                    source,
                    candidate,
                }),
                on_drop: TargetAckResult::RetrySameResidence,
            },
        }
    }

    /// Returns the demand episode fenced by this delivery.
    pub(in crate::client::pool) fn demand(&self) -> DemandId {
        self.demand
    }

    /// Returns whether admission still recognizes this delivery fence.
    #[cfg(test)]
    pub(in crate::client::pool) fn is_current(&self) -> bool {
        self.origin
            .delivery_is_current(self.delivery, &self.target, self.demand)
    }

    /// Materializes source-owned state and attempts one target delivery.
    pub(super) fn deliver_once(mut self) -> Option<AdmissionAction> {
        if !self.materialize() {
            return None;
        }
        match self.origin.target(&self.target) {
            Some(target) => OriginCell::receive_delivery(&target, self),
            None => self.reject(None),
        }
    }

    /// Converts every fallible source-side payload before target reservation.
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
                claim,
                source,
                candidate,
            } => match candidate.commit() {
                Ok(selection) => MaterializedPayload::BorrowedH1 {
                    claim,
                    source,
                    selection,
                },
                Err(candidate) => {
                    drop(candidate);
                    let next = OriginAdmission::finish_delivery(
                        &self.origin,
                        self.delivery,
                        &self.target,
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

    /// Materializes this guard before tests manually split the target
    /// reservation and installation transitions.
    #[cfg(test)]
    pub(in crate::client::pool) fn materialize_for_test(&mut self) -> bool {
        self.materialize()
    }

    /// Moves the materialized payload into a target-owned event.
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
                claim,
                source,
                selection,
            } => (
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)),
                DeliveryKind::BorrowedH1 { claim, source },
            ),
        };
        self.state = DeliveryGuardState::Disarmed;
        (
            event,
            DeliveryAck {
                origin: self.origin.clone(),
                delivery: self.delivery,
                target: self.target.clone(),
                successor,
                kind: Some(kind),
            },
        )
    }

    /// Refunnels this delivery after target revalidation rejects it.
    pub(in crate::client::pool) fn reject(
        mut self,
        successor: Option<DemandSnapshot>,
    ) -> Option<AdmissionAction> {
        let state = std::mem::replace(&mut self.state, DeliveryGuardState::Disarmed);
        self.finish_state(state, TargetAckResult::Rejected { successor })
    }

    /// Resolves payload fallback before closing the admission-owned fence.
    fn finish_state(
        &self,
        state: DeliveryGuardState,
        result: TargetAckResult,
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
                    &self.target,
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
                        &self.target,
                        None,
                        result,
                    )
                }
                Some(MaterializedPayload::BorrowedH1 {
                    claim,
                    source,
                    selection,
                }) => {
                    let source_cell = self.origin.target(&source);
                    drop(selection);
                    let outcome = match source_cell {
                        Some(source_cell) => {
                            SourceOutcome::reported(source, source_cell.cancel_h1_claim(claim))
                        }
                        None => SourceOutcome::expired(source),
                    };
                    OriginAdmission::finish_borrow_delivery(
                        &self.origin,
                        claim,
                        self.delivery,
                        &self.target,
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
            .field("target", &self.target)
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

/// Target-side acknowledgement after an event becomes target-owned.
pub(in crate::client::pool) struct DeliveryAck {
    /// Admission authority that owns the outstanding fence.
    origin: Arc<OriginAdmission>,
    /// Delivery fence closed by this acknowledgement.
    delivery: DeliveryId,
    /// Cell that now owns or rejected the acquisition event.
    target: CellId,
    /// Target demand to publish after the old episode finishes.
    successor: Option<DemandSnapshot>,
    /// Payload-specific source completion still owed by this guard.
    kind: Option<DeliveryKind>,
}

/// Terminal work required after the target takes ownership.
enum DeliveryKind {
    /// Only the admission delivery fence remains to acknowledge.
    Capacity,
    /// The source claim endpoint must also learn whether transfer won.
    BorrowedH1 { claim: ClaimId, source: CellId },
}

impl DeliveryAck {
    /// Acknowledges that target state accepted the acquisition event.
    pub(in crate::client::pool) fn accept(mut self) -> Option<AdmissionAction> {
        let kind = self
            .kind
            .take()
            .expect("delivery acknowledgement completed more than once");
        let successor = self.successor.take();
        self.finish(kind, TargetAckResult::Accepted { successor }, None)
    }

    /// Refunnels events rejected after target reservation.
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
            TargetAckResult::Rejected { successor },
            Some(returned_events),
        )
    }

    /// Completes the payload-specific source endpoint and delivery fence.
    fn finish(
        &self,
        kind: DeliveryKind,
        result: TargetAckResult,
        returned_events: Option<[Option<AcquisitionEvent>; 2]>,
    ) -> Option<AdmissionAction> {
        match kind {
            DeliveryKind::Capacity => {
                drop(returned_events);
                OriginAdmission::finish_delivery(
                    &self.origin,
                    self.delivery,
                    &self.target,
                    None,
                    result,
                )
            }
            DeliveryKind::BorrowedH1 { claim, source } => {
                let rejected = returned_events.is_some();
                let source_cell = rejected.then(|| self.origin.target(&source)).flatten();
                drop(returned_events);
                let rejected_outcome = rejected.then(|| match source_cell {
                    Some(source_cell) => {
                        SourceOutcome::reported(source.clone(), source_cell.cancel_h1_claim(claim))
                    }
                    None => SourceOutcome::expired(source.clone()),
                });
                OriginAdmission::finish_borrow_delivery(
                    &self.origin,
                    claim,
                    self.delivery,
                    &self.target,
                    result,
                    (!rejected).then_some(source),
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
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl Drop for DeliveryAck {
    fn drop(&mut self) {
        let Some(kind) = self.kind.take() else {
            return;
        };
        let successor = self.successor.take();
        let next = self.finish(kind, TargetAckResult::Accepted { successor }, None);
        OriginAdmission::drive(next);
    }
}

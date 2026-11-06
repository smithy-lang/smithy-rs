/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Bounded operation vocabulary for one acquisition queue.

use crate::client::pool::admission::ProtocolRequirement;

/// Number of harness slots that may each hold one live acquisition attempt.
pub(in crate::client::pool::cell::waiters) const SLOTS: u8 = 8;

/// Maximum number of reserved deliveries left uncommitted at once.
pub(in crate::client::pool::cell::waiters) const CROSSINGS: usize = 4;

/// Whether the cell has an admission bound.
///
/// Production never mixes the two within one cell, so each sequence runs in
/// one lane. Admission operations are meaningless in the unbounded lane and
/// are never generated for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::cell::waiters) enum Lane {
    /// Registrations join the FIFO and wait for a delivery.
    Bounded,
    /// Registrations start ready to establish; no demand is published.
    Unbounded,
}

impl Lane {
    pub(in crate::client::pool::cell::waiters) const ALL: [Self; 2] =
        [Self::Bounded, Self::Unbounded];

    pub(in crate::client::pool::cell::waiters) fn is_bounded(self) -> bool {
        self == Self::Bounded
    }
}

/// Protocol requirement selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::cell::waiters) enum Requirement {
    H1Required,
    H1Compatible,
    H2Required,
}

impl Requirement {
    pub(in crate::client::pool::cell::waiters) fn into_protocol(self) -> ProtocolRequirement {
        match self {
            Self::H1Required => ProtocolRequirement::H1Required,
            Self::H1Compatible => ProtocolRequirement::H1Compatible,
            Self::H2Required => ProtocolRequirement::H2Required,
        }
    }
}

/// Which demand generation an admission-side operation names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::cell::waiters) enum DemandSelector {
    /// The generation currently published for the FIFO head.
    Current,
    /// A retired generation, or one that was never issued.
    Stale,
}

/// Which peer-route cutoff an HTTP/2 activation offer carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::cell::waiters) enum CutoffSelector {
    /// A local activation with no route priority.
    None,
    /// The cutoff the cell would record when attaching a peer route now.
    RouteCutoff,
    /// The identity of one harness slot, or none if that slot is empty.
    ///
    /// Skipped, as production would not offer, when no compatible attempt
    /// at or before that identity exists.
    Slot(u8),
}

/// One step a caller of the acquisition queue may take.
///
/// Each operation is skipped, and counted as skipped, when its slot or
/// crossing does not hold what the operation needs. The harness never issues
/// an operation the queue documents as a caller-contract panic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::client::pool::cell::waiters) enum Operation {
    /// Registers a new attempt in an empty slot.
    Register { slot: u8, requirement: Requirement },
    /// Cancels the attempt in a slot.
    Cancel { slot: u8 },
    /// Polls the attempt in a slot with that slot's counting waker.
    Poll { slot: u8 },
    /// The establishment task reports its first poll.
    StartEstablishment { slot: u8 },
    /// The establishment task reports its terminal result.
    CommitEstablishment { slot: u8 },
    /// Admission reserves the FIFO head for a delivery naming a demand.
    ReserveDelivery { demand: DemandSelector },
    /// A reserved delivery arrives carrying capacity.
    CommitCapacity { crossing: u8 },
    /// A reserved delivery arrives carrying a borrowed HTTP/1 sender.
    CommitBorrowedH1 { crossing: u8 },
    /// A cell-owned HTTP/1 sender returns.
    OfferReturnedH1,
    /// An HTTP/2 generation offers an activation.
    OfferH2Activation { cutoff: CutoffSelector },
    /// Admission acknowledges a peer route for the head demand.
    SupersedeDemand { demand: DemandSelector },
    /// Drives every live attempt to a delivered outcome and asserts emptiness.
    Quiesce,
}

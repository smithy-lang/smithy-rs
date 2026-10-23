/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Bounded operation vocabulary and origin profiles.

use crate::client::pool::admission::ProtocolRequirement;
use crate::client::pool::partition::{EligibilityGroup, PartitionId};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Fixed partition layout and connection limit for one sequence lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum Profile {
    /// One bounded partition with no peers.
    ///
    /// Peer borrow and route paths must stay inert. Reclaim may still close
    /// the partition's own idle connection for protocol-incompatible demand.
    Single,
    /// Four partitions sharing one eligibility group.
    ///
    /// Every partition may borrow and route to every other. This is the
    /// partitioned pool without interface bindings.
    SharedGroup,
    /// Four partitions in two eligibility groups of two.
    ///
    /// Supply is group-scoped while permits and reclaim stay origin-wide.
    TwoGroups,
    /// Four partitions, each its own eligibility group.
    ///
    /// No peer reuse is possible; reclaim remains origin-wide.
    PartitionLocal,
}

impl Profile {
    pub(in crate::client::pool::admission) const ALL: [Self; 4] = [
        Self::Single,
        Self::SharedGroup,
        Self::TwoGroups,
        Self::PartitionLocal,
    ];

    /// Returns the number of partitions in this profile.
    pub(in crate::client::pool::admission) const fn partitions(self) -> u8 {
        match self {
            Self::Single => 1,
            Self::SharedGroup | Self::TwoGroups | Self::PartitionLocal => 4,
        }
    }

    /// Returns the bounded connection limit for the origin.
    pub(in crate::client::pool::admission) fn limit(self) -> NonZeroUsize {
        NonZeroUsize::new(match self {
            Self::Single => 1,
            Self::SharedGroup | Self::TwoGroups | Self::PartitionLocal => 2,
        })
        .expect("profile limit is nonzero")
    }

    /// Maps a partition selector to its identity.
    pub(in crate::client::pool::admission) fn partition(self, selector: u8) -> PartitionId {
        match self {
            Self::Single => PartitionId::ANONYMOUS,
            _ => PartitionId::from_index((selector % self.partitions()) as usize),
        }
    }

    /// Returns every partition identity in this profile.
    pub(in crate::client::pool::admission) fn all_partitions(self) -> Vec<PartitionId> {
        (0..self.partitions())
            .map(|selector| self.partition(selector))
            .collect()
    }

    /// Returns the eligibility group for one partition.
    pub(in crate::client::pool::admission) fn group_of(
        self,
        partition: PartitionId,
    ) -> EligibilityGroup {
        match self {
            Self::Single | Self::PartitionLocal => EligibilityGroup::Partition(partition),
            Self::SharedGroup => EligibilityGroup::NetworkInterface(None),
            Self::TwoGroups => {
                let index = self
                    .all_partitions()
                    .iter()
                    .position(|candidate| *candidate == partition)
                    .expect("partition belongs to the profile");
                let interface: Arc<str> = if index < 2 {
                    Arc::from("interface-a")
                } else {
                    Arc::from("interface-b")
                };
                EligibilityGroup::NetworkInterface(Some(interface))
            }
        }
    }

    /// Returns every distinct eligibility group in this profile.
    pub(in crate::client::pool::admission) fn groups(self) -> Vec<EligibilityGroup> {
        let mut groups: Vec<_> = self
            .all_partitions()
            .into_iter()
            .map(|partition| self.group_of(partition))
            .collect();
        groups.sort();
        groups.dedup();
        groups
    }
}

/// Protocol requirement selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum Requirement {
    H1Required,
    H1Compatible,
    H2Required,
}

impl From<Requirement> for ProtocolRequirement {
    fn from(requirement: Requirement) -> Self {
        match requirement {
            Requirement::H1Required => Self::H1Required,
            Requirement::H1Compatible => Self::H1Compatible,
            Requirement::H2Required => Self::H2Required,
        }
    }
}

/// Requesting-cell verdict on a delivered assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum Outcome {
    Accepted,
    Refused,
    Retry,
}

/// Supplier-cell reply to an HTTP/1 idle probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum ProbeReply {
    /// The supplier had an idle sender; the match resolves now.
    IdleFound,
    /// The supplier was busy and remains live.
    Busy,
    /// The supplier cell no longer exists.
    Expired,
}

/// Supplier-cell reply to an HTTP/1 exact reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum ReservationReply {
    /// An idle sender was taken; the match resolves now.
    Candidate,
    /// The reservation was installed to intercept the next return.
    Installed,
    /// The supplier refused the reservation and remains live.
    Rejected,
    /// The supplier cell no longer exists.
    Expired,
}

/// One state transition attempted against admission.
///
/// Slot and partition fields are selectors normalized against the state
/// present when the operation executes. An inapplicable selector leaves the
/// state unchanged, which keeps every shrunk sequence valid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::client::pool::admission) enum Operation {
    // Demand.
    /// Publishes a fresh demand generation for one partition.
    PublishDemand {
        partition: u8,
        requirement: Requirement,
    },
    /// Republishes the current generation at the next version.
    BumpDemandVersion {
        partition: u8,
    },
    /// Publishes an inactive snapshot for the current generation.
    CancelDemand {
        partition: u8,
    },

    // Permits held outside admission, standing in for connection leases.
    TakePermit,
    ReturnPermit {
        slot: u8,
    },

    // Capacity delivery.
    PrepareCapacityDelivery,
    /// Settles one held assignment of any source.
    ///
    /// `stale_route` applies only to an HTTP/2 route and reports that the
    /// requesting cell found the selected generation gone.
    SettleAssignment {
        slot: u8,
        outcome: Outcome,
        successor: bool,
        stale_route: bool,
    },

    // HTTP/1 supply and retained matches.
    PublishH1Supply {
        partition: u8,
        returnable: bool,
        blocked: bool,
    },
    PrepareH1Match,
    H1ProbeReply {
        slot: u8,
        reply: ProbeReply,
    },
    H1ReservationReply {
        slot: u8,
        reply: ReservationReply,
    },
    /// A sender returned to a supplier holding an installed reservation.
    H1SenderReturned {
        slot: u8,
        accepted: bool,
    },
    /// Runs one queued cancellation crossing.
    H1CancelStep,

    // HTTP/2 supply, routes, and reclaim.
    PublishH2Supply {
        partition: u8,
        fresh_generation: bool,
        idle: bool,
    },
    PublishH2Unavailable {
        partition: u8,
    },
    PrepareH2Route,
    PrepareH2Reclaim,
    SettleH2Reclaim {
        closed: bool,
    },

    // Liveness.
    /// Drains admission the way production would and checks for stalls.
    Quiesce,
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Source-cell endpoint for bounded-origin HTTP/1 claims.
//!
//! Admission owns claim scheduling and target identity. A source cell owns the
//! smaller question that must be serialized with sender return: whether one
//! claim is waiting to intercept a return, already owns a provisional sender,
//! or has completed. The fairness bit is independent of claim residence
//! because an HTTP/2-only local head cannot consume an HTTP/1 turn yet.

use super::super::admission::claims::ClaimId;

/// One source cell's claim endpoint and cross-cell fairness debt.
#[derive(Debug, Default)]
pub(super) struct SourceClaimSlot {
    /// Residence of the source endpoint for its current claim.
    state: SourceClaimState,
    /// Whether the next usable local HTTP/1 turn belongs to this source.
    local_turn_owed: bool,
}

impl SourceClaimSlot {
    /// Installs a claim that will intercept a future reusable return.
    pub(super) fn install(&mut self, claim: ClaimId) -> bool {
        if !matches!(self.state, SourceClaimState::Available) {
            return false;
        }
        self.state = SourceClaimState::Installed(claim);
        true
    }

    /// Installs a claim that has already extracted an idle sender.
    pub(super) fn install_resolving(&mut self, claim: ClaimId) -> bool {
        if !matches!(self.state, SourceClaimState::Available) {
            return false;
        }
        self.state = SourceClaimState::Resolving(claim);
        true
    }

    /// Reserves the next reusable return for an installed claim.
    pub(super) fn intercept_return(&mut self) -> Option<ClaimId> {
        let SourceClaimState::Installed(claim) = self.state else {
            return None;
        };
        self.state = SourceClaimState::Resolving(claim);
        Some(claim)
    }

    /// Clears a matching claim without earning a local fairness turn.
    pub(super) fn reject(&mut self, claim: ClaimId) -> bool {
        if !self.names(claim) {
            return false;
        }
        self.state = SourceClaimState::Available;
        true
    }

    /// Completes an irreversible transfer and records any usable local turn.
    pub(super) fn complete_transfer(&mut self, claim: ClaimId, local_h1_demand: bool) -> bool {
        if !matches!(self.state, SourceClaimState::Resolving(current) if current == claim) {
            return false;
        }
        self.state = SourceClaimState::Available;
        self.local_turn_owed |= local_h1_demand;
        true
    }

    /// Returns whether a usable local turn currently excludes a peer claim.
    pub(super) fn blocks_peer_claim(&self, local_h1_demand: bool) -> bool {
        self.local_turn_owed && local_h1_demand
    }

    /// Consumes an owed turn when local HTTP/1 service wins.
    pub(super) fn consume_local_turn(&mut self) -> bool {
        if !self.local_turn_owed {
            return false;
        }
        self.local_turn_owed = false;
        true
    }

    /// Clears debt that can no longer be consumed by local demand.
    pub(super) fn clear_unused_turn(&mut self, local_h1_demand: bool) -> bool {
        if !self.local_turn_owed || local_h1_demand {
            return false;
        }
        self.local_turn_owed = false;
        true
    }

    /// Returns whether another source claim may be installed.
    pub(super) fn is_available(&self) -> bool {
        matches!(self.state, SourceClaimState::Available)
    }

    /// Returns whether this slot still names `claim`.
    pub(super) fn names(&self, claim: ClaimId) -> bool {
        matches!(
            self.state,
            SourceClaimState::Installed(current) | SourceClaimState::Resolving(current)
                if current == claim
        )
    }

    /// Checks relationships that are not already encoded by the state enum.
    pub(super) fn assert_consistent(&self, _source_supports_installed_claim: bool) {
        #[cfg(debug_assertions)]
        if matches!(self.state, SourceClaimState::Installed(_)) {
            assert!(
                _source_supports_installed_claim,
                "installed HTTP/1 claim had no externally owned source record to settle it"
            );
        }
    }

    #[cfg(test)]
    pub(super) fn local_turn_owed(&self) -> bool {
        self.local_turn_owed
    }
}

/// Authoritative source-cell residence of a nonterminal claim.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SourceClaimState {
    /// No claim may intercept a sender return.
    #[default]
    Available,
    /// The next reusable sender return is reserved for this claim.
    Installed(ClaimId),
    /// A provisional sender is outside the source lock for this claim.
    Resolving(ClaimId),
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;

    #[test]
    fn claim_slot_distinguishes_install_resolution_and_completion() {
        let claim = ClaimId::for_test(1);
        let mut slot = SourceClaimSlot::default();

        assert!(slot.install(claim));
        assert!(!slot.install(ClaimId::for_test(2)));
        assert_eq!(Some(claim), slot.intercept_return());
        assert!(slot.complete_transfer(claim, true));
        assert!(slot.local_turn_owed());
        assert!(slot.is_available());
    }

    #[test]
    fn rejection_does_not_manufacture_a_fairness_turn() {
        let claim = ClaimId::for_test(1);
        let mut slot = SourceClaimSlot::default();

        assert!(slot.install_resolving(claim));
        assert!(slot.reject(claim));
        assert!(!slot.local_turn_owed());
    }

    #[test]
    fn owed_turn_blocks_only_while_local_h1_demand_can_use_it() {
        let claim = ClaimId::for_test(1);
        let mut slot = SourceClaimSlot::default();
        assert!(slot.install_resolving(claim));
        assert!(slot.complete_transfer(claim, true));

        assert!(slot.blocks_peer_claim(true));
        assert!(!slot.blocks_peer_claim(false));
        assert!(slot.clear_unused_turn(false));
        assert!(!slot.blocks_peer_claim(true));
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/1 connection records and exclusive request-sender ownership.
//!
//! Every installed connection remains in its connection-owning [`H1Records`]
//! until logical close. While the record is live, its exclusive sender exists
//! in exactly one place: inside an `Idle` record or in one external owner while
//! the record is `Selected` or `Reserved`. Transitions move the sender; they
//! never copy it. The residence records that ownership:
//!
//! ```text
//! completed handshake -- install_selected --------------------> Selected
//! test fixture ------- install_idle ---------------------------> Idle(sender)
//!
//! Idle(sender) -- select_idle --------------------------------> Selected
//! Idle(sender) -- take_idle_for_reuse ------------------------> Reserved
//! Selected ---- reserve_for_reuse ----------------------------> Reserved
//! Reserved ---- commit_return_to_waiter ----------------------> Selected
//! Selected ---- return_idle ----------------------------------> Idle(sender)
//! Reserved ---- return_idle ----------------------------------> Idle(sender)
//!
//! Idle(sender) ------------ begin_close ----------------------> Closing
//! Selected / Reserved ---- begin_close / close_owned --------> Closing
//! Closing -------- finish_close ------------------------------> removed
//! ```
//!
//! Outside the lock, ownership moves through values whose drop behavior is
//! specific to the protocol phase:
//!
//! ```text
//! H1Selection
//!   |-- dropped ------------------------------> return to connection-owning cell
//!   |-- retire_connection --------------------------------> logical close
//!   `-- request accepted --> H1Exchange
//!                              |-- incomplete or dropped --> logical close
//!                              `-- offer_for_reuse --------> return to connection-owning cell
//!
//! cross-cell reuse --> ProvisionalH1
//!                        |-- borrow ----------------------> H1Selection
//!                        |-- reclaim ---------------------> logical close
//!                        `-- rejected or dropped ---------> return to connection-owning cell
//! ```
//!
//! [`H1Selection`], [`H1Exchange`], and [`ProvisionalH1`] are the
//! sender-owning values outside the cell lock. Their drop behavior returns or
//! retires the sender so a cancellation cannot leave a record with no terminal
//! owner. They retain the owning cell weakly because a selected sender may
//! temporarily be the ready result in that same cell; cell teardown makes the
//! fallback close the connection directly.

use super::super::admission::reuse::{
    H1AvailabilitySnapshot, PreparedReuseInstall, ReuseCandidate, ReuseId, ReuseInstallResult,
};
use super::super::admission::{AdmissionAction, OriginAdmission};
use super::super::connection::{CloseReason, ConnectionState};
use super::{AcquisitionResult, OriginCell, ReuseInstall};
use crate::sync::{Arc, Weak};
use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::task::{Context, Poll};
use std::time::SystemTime;

use aws_smithy_types::body::SdkBody;

/// Exclusive request handle for one Hyper HTTP/1 client connection.
///
/// Hyper creates one `SendRequest<SdkBody>` handle when the client handshake
/// completes. Pool code calls this handle the HTTP/1 sender. Moving it between
/// residence guards transfers authority to send the next request; it does not
/// move the socket or protocol driver. The test-only variant exercises those
/// ownership transitions without running Hyper.
pub(in crate::client::pool) enum H1Sender {
    /// Hyper's exclusive HTTP/1 request sender.
    Hyper(hyper::client::conn::http1::SendRequest<SdkBody>),
    /// Synthetic sender identity used only by ownership tests.
    #[cfg(test)]
    Test(u64),
}

impl H1Sender {
    /// Wraps a sender returned by a successful Hyper HTTP/1 handshake.
    pub(in crate::client::pool) fn from_hyper(
        sender: hyper::client::conn::http1::SendRequest<SdkBody>,
    ) -> Self {
        Self::Hyper(sender)
    }

    /// Returns the Hyper sender for readiness and dispatch.
    ///
    /// # Panics
    ///
    /// Panics when a test-only sender reaches the real dispatch path.
    pub(in crate::client::pool) fn hyper_mut(
        &mut self,
    ) -> &mut hyper::client::conn::http1::SendRequest<SdkBody> {
        match self {
            Self::Hyper(sender) => sender,
            #[cfg(test)]
            Self::Test(_) => panic!("test HTTP/1 sender reached Hyper dispatch"),
        }
    }

    /// Returns whether Hyper already permits another request.
    fn is_ready(&self) -> bool {
        match self {
            Self::Hyper(sender) => sender.is_ready(),
            #[cfg(test)]
            Self::Test(_) => panic!("test HTTP/1 sender reached Hyper readiness"),
        }
    }

    /// Creates a synthetic sender for state-machine tests.
    #[cfg(test)]
    pub(in crate::client::pool) fn test(id: u64) -> Self {
        Self::Test(id)
    }

    /// Returns the synthetic sender identity.
    #[cfg(test)]
    pub(super) fn test_id(&self) -> u64 {
        match self {
            Self::Test(id) => *id,
            Self::Hyper(_) => panic!("Hyper sender used in a synthetic ownership test"),
        }
    }
}

impl fmt::Debug for H1Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyper(_) => f.write_str("H1Sender::Hyper"),
            #[cfg(test)]
            Self::Test(id) => f.debug_tuple("H1Sender::Test").field(id).finish(),
        }
    }
}

/// Local reservation for one cross-cell reuse operation and its fairness debt.
///
/// Reservation residence and fairness debt are separate because a completed
/// transfer releases the reservation immediately, while the debt remains until
/// later local service or the disappearance of compatible local demand.
#[derive(Debug, Default)]
pub(super) struct H1ReuseReservation {
    /// Residence of the current reuse reservation.
    state: H1ReuseReservationState,
    /// Whether this cell's next usable HTTP/1 turn must remain local after the
    /// operation that earned it has completed.
    local_turn_owed: bool,
}

/// Authoritative residence of one cell-local reuse reservation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum H1ReuseReservationState {
    /// No reuse operation may intercept a sender return.
    #[default]
    Available,
    /// The next reusable sender return is reserved for this reuse operation.
    Installed(ReuseId),
    /// A provisional sender is outside the connection-owning cell lock for this reuse operation.
    Resolving(ReuseId),
}

impl H1ReuseReservation {
    /// Installs a reuse operation that will intercept a future reusable return.
    pub(super) fn install(&mut self, reuse_id: ReuseId) -> bool {
        if !matches!(self.state, H1ReuseReservationState::Available) {
            return false;
        }
        self.state = H1ReuseReservationState::Installed(reuse_id);
        true
    }

    /// Installs a reuse operation that has already extracted an idle sender.
    pub(super) fn install_resolving(&mut self, reuse_id: ReuseId) -> bool {
        if !matches!(self.state, H1ReuseReservationState::Available) {
            return false;
        }
        self.state = H1ReuseReservationState::Resolving(reuse_id);
        true
    }

    /// Reserves the next reusable return for an installed reuse operation.
    pub(super) fn intercept_return(&mut self) -> Option<ReuseId> {
        let H1ReuseReservationState::Installed(reuse_id) = self.state else {
            return None;
        };
        self.state = H1ReuseReservationState::Resolving(reuse_id);
        Some(reuse_id)
    }

    /// Clears a matching reuse operation without earning a local fairness turn.
    pub(super) fn reject(&mut self, reuse_id: ReuseId) -> bool {
        if !self.names(reuse_id) {
            return false;
        }
        self.state = H1ReuseReservationState::Available;
        true
    }

    /// Completes an irreversible transfer and records any usable local turn.
    pub(super) fn complete_transfer(&mut self, reuse_id: ReuseId, local_h1_demand: bool) -> bool {
        if !matches!(self.state, H1ReuseReservationState::Resolving(current) if current == reuse_id)
        {
            return false;
        }
        self.state = H1ReuseReservationState::Available;
        self.local_turn_owed |= local_h1_demand;
        true
    }

    /// Returns whether a usable local turn currently excludes a peer reuse operation.
    pub(super) fn blocks_peer_reuse(&self, local_h1_demand: bool) -> bool {
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

    /// Returns whether another reuse reservation may be installed.
    pub(super) fn is_available(&self) -> bool {
        matches!(self.state, H1ReuseReservationState::Available)
    }

    /// Returns whether this reservation still names the given operation.
    pub(super) fn names(&self, reuse_id: ReuseId) -> bool {
        matches!(
            self.state,
            H1ReuseReservationState::Installed(current) | H1ReuseReservationState::Resolving(current)
                if current == reuse_id
        )
    }

    /// Checks relationships that are not already encoded by the state enum.
    pub(super) fn assert_consistent(&self, _supports_installed_reuse: bool) {
        #[cfg(debug_assertions)]
        {
            if std::thread::panicking() {
                return;
            }
            if matches!(self.state, H1ReuseReservationState::Installed(_)) {
                assert!(
                    _supports_installed_reuse,
                    "installed HTTP/1 reuse operation had no externally owned connection-owning cell record to settle it"
                );
            }
        }
    }

    #[cfg(test)]
    pub(super) fn local_turn_owed(&self) -> bool {
        self.local_turn_owed
    }
}

/// Records owned by one cell and the order of reusable senders.
#[derive(Debug, Default)]
pub(super) struct H1Records {
    /// Every installed record that has not completed logical close.
    records: HashMap<ConnectionId, H1Record>,
    /// Reusable records in return order; selection takes the newest sender.
    idle: VecDeque<ConnectionId>,
}

/// One cell-owned HTTP/1 connection record.
#[derive(Debug)]
struct H1Record {
    /// Shared logical and physical connection lifetime.
    connection: Arc<ConnectionState>,
    /// Location of the record's exclusive sender.
    residence: H1Residence,
}

/// Authoritative location of one exclusive HTTP/1 sender.
#[derive(Debug)]
enum H1Residence {
    /// The sender is stored in this record and available for local selection.
    Idle {
        /// Exclusive sender available for the next local request.
        sender: H1Sender,
        /// Maintenance deadline, absent when idle expiry is disabled.
        deadline: Option<SystemTime>,
    },
    /// A request-side [`H1Selection`] or [`H1Exchange`] owns the sender.
    Selected,
    /// A [`ProvisionalH1`] owns the sender for cross-cell reuse.
    Reserved,
    /// Logical close has started and no new selection or return may commit.
    Closing,
}

impl H1Records {
    /// Installs a fresh connection as selected by its launching acquisition.
    pub(super) fn install_selected(
        &mut self,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
    ) -> Result<OwnedH1, OwnedH1> {
        let owner = OwnedH1::new(connection, sender, false);
        if self.records.contains_key(&owner.id()) {
            return Err(owner);
        }
        self.records.insert(
            owner.id(),
            H1Record {
                connection: owner.connection.clone(),
                residence: H1Residence::Selected,
            },
        );
        self.assert_consistent();
        Ok(owner)
    }

    /// Installs a connection that completed without a live launching waiter.
    #[cfg(test)]
    pub(super) fn install_idle(
        &mut self,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
        deadline: Option<SystemTime>,
    ) -> Result<(), OwnedH1> {
        let owner = OwnedH1::new(connection, sender, true);
        if self.records.contains_key(&owner.id()) {
            return Err(owner);
        }
        let id = owner.id();
        let OwnedH1 {
            connection, sender, ..
        } = owner;
        self.records.insert(
            id,
            H1Record {
                connection,
                residence: H1Residence::Idle { sender, deadline },
            },
        );
        self.idle.push_back(id);
        self.assert_consistent();
        Ok(())
    }

    /// Takes the most recently returned idle sender for one request.
    pub(super) fn select_idle(&mut self) -> Option<OwnedH1> {
        let id = self.idle.pop_back()?;
        let record = self
            .records
            .get_mut(&id)
            .expect("idle HTTP/1 record disappeared");
        if !matches!(record.residence, H1Residence::Idle { .. }) {
            panic!("idle HTTP/1 order named a non-idle record");
        }
        let H1Residence::Idle { sender, .. } =
            std::mem::replace(&mut record.residence, H1Residence::Selected)
        else {
            unreachable!("HTTP/1 residence changed under the cell lock");
        };
        let owner = OwnedH1::new(record.connection.clone(), sender, true);
        self.assert_consistent();
        Some(owner)
    }

    /// Extracts the newest idle sender into provisional return residence.
    ///
    /// A return reuse uses this path so rejection can follow the same ordinary
    /// return fallback as a sender intercepted after an active exchange.
    pub(super) fn take_idle_for_reuse(&mut self) -> Option<OwnedH1> {
        let id = self.idle.pop_back()?;
        let record = self
            .records
            .get_mut(&id)
            .expect("idle HTTP/1 record disappeared");
        let H1Residence::Idle { sender, .. } =
            std::mem::replace(&mut record.residence, H1Residence::Reserved)
        else {
            panic!("idle HTTP/1 order named a non-idle record");
        };
        let owner = OwnedH1::new(record.connection.clone(), sender, true);
        self.assert_consistent();
        Some(owner)
    }

    /// Returns whether a sender could satisfy a connection-owning cell reuse now or on return.
    pub(super) fn has_returnable(&self) -> bool {
        self.records.values().any(|record| {
            matches!(
                record.residence,
                H1Residence::Idle { .. } | H1Residence::Selected | H1Residence::Reserved
            )
        })
    }

    /// Returns whether `owner` may still re-enter reusable return policy.
    pub(super) fn accepts_return(&self, owner: &OwnedH1) -> bool {
        self.records.get(&owner.id()).is_some_and(|record| {
            matches!(
                record.residence,
                H1Residence::Selected | H1Residence::Reserved
            )
        })
    }

    /// Returns whether an installed reuse still has a cell-local reservation.
    ///
    /// Logical close may move an externally owned sender to `Closing` before
    /// its return resolves the installed reuse, so reuse consistency is
    /// broader than current reuse eligibility.
    pub(super) fn supports_installed_reuse(&self) -> bool {
        self.records.values().any(|record| {
            matches!(
                record.residence,
                H1Residence::Selected | H1Residence::Reserved | H1Residence::Closing
            )
        })
    }

    /// Reserves an external sender for cross-cell reuse.
    pub(super) fn reserve_for_reuse(&mut self, owner: &OwnedH1) -> bool {
        let Some(record) = self.records.get_mut(&owner.id()) else {
            return false;
        };
        match record.residence {
            H1Residence::Selected => {
                record.residence = H1Residence::Reserved;
                self.assert_consistent();
                true
            }
            H1Residence::Reserved => true,
            H1Residence::Idle { .. } | H1Residence::Closing => false,
        }
    }

    /// Restores a returned sender to idle storage.
    pub(super) fn return_idle(
        &mut self,
        owner: OwnedH1,
        deadline: Option<SystemTime>,
    ) -> Result<(), OwnedH1> {
        let Some(record) = self.records.get_mut(&owner.id()) else {
            return Err(owner);
        };
        if !matches!(
            record.residence,
            H1Residence::Selected | H1Residence::Reserved
        ) {
            return Err(owner);
        }
        let id = owner.id();
        let OwnedH1 { sender, .. } = owner;
        record.residence = H1Residence::Idle { sender, deadline };
        self.idle.push_back(id);
        self.assert_consistent();
        Ok(())
    }

    /// Commits a selected or returning sender to a waiting request.
    pub(super) fn commit_return_to_waiter(&mut self, owner: &OwnedH1) -> bool {
        let Some(record) = self.records.get_mut(&owner.id()) else {
            return false;
        };
        match record.residence {
            H1Residence::Selected => true,
            H1Residence::Reserved => {
                record.residence = H1Residence::Selected;
                self.assert_consistent();
                true
            }
            H1Residence::Idle { .. } | H1Residence::Closing => false,
        }
    }

    /// Marks a selected or returning sender as closing.
    ///
    /// The caller still owns the sender and must complete logical close after
    /// releasing the cell lock.
    pub(super) fn close_owned(&mut self, owner: &OwnedH1) -> bool {
        let should_close = match self.records.get_mut(&owner.id()) {
            Some(record) => match record.residence {
                H1Residence::Selected | H1Residence::Reserved => {
                    record.residence = H1Residence::Closing;
                    true
                }
                H1Residence::Closing => false,
                H1Residence::Idle { .. } => {
                    panic!("externally owned HTTP/1 sender was recorded as idle")
                }
            },
            None => true,
        };
        self.assert_consistent();
        should_close
    }

    /// Starts close for a record named without its external sender.
    ///
    /// Idle close extracts the sender. Selected and returning records stay
    /// represented as `Closing` until their external owner comes back.
    pub(super) fn begin_close(&mut self, id: ConnectionId) -> Option<CloseRecord> {
        let record = self.records.get_mut(&id)?;
        let connection = record.connection.clone();
        let sender = match &record.residence {
            H1Residence::Idle { .. } => {
                let position = self
                    .idle
                    .iter()
                    .position(|candidate| *candidate == id)
                    .expect("idle HTTP/1 record was absent from idle order");
                self.idle.remove(position);
                let H1Residence::Idle { sender, .. } =
                    std::mem::replace(&mut record.residence, H1Residence::Closing)
                else {
                    unreachable!("HTTP/1 residence changed under the cell lock");
                };
                Some(sender)
            }
            H1Residence::Selected | H1Residence::Reserved => {
                record.residence = H1Residence::Closing;
                None
            }
            H1Residence::Closing => return None,
        };
        self.assert_consistent();
        Some(CloseRecord { connection, sender })
    }

    /// Removes a record after logical close and sender destruction complete.
    pub(super) fn finish_close(&mut self, id: ConnectionId) {
        let Some(record) = self.records.get(&id) else {
            return;
        };
        if !matches!(record.residence, H1Residence::Closing) {
            return;
        }
        self.records.remove(&id);
        self.assert_consistent();
    }

    /// Returns the installed record and idle counts.
    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize) {
        (self.records.len(), self.idle.len())
    }

    /// Returns the sole installed connection for focused dispatch tests.
    #[cfg(all(test, feature = "rt-tokio"))]
    pub(super) fn only_connection_for_test(&self) -> Arc<ConnectionState> {
        assert_eq!(
            1,
            self.records.len(),
            "expected exactly one installed HTTP/1 record"
        );
        self.records
            .values()
            .next()
            .expect("HTTP/1 record count changed under the cell lock")
            .connection
            .clone()
    }

    /// Returns idle records whose configured deadline has elapsed.
    pub(super) fn expired_idle(&self, now: SystemTime) -> Vec<ConnectionId> {
        self.idle
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.records.get(id).map(|record| &record.residence),
                    Some(H1Residence::Idle {
                        deadline: Some(deadline),
                        ..
                    }) if *deadline <= now
                )
            })
            .collect()
    }

    /// Returns the nearest configured deadline among reusable senders.
    pub(super) fn nearest_idle_deadline(&self) -> Option<SystemTime> {
        self.idle
            .iter()
            .filter_map(|id| {
                match &self
                    .records
                    .get(id)
                    .expect("idle HTTP/1 record disappeared")
                    .residence
                {
                    H1Residence::Idle { deadline, .. } => *deadline,
                    _ => unreachable!("idle HTTP/1 order named a non-idle record"),
                }
            })
            .min()
    }

    /// Returns every installed record identity for pool-wide shutdown.
    pub(super) fn connection_ids(&self) -> Vec<ConnectionId> {
        self.records.keys().copied().collect()
    }

    /// Checks that idle records and the idle order describe the same set.
    pub(super) fn assert_consistent(&self) {
        #[cfg(debug_assertions)]
        {
            if std::thread::panicking() {
                return;
            }
            let idle_records = self
                .records
                .values()
                .filter(|record| matches!(record.residence, H1Residence::Idle { .. }))
                .count();
            assert_eq!(
                idle_records,
                self.idle.len(),
                "HTTP/1 idle count did not match idle order"
            );
            let mut seen = HashMap::new();
            for id in &self.idle {
                assert!(
                    matches!(
                        self.records.get(id).map(|record| &record.residence),
                        Some(H1Residence::Idle { .. })
                    ),
                    "HTTP/1 idle order named a missing or non-idle record"
                );
                assert!(
                    seen.insert(*id, ()).is_none(),
                    "HTTP/1 idle order contained a duplicate record"
                );
            }
        }
    }
}

/// Exclusive sender and connection state detached from a connection-owning cell record.
#[derive(Debug)]
pub(super) struct OwnedH1 {
    /// Shared lifetime state for the physical connection.
    connection: Arc<ConnectionState>,
    /// The one Hyper HTTP/1 sender for the connection.
    sender: H1Sender,
    /// Whether this sender came from an already installed reusable record.
    reused: bool,
}

impl OwnedH1 {
    /// Creates detached sender ownership.
    fn new(connection: Arc<ConnectionState>, sender: H1Sender, reused: bool) -> Self {
        Self {
            connection,
            sender,
            reused,
        }
    }

    /// Returns the installed connection identity.
    pub(super) fn id(&self) -> ConnectionId {
        self.connection.id()
    }

    /// Returns whether this selection came from an existing reusable record.
    pub(super) fn is_reused(&self) -> bool {
        self.reused
    }

    /// Marks the next request as reusing an already established connection.
    pub(super) fn mark_reused(&mut self) {
        self.reused = true;
    }

    /// Returns the shared connection state.
    pub(super) fn connection(&self) -> &Arc<ConnectionState> {
        &self.connection
    }

    /// Returns the exclusive sender.
    pub(super) fn sender_mut(&mut self) -> &mut H1Sender {
        &mut self.sender
    }

    /// Returns the synthetic sender identity.
    #[cfg(test)]
    pub(super) fn test_sender_id(&self) -> u64 {
        self.sender.test_id()
    }
}

/// Returns an external sender to its connection-owning cell.
///
/// A sender whose cell was torn down closes its connection directly.
fn return_to_connection_cell(connection_cell: &Weak<OriginCell>, owner: OwnedH1) {
    if let Some(connection_cell) = connection_cell.upgrade() {
        OriginCell::return_h1_owner(&connection_cell, owner);
    } else {
        owner.connection().logical_close(CloseReason::PoolDropped);
        drop(owner);
    }
}

/// Retires an external sender through its connection-owning cell.
///
/// A sender whose cell was torn down closes its connection directly.
fn retire_at_connection_cell(
    connection_cell: &Weak<OriginCell>,
    owner: OwnedH1,
    reason: CloseReason,
) {
    if let Some(connection_cell) = connection_cell.upgrade() {
        OriginCell::retire_h1_owner(&connection_cell, owner, reason);
    } else {
        owner.connection().logical_close(reason);
        drop(owner);
    }
}

/// State detached when close begins for a record named by identity.
pub(super) struct CloseRecord {
    /// Connection whose logical lifetime close must end.
    pub(super) connection: Arc<ConnectionState>,
    /// Idle sender extracted for destruction, if close found one in the cell.
    pub(super) sender: Option<H1Sender>,
}

/// Exclusive sender checked out for readiness and one request dispatch.
///
/// Local selection, successful establishment, or peer reuse creates this value
/// while the installed record is `Selected`. The value alone may use the
/// sender. Hyper accepting the request transfers ownership to [`H1Exchange`];
/// explicit retirement closes the installed record. Dropping an undispatched
/// selection returns the sender through ordinary connection-owning-cell policy.
pub(in crate::client::pool) struct H1Selection {
    /// Non-retaining reference to the cell that owns the installed record.
    connection_cell: Weak<OriginCell>,
    /// Sender ownership until return, retirement, or response transfer.
    owner: Option<OwnedH1>,
}

impl H1Selection {
    /// Creates a selected sender owned outside the connection-owning cell lock.
    pub(super) fn new(connection_cell: &Arc<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            connection_cell: Weak::from_arc(connection_cell),
            owner: Some(owner),
        }
    }

    /// Returns this selection's physical connection identity.
    pub(in crate::client::pool) fn connection_id(&self) -> ConnectionId {
        self.owner
            .as_ref()
            .expect("HTTP/1 selection consumed more than once")
            .id()
    }

    /// Returns whether this sender came from a reusable installed record.
    pub(in crate::client::pool) fn is_reused(&self) -> bool {
        self.owner
            .as_ref()
            .expect("HTTP/1 selection consumed more than once")
            .is_reused()
    }

    /// Returns the selected connection state.
    pub(in crate::client::pool) fn connection(&self) -> &Arc<ConnectionState> {
        self.owner
            .as_ref()
            .expect("HTTP/1 selection consumed more than once")
            .connection()
    }

    /// Returns the exclusive sender for readiness and dispatch.
    pub(in crate::client::pool) fn sender_mut(&mut self) -> &mut H1Sender {
        self.owner
            .as_mut()
            .expect("HTTP/1 selection consumed more than once")
            .sender_mut()
    }

    /// Transfers the sender after Hyper accepts the request.
    ///
    /// The installed record remains `Selected`; [`H1Exchange`] now owns the
    /// sender until the response proves reuse or requires retirement.
    pub(in crate::client::pool) fn into_exchange(mut self) -> H1Exchange {
        H1Exchange {
            connection_cell: self.connection_cell.clone(),
            owner: self.owner.take(),
        }
    }

    /// Returns non-retaining close authority for this selected generation.
    pub(in crate::client::pool) fn close_handle(&self) -> H1CloseHandle {
        let owner = self
            .owner
            .as_ref()
            .expect("HTTP/1 selection consumed more than once");
        H1CloseHandle {
            connection_cell: self.connection_cell.clone(),
            connection: Weak::from_arc(owner.connection()),
            id: owner.id(),
        }
    }

    /// Retires this selection instead of returning it for reuse.
    pub(in crate::client::pool) fn retire_connection(mut self, reason: CloseReason) {
        if let Some(owner) = self.owner.take() {
            retire_at_connection_cell(&self.connection_cell, owner, reason);
        }
    }

    /// Returns the synthetic sender identity.
    #[cfg(test)]
    pub(super) fn test_sender_id(&self) -> u64 {
        self.owner
            .as_ref()
            .expect("HTTP/1 selection consumed more than once")
            .test_sender_id()
    }
}

impl fmt::Debug for H1Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("H1Selection")
            .field(
                "connection-owning cell",
                &self
                    .connection_cell
                    .upgrade()
                    .map(|connection_cell| connection_cell.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for H1Selection {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_connection_cell(&self.connection_cell, owner);
        }
    }
}

/// Exclusive sender for a request Hyper accepted on this connection.
///
/// The installed record remains `Selected` while response-body processing owns
/// this value. After a complete response and successful Hyper readiness,
/// [`H1Exchange::offer_for_reuse`] runs owning-cell return arbitration.
/// Explicit retirement handles protocol failure or upgrade. Dropping the
/// exchange means a reusable message boundary was not proven and closes the
/// connection as an incomplete HTTP/1 exchange.
pub(in crate::client::pool) struct H1Exchange {
    /// Non-retaining reference to the cell that owns the selected record.
    connection_cell: Weak<OriginCell>,
    /// Sender held until Hyper proves it may return.
    owner: Option<OwnedH1>,
}

impl H1Exchange {
    /// Returns whether Hyper already permits another request.
    pub(in crate::client::pool) fn is_ready(&self) -> bool {
        self.owner
            .as_ref()
            .expect("HTTP/1 exchange consumed more than once")
            .sender
            .is_ready()
    }

    /// Polls Hyper for proof that the sender can accept another request.
    pub(in crate::client::pool) fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), hyper::Error>> {
        self.owner
            .as_mut()
            .expect("HTTP/1 exchange consumed more than once")
            .sender_mut()
            .hyper_mut()
            .poll_ready(cx)
    }

    /// Offers a proven-ready sender to owning-cell reuse policy.
    pub(in crate::client::pool) fn offer_for_reuse(mut self) {
        let mut owner = self
            .owner
            .take()
            .expect("HTTP/1 exchange consumed more than once");
        owner.mark_reused();
        return_to_connection_cell(&self.connection_cell, owner);
    }

    /// Retires the sender instead of returning it to the connection-owning cell.
    pub(in crate::client::pool) fn retire_connection(mut self, reason: CloseReason) {
        if let Some(owner) = self.owner.take() {
            let upgrade = (reason == CloseReason::Upgraded).then(|| owner.connection().clone());
            retire_at_connection_cell(&self.connection_cell, owner, reason);
            if let Some(connection) = upgrade {
                connection.refine_protocol_close_as_upgrade();
                #[cfg(debug_assertions)]
                connection.debug_assert_close_reason(CloseReason::Upgraded);
            }
        }
    }
}

impl fmt::Debug for H1Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("H1Exchange")
            .field(
                "connection-owning cell",
                &self
                    .connection_cell
                    .upgrade()
                    .map(|connection_cell| connection_cell.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for H1Exchange {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            retire_at_connection_cell(
                &self.connection_cell,
                owner,
                CloseReason::IncompleteH1Exchange,
            );
        }
    }
}

/// Sender temporarily detached from a `Reserved` record for peer reuse.
///
/// Reuse arbitration creates this value before crossing from the
/// connection-owning cell to a requesting cell. A successful borrow converts
/// it into [`H1Selection`]; reclaim closes the connection and releases its
/// capacity. Rejection, cancellation, or `Drop` returns the sender through
/// ordinary connection-owning-cell policy.
pub(in crate::client::pool) struct ProvisionalH1 {
    /// Non-retaining connection-owning cell whose record remains in `Reserved`.
    connection_cell: Weak<OriginCell>,
    /// Sender reserved by the provisional action.
    owner: Option<OwnedH1>,
}

impl ProvisionalH1 {
    /// Creates a provisional owner for a sender extracted by reuse arbitration.
    pub(super) fn new(connection_cell: &Arc<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            connection_cell: Weak::from_arc(connection_cell),
            owner: Some(owner),
        }
    }

    /// Returns the selected connection identity without consuming the sender.
    pub(in crate::client::pool) fn connection_id(&self) -> ConnectionId {
        self.owner
            .as_ref()
            .expect("provisional HTTP/1 sender consumed more than once")
            .id()
    }

    /// Transfers the cell reference and sender into the next reuse transition.
    pub(super) fn into_parts(mut self) -> (Weak<OriginCell>, OwnedH1) {
        let owner = self
            .owner
            .take()
            .expect("provisional HTTP/1 sender consumed more than once");
        (self.connection_cell.clone(), owner)
    }

    /// Restores provisional ownership when a reuse transition cannot commit.
    pub(super) fn from_parts(connection_cell: Weak<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            connection_cell,
            owner: Some(owner),
        }
    }
}

impl fmt::Debug for ProvisionalH1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProvisionalH1")
            .field(
                "connection-owning cell",
                &self
                    .connection_cell
                    .upgrade()
                    .map(|connection_cell| connection_cell.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for ProvisionalH1 {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_connection_cell(&self.connection_cell, owner);
        }
    }
}

/// Non-retaining authority to retire one installed H1 record.
#[derive(Clone, Debug)]
pub(in crate::client::pool) struct H1CloseHandle {
    /// Connection-owning cell used to remove dispatch eligibility before
    /// logical close.
    connection_cell: Weak<OriginCell>,
    /// Core fallback when the connection-owning cell no longer exists.
    connection: Weak<ConnectionState>,
    /// Generation identity rejected by stale close actions.
    id: ConnectionId,
}

impl H1CloseHandle {
    /// Creates a close handle for a newly installed H1 record.
    pub(in crate::client::pool) fn new(
        connection_cell: &Arc<OriginCell>,
        connection: &Arc<ConnectionState>,
    ) -> Self {
        Self {
            connection_cell: Weak::from_arc(connection_cell),
            connection: Weak::from_arc(connection),
            id: connection.id(),
        }
    }

    /// Begins logical close and returns whether this signal won.
    pub(in crate::client::pool) fn close(&self, reason: CloseReason) -> bool {
        if let Some(connection_cell) = self.connection_cell.upgrade() {
            return OriginCell::close_h1(&connection_cell, self.id, reason);
        }
        self.connection
            .upgrade()
            .is_some_and(|connection| connection.logical_close(reason))
    }
}

/// Driver-owned fallback that closes its H1 record on termination.
#[derive(Debug)]
pub(in crate::client::pool) struct H1DriverGuard {
    /// Non-retaining generation close authority.
    close: H1CloseHandle,
    /// Whether drop still represents owner-runtime shutdown.
    active: bool,
}

impl H1DriverGuard {
    /// Arms driver-lifecycle cleanup for an installed H1 record.
    pub(in crate::client::pool) fn new(close: H1CloseHandle) -> Self {
        Self {
            close,
            active: true,
        }
    }

    /// Records ordinary protocol-driver completion.
    pub(in crate::client::pool) fn protocol_closed(mut self) {
        self.active = false;
        self.close.close(CloseReason::ProtocolClosed);
    }
}

impl Drop for H1DriverGuard {
    fn drop(&mut self) {
        if self.active {
            self.close.close(CloseReason::OwnerRuntimeShutdown);
        }
    }
}

impl OriginCell {
    /// Installs a fresh H1 record selected by its launching request.
    ///
    /// # Panics
    ///
    /// Panics if this cell already contains the connection identity.
    pub(in crate::client::pool) fn install_selected_h1(
        cell: &Arc<Self>,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
    ) -> H1Selection {
        let (installed, availability) = {
            let mut state = cell.state.lock();
            let installed = state.h1.install_selected(connection, sender);
            state.assert_consistent();
            (installed, state.take_h1_availability_update())
        };
        cell.publish_h1_availability(availability);
        match installed {
            Ok(owner) => H1Selection::new(cell, owner),
            Err(owner) => {
                owner
                    .connection()
                    .logical_close(CloseReason::ProtocolClosed);
                drop(owner);
                panic!("duplicate HTTP/1 connection identity installed in one cell");
            }
        }
    }

    /// Installs an H1 record whose launching acquisition already completed.
    ///
    /// # Panics
    ///
    /// Panics if this cell already contains the connection identity.
    #[cfg(test)]
    pub(in crate::client::pool) fn install_idle_h1(
        cell: &Arc<Self>,
        connection: Arc<ConnectionState>,
        sender: H1Sender,
    ) {
        let deadline = cell.idle_deadline();
        let (installed, availability) = {
            let mut state = cell.state.lock();
            let installed = state.h1.install_idle(connection, sender, deadline);
            state.assert_consistent();
            (installed, state.take_h1_availability_update())
        };
        cell.publish_h1_availability(availability);
        if let Err(owner) = installed {
            owner
                .connection()
                .logical_close(CloseReason::ProtocolClosed);
            drop(owner);
            panic!("duplicate HTTP/1 connection identity installed in one cell");
        }
        cell.notify_maintenance(deadline);
    }

    /// Selects the newest reusable H1 sender without origin-wide coordination.
    pub(in crate::client::pool) fn select_h1(cell: &Arc<Self>) -> Option<H1Selection> {
        let (owner, availability) = {
            let mut state = cell.state.lock();
            let owner = state.h1.select_idle();
            if owner.is_some() {
                state.reuse.consume_local_turn();
            }
            state.assert_consistent();
            let availability = cell
                .admission
                .as_ref()
                .and_then(|_| state.take_h1_availability_update());
            (owner, availability)
        };
        cell.publish_h1_availability(availability);
        let owner = owner?;
        Some(H1Selection::new(cell, owner))
    }

    /// Reserves this cell's selected connection for one peer reuse operation.
    pub(in crate::client::pool) fn install_h1_reuse(
        cell: &Arc<Self>,
        origin: Arc<OriginAdmission>,
        prepared: PreparedReuseInstall,
    ) -> Option<AdmissionAction> {
        let decision = {
            let mut state = cell.state.lock();
            state.install_reuse(prepared.id)
        };

        let result = match decision {
            ReuseInstall::Installed => ReuseInstallResult::Installed,
            ReuseInstall::Candidate(owner) => {
                let provisional = ProvisionalH1::new(cell, owner);
                ReuseInstallResult::Candidate(ReuseCandidate::new(
                    origin.clone(),
                    prepared.id,
                    cell.id.partition(),
                    provisional,
                ))
            }
            ReuseInstall::Rejected(availability) => ReuseInstallResult::Rejected(availability),
        };
        OriginAdmission::finish_h1_reuse_install(&origin, prepared.id, cell.id.partition(), result)
    }

    /// Clears an installed or resolving reservation after request cancellation.
    pub(in crate::client::pool) fn cancel_h1_reuse(
        &self,
        reuse_id: ReuseId,
    ) -> H1AvailabilitySnapshot {
        self.state.lock().cancel_reuse(reuse_id)
    }

    /// Returns a rejected provisional sender through ordinary connection-owning cell handling.
    pub(in crate::client::pool) fn reject_h1_reuse_candidate(
        cell: &Arc<Self>,
        reuse_id: ReuseId,
        provisional: ProvisionalH1,
    ) -> H1AvailabilitySnapshot {
        {
            let mut state = cell.state.lock();
            state.reject_reuse_candidate(reuse_id);
        }
        drop(provisional);
        let mut state = cell.state.lock();
        state.assert_consistent();
        state.report_h1_availability()
    }

    /// Revalidates a reuse operation and commits its provisional sender for dispatch.
    pub(in crate::client::pool) fn commit_h1_reuse(
        cell: &Arc<Self>,
        reuse_id: ReuseId,
        provisional: ProvisionalH1,
    ) -> Result<H1Selection, ProvisionalH1> {
        let (connection_cell, owner) = provisional.into_parts();
        let committed = cell.state.lock().commit_reuse(reuse_id, &owner);
        if committed {
            Ok(H1Selection::new(cell, owner))
        } else {
            Err(ProvisionalH1::from_parts(connection_cell, owner))
        }
    }

    /// Closes a selected sender and records fairness only if close wins.
    pub(in crate::client::pool) fn reclaim_h1_reuse(
        cell: &Arc<Self>,
        reuse_id: ReuseId,
        provisional: ProvisionalH1,
    ) -> Result<(H1AvailabilitySnapshot, bool), ProvisionalH1> {
        let (connection_cell, owner) = provisional.into_parts();
        if !cell.state.lock().reuse.names(reuse_id) {
            return Err(ProvisionalH1::from_parts(connection_cell, owner));
        }

        let close_won = Self::retire_h1_owner(cell, owner, CloseReason::Reclaimed);
        let availability = {
            let mut state = cell.state.lock();
            let availability = state.finish_reuse(reuse_id, close_won);
            state.assert_consistent();
            availability
        };
        Ok((availability, close_won))
    }

    /// Completes local reuse state after a requesting cell accepts the sender.
    pub(in crate::client::pool) fn complete_h1_reuse(
        &self,
        reuse_id: ReuseId,
        transferred: bool,
    ) -> H1AvailabilitySnapshot {
        self.state.lock().finish_reuse(reuse_id, transferred)
    }

    /// Publishes this cell's HTTP/1 availability when the origin is bounded.
    pub(super) fn publish_h1_availability(&self, availability: Option<H1AvailabilitySnapshot>) {
        if let (Some(admission), Some(availability)) = (&self.admission, availability) {
            OriginAdmission::update_h1_availability(
                admission,
                self.id.partition(),
                self.eligibility_group.clone(),
                availability,
            );
        }
    }

    /// Returns a reusable sender to the oldest compatible waiter or idle set.
    ///
    /// Demand publication, task wakeup, and any rejected-result fallback all
    /// run after the cell lock is released. `owner` remains outside the locked
    /// scope so sender or connection drop cannot run while the cell guard is
    /// live.
    fn return_h1_owner(cell: &Arc<Self>, owner: OwnedH1) {
        let connection_id = owner.id();
        let mut owner = Some(owner);
        let mut installation = None;
        let mut intercepted = None;
        let mut rejected_reuse = None;
        let idle_deadline = cell.idle_deadline();
        let should_retire = {
            let mut state = cell.state.lock();
            let returnable = state
                .h1
                .accepts_return(owner.as_ref().expect("HTTP/1 owner disappeared"));
            if !returnable {
                if let Some(reuse_id) = state.reuse.intercept_return() {
                    let snapshot = state.finish_reuse(reuse_id, false);
                    rejected_reuse = Some((reuse_id, snapshot));
                } else {
                    state.assert_consistent();
                }
                true
            } else if let Some(reuse_id) = state.reuse.intercept_return() {
                if state
                    .h1
                    .reserve_for_reuse(owner.as_ref().expect("HTTP/1 owner disappeared"))
                {
                    state.assert_consistent();
                    intercepted = Some((
                        reuse_id,
                        ProvisionalH1::new(cell, owner.take().expect("HTTP/1 owner disappeared")),
                    ));
                    false
                } else {
                    let snapshot = state.finish_reuse(reuse_id, false);
                    rejected_reuse = Some((reuse_id, snapshot));
                    true
                }
            } else if state.waiters.can_accept_h1()
                && state
                    .h1
                    .commit_return_to_waiter(owner.as_ref().expect("HTTP/1 owner disappeared"))
            {
                state.reuse.consume_local_turn();
                state.assert_consistent();
                let mut returned = owner.take().expect("HTTP/1 owner disappeared");
                returned.mark_reused();
                installation = Some(state.waiters.install_returned_h1(
                    || AcquisitionResult::H1(H1Selection::new(cell, returned)),
                    &cell.eligibility_group,
                ));
                let install = installation
                    .as_mut()
                    .expect("HTTP/1 installation disappeared");
                if let Some(waiter) = install.waiter {
                    state.h2.cancel_pending_waiter(waiter);
                }
                install.demand_updates =
                    state.publishable_demand_updates(std::mem::take(&mut install.demand_updates));
                false
            } else {
                let returned = state.h1.return_idle(
                    owner.take().expect("HTTP/1 owner disappeared"),
                    idle_deadline,
                );
                match returned {
                    Ok(()) => {
                        state.assert_consistent();
                        false
                    }
                    Err(returned) => {
                        owner = Some(returned);
                        true
                    }
                }
            }
        };

        if should_retire {
            if tracing::level_enabled!(tracing::Level::TRACE) {
                trace_h1_return(cell, connection_id, H1ReturnTrace::Rejected);
            }
            if let Some((reuse_id, snapshot)) = rejected_reuse {
                let admission = cell
                    .admission
                    .as_ref()
                    .expect("an HTTP/1 reuse operation requires bounded admission");
                OriginAdmission::reject_returned_h1_reuse(
                    admission,
                    reuse_id,
                    cell.id.partition(),
                    snapshot,
                );
            }
            Self::retire_h1_owner(
                cell,
                owner.take().expect("retired HTTP/1 owner disappeared"),
                CloseReason::ProtocolClosed,
            );
            return;
        }

        if let Some((reuse_id, provisional)) = intercepted {
            if tracing::level_enabled!(tracing::Level::TRACE) {
                trace_h1_return(cell, connection_id, H1ReturnTrace::ReuseIntercepted);
            }
            let admission = cell
                .admission
                .as_ref()
                .expect("an HTTP/1 reuse operation requires bounded admission");
            let candidate = ReuseCandidate::new(
                admission.clone(),
                reuse_id,
                cell.id.partition(),
                provisional,
            );
            let action = OriginAdmission::resolve_h1_reuse(admission, reuse_id, candidate);
            OriginAdmission::drive(action);
            return;
        }

        let Some(installation) = installation else {
            if tracing::level_enabled!(tracing::Level::TRACE) {
                trace_h1_return(cell, connection_id, H1ReturnTrace::Idle);
            }
            cell.notify_maintenance(idle_deadline);
            if cell.admission.is_some() {
                let availability = cell.state.lock().take_h1_availability_update();
                cell.publish_h1_availability(availability);
            }
            return;
        };
        if tracing::level_enabled!(tracing::Level::TRACE) {
            trace_h1_return(cell, connection_id, H1ReturnTrace::LocalDemand);
        }
        if let Some(admission) = &cell.admission {
            for snapshot in installation.demand_updates.into_iter().flatten() {
                OriginAdmission::publish_demand(admission, cell.id.partition(), snapshot);
            }
        }
        drop(installation.returned_event);
        if let Some(waker) = installation.waker {
            waker.wake();
        }
        if cell.admission.is_some() {
            let availability = cell.state.lock().take_h1_availability_update();
            cell.publish_h1_availability(availability);
        }
    }

    /// Retires an externally owned H1 sender and removes its connection-owning cell record.
    ///
    /// Returns whether this path won the connection's logical-close race.
    fn retire_h1_owner(cell: &Arc<Self>, owner: OwnedH1, reason: CloseReason) -> bool {
        let should_close = {
            let mut state = cell.state.lock();
            let should_close = state.h1.close_owned(&owner);
            state.assert_consistent();
            should_close
        };

        let id = owner.id();
        let won = should_close && owner.connection().logical_close(reason);
        drop(owner);

        let mut state = cell.state.lock();
        state.h1.finish_close(id);
        state.assert_consistent();
        let availability = state.take_h1_availability_update();
        drop(state);
        cell.publish_h1_availability(availability);
        won
    }

    /// Begins close for an installed H1 record named without its sender.
    ///
    /// Returns whether this signal won the connection's logical-close race.
    pub(super) fn close_h1(cell: &Arc<Self>, id: ConnectionId, reason: CloseReason) -> bool {
        let Some((close, availability)) = ({
            let mut state = cell.state.lock();
            let close = state.h1.begin_close(id);
            state.assert_consistent();
            close.map(|close| (close, state.take_h1_availability_update()))
        }) else {
            return false;
        };

        cell.publish_h1_availability(availability);
        let remove_record = close.sender.is_some();
        let won = close.connection.logical_close(reason);
        drop(close.sender);
        if remove_record {
            let mut state = cell.state.lock();
            state.h1.finish_close(id);
            state.assert_consistent();
        }
        won
    }

    /// Returns H1 record counts for focused ownership tests.
    #[cfg(test)]
    pub(super) fn h1_counts(&self) -> (usize, usize) {
        self.state.lock().h1.counts()
    }

    /// Returns the sole installed HTTP/1 connection for focused dispatch tests.
    #[cfg(all(test, feature = "rt-tokio"))]
    pub(in crate::client::pool) fn only_h1_connection_for_test(&self) -> Arc<ConnectionState> {
        self.state.lock().h1.only_connection_for_test()
    }
}

/// Terminal outcome for one reusable HTTP/1 sender return.
enum H1ReturnTrace {
    /// The owning cell no longer accepts the sender.
    Rejected,
    /// Origin admission reserved the sender for peer demand.
    ReuseIntercepted,
    /// No compatible demand exists, so the sender became idle.
    Idle,
    /// A waiter in the owning cell accepted the sender.
    LocalDemand,
}

/// Emits a committed HTTP/1 return outcome outside the cell lock.
// Keep field formatting out of a return transition that can synchronously
// enter admission and sender fallbacks.
#[inline(never)]
fn trace_h1_return(cell: &OriginCell, connection_id: ConnectionId, outcome: H1ReturnTrace) {
    match outcome {
        H1ReturnTrace::Rejected => tracing::trace!(
            connection_id = %connection_id,
            connection_partition = ?cell.id.partition(),
            origin_scheme = %cell.id.origin().scheme(),
            origin_host = cell.id.origin().host(),
            origin_port = ?cell.id.origin().port(),
            "HTTP/1 return was rejected by its connection-owning cell"
        ),
        H1ReturnTrace::ReuseIntercepted => tracing::trace!(
            connection_id = %connection_id,
            connection_partition = ?cell.id.partition(),
            origin_scheme = %cell.id.origin().scheme(),
            origin_host = cell.id.origin().host(),
            origin_port = ?cell.id.origin().port(),
            "HTTP/1 return was intercepted by a cross-cell reuse operation"
        ),
        H1ReturnTrace::Idle => tracing::trace!(
            connection_id = %connection_id,
            connection_partition = ?cell.id.partition(),
            origin_scheme = %cell.id.origin().scheme(),
            origin_host = cell.id.origin().host(),
            origin_port = ?cell.id.origin().port(),
            "HTTP/1 connection returned to idle storage"
        ),
        H1ReturnTrace::LocalDemand => tracing::trace!(
            connection_id = %connection_id,
            request_partition = ?cell.id.partition(),
            connection_partition = ?cell.id.partition(),
            origin_scheme = %cell.id.origin().scheme(),
            origin_host = cell.id.origin().host(),
            origin_port = ?cell.id.origin().port(),
            "HTTP/1 return satisfied local demand"
        ),
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::connection::ConnectionInfo;
    use crate::client::pool::PartitionId;

    #[test]
    fn closing_a_selected_record_rejects_its_later_return() {
        let info = ConnectionInfo::for_test(ConnectionId::new(1), PartitionId::from_index(1));
        let (connection, _physical) = ConnectionState::unbounded(info);
        let mut records = H1Records::default();
        let owner = records
            .install_selected(connection, H1Sender::test(11))
            .expect("fresh HTTP/1 record was rejected");

        assert!(records.accepts_return(&owner));
        assert!(records.begin_close(owner.id()).is_some());
        assert!(!records.accepts_return(&owner));

        drop(owner);
        records.finish_close(ConnectionId::new(1));
    }
    #[test]
    fn reuse_reservation_distinguishes_install_resolution_and_completion() {
        let reuse_id = ReuseId::for_test(1);
        let mut slot = H1ReuseReservation::default();

        assert!(slot.install(reuse_id));
        assert!(!slot.install(ReuseId::for_test(2)));
        assert_eq!(Some(reuse_id), slot.intercept_return());
        assert!(slot.complete_transfer(reuse_id, true));
        assert!(slot.local_turn_owed());
        assert!(slot.is_available());
    }

    #[test]
    fn rejection_does_not_manufacture_a_fairness_turn() {
        let reuse_id = ReuseId::for_test(1);
        let mut slot = H1ReuseReservation::default();

        assert!(slot.install_resolving(reuse_id));
        assert!(slot.reject(reuse_id));
        assert!(!slot.local_turn_owed());
    }

    #[test]
    fn owed_turn_blocks_only_while_local_h1_demand_can_use_it() {
        let reuse_id = ReuseId::for_test(1);
        let mut slot = H1ReuseReservation::default();
        assert!(slot.install_resolving(reuse_id));
        assert!(slot.complete_transfer(reuse_id, true));

        assert!(slot.blocks_peer_reuse(true));
        assert!(!slot.blocks_peer_reuse(false));
        assert!(slot.clear_unused_turn(false));
        assert!(!slot.blocks_peer_reuse(true));
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Source-cell ownership for exclusive HTTP/1 senders.
//!
//! An installed record stays in [`H1Records`] while its sender moves through
//! idle storage, request selection, response completion, and return
//! arbitration. The residence says where that one sender must be:
//!
//! ```text
//! completed handshake -- install_selected --------------------> Selected
//! test fixture ------- install_idle ---------------------------> Idle(sender)
//!
//! Idle(sender) -- select_idle --------------------------------> Selected
//! Idle(sender) -- take_idle_for_claim ------------------------> Returning
//! Selected ---- begin_return ---------------------------------> Returning
//! Returning --- commit_return_to_waiter ----------------------> Selected
//! Selected ---- return_idle ----------------------------------> Idle(sender)
//! Returning --- return_idle ----------------------------------> Idle(sender)
//!
//! Idle(sender) ------------ begin_close ----------------------> Closing
//! Selected / Returning ---- begin_close / close_owned --------> Closing
//! Closing -------- finish_close ------------------------------> removed
//! ```
//!
//! Outside the lock, ownership moves through values whose drop behavior is
//! specific to the protocol phase:
//!
//! ```text
//! H1Selection
//!   |-- rejected or dropped ------------------------------> source return
//!   `-- request accepted --> H1Exchange
//!                              |-- incomplete or dropped --> logical close
//!                              `-- reusable -------------> H1ReturnOffer
//!                                                           `-- source return
//!
//! source claim --> ProvisionalH1 --> H1Selection or source return
//! ```
//!
//! [`H1Selection`], [`H1Exchange`], [`H1ReturnOffer`], and
//! [`ProvisionalH1`] are the sender-owning values outside the cell lock. Their
//! drop behavior returns or retires the sender so a cancellation cannot leave
//! a record with no terminal owner. They retain the source cell weakly because
//! a selected sender may temporarily be the ready result inside that same
//! cell; source teardown makes the fallback close the connection directly.

use super::super::connection::{CloseReason, ConnectionState};
use super::OriginCell;
use crate::sync::{Arc, Weak};
use aws_smithy_runtime_api::client::connection::ConnectionId;
use std::collections::{HashMap, VecDeque};
use std::fmt;
#[cfg(feature = "default-client")]
use std::task::{Context, Poll};
use std::time::SystemTime;

#[cfg(feature = "default-client")]
use aws_smithy_types::body::SdkBody;

/// Concrete exclusive sender stored by an HTTP/1 record.
///
/// Production has one variant containing Hyper's sender directly. The
/// test-only variant lets state-machine and Loom tests exercise ownership
/// transitions without running a protocol driver.
pub(in crate::client::pool) enum H1Sender {
    /// Hyper's exclusive HTTP/1 request sender.
    #[cfg(feature = "default-client")]
    Hyper(hyper::client::conn::http1::SendRequest<SdkBody>),
    /// Synthetic sender identity used only by ownership tests.
    #[cfg(test)]
    Test(u64),
}

impl H1Sender {
    /// Wraps a sender returned by a successful Hyper HTTP/1 handshake.
    #[cfg(feature = "default-client")]
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
    #[cfg(feature = "default-client")]
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
    #[cfg(feature = "default-client")]
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
            #[cfg(feature = "default-client")]
            Self::Hyper(_) => panic!("Hyper sender used in a synthetic ownership test"),
        }
    }
}

impl fmt::Debug for H1Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "default-client")]
            Self::Hyper(_) => f.write_str("H1Sender::Hyper"),
            #[cfg(test)]
            Self::Test(id) => f.debug_tuple("H1Sender::Test").field(id).finish(),
        }
    }
}

/// Records owned by one source cell and the order of reusable senders.
#[derive(Debug, Default)]
pub(super) struct H1Records {
    /// Every installed record that has not completed logical close.
    records: HashMap<ConnectionId, H1Record>,
    /// Reusable records in return order; selection takes the newest sender.
    idle: VecDeque<ConnectionId>,
}

/// One source-owned HTTP/1 connection record.
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
    /// [`H1ReturnOffer`] or [`ProvisionalH1`] owns the sender.
    Returning,
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
    /// A return claim uses this path so rejection can follow the same ordinary
    /// return fallback as a sender intercepted after an active exchange.
    pub(super) fn take_idle_for_claim(&mut self) -> Option<OwnedH1> {
        let id = self.idle.pop_back()?;
        let record = self
            .records
            .get_mut(&id)
            .expect("idle HTTP/1 record disappeared");
        let H1Residence::Idle { sender, .. } =
            std::mem::replace(&mut record.residence, H1Residence::Returning)
        else {
            panic!("idle HTTP/1 order named a non-idle record");
        };
        let mut owner = OwnedH1::new(record.connection.clone(), sender, true);
        owner.mark_reused();
        self.assert_consistent();
        Some(owner)
    }

    /// Returns whether a sender could satisfy a source claim now or on return.
    pub(super) fn has_returnable(&self) -> bool {
        self.records.values().any(|record| {
            matches!(
                record.residence,
                H1Residence::Idle { .. } | H1Residence::Selected | H1Residence::Returning
            )
        })
    }

    /// Returns whether `owner` may still re-enter reusable return policy.
    pub(super) fn accepts_return(&self, owner: &OwnedH1) -> bool {
        self.records.get(&owner.id()).is_some_and(|record| {
            matches!(
                record.residence,
                H1Residence::Selected | H1Residence::Returning
            )
        })
    }

    /// Returns whether an installed claim still has a source-side endpoint.
    ///
    /// Logical close may move an externally owned sender to `Closing` before
    /// its return resolves the installed claim, so claim consistency is
    /// intentionally broader than current reuse eligibility.
    pub(super) fn supports_installed_claim(&self) -> bool {
        self.records.values().any(|record| {
            matches!(
                record.residence,
                H1Residence::Selected | H1Residence::Returning | H1Residence::Closing
            )
        })
    }

    /// Returns whether this cell should remain advertised as an H1 source.
    pub(super) fn is_advertisable(&self) -> bool {
        self.has_returnable()
    }

    /// Moves an externally selected sender into return arbitration.
    pub(super) fn begin_return(&mut self, id: ConnectionId) -> bool {
        let Some(record) = self.records.get_mut(&id) else {
            return false;
        };
        if !matches!(record.residence, H1Residence::Selected) {
            return false;
        }
        record.residence = H1Residence::Returning;
        self.assert_consistent();
        true
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
            H1Residence::Selected | H1Residence::Returning
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
            H1Residence::Returning => {
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
                H1Residence::Selected | H1Residence::Returning => {
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
            H1Residence::Selected | H1Residence::Returning => {
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

/// Exclusive sender and connection state detached from a source-cell record.
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

/// Returns an external sender to its source or closes it after source teardown.
fn return_to_source(source: &Weak<OriginCell>, owner: OwnedH1) {
    if let Some(source) = source.upgrade() {
        OriginCell::return_h1_owner(&source, owner);
    } else {
        owner.connection().logical_close(CloseReason::PoolDropped);
        drop(owner);
    }
}

/// Retires an external sender through its source or directly after teardown.
fn retire_at_source(source: &Weak<OriginCell>, owner: OwnedH1, reason: CloseReason) {
    if let Some(source) = source.upgrade() {
        OriginCell::retire_h1_owner(&source, owner, reason);
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

/// An exclusive H1 sender selected for request preparation and dispatch.
///
/// Dropping an undispatched selection follows ordinary source return. Once
/// Hyper accepts a request, convert this into [`H1Exchange`]. Dropping that
/// exchange retires the connection unless Hyper proves a reusable boundary.
pub(in crate::client::pool) struct H1Selection {
    /// Non-retaining reference to the cell that owns the installed record.
    source: Weak<OriginCell>,
    /// Sender ownership until return, retirement, or response transfer.
    owner: Option<OwnedH1>,
}

impl H1Selection {
    /// Creates a selected sender owned outside the source-cell lock.
    pub(super) fn new(source: &Arc<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            source: Weak::from_arc(source),
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

    /// Transfers an accepted request's sender to response-lifecycle cleanup.
    pub(in crate::client::pool) fn into_exchange(mut self) -> H1Exchange {
        H1Exchange {
            source: self.source.clone(),
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
            source: self.source.clone(),
            connection: Weak::from_arc(owner.connection()),
            id: owner.id(),
        }
    }

    /// Retires this selection instead of returning it for reuse.
    pub(in crate::client::pool) fn retire(mut self, reason: CloseReason) {
        if let Some(owner) = self.owner.take() {
            retire_at_source(&self.source, owner, reason);
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
                "source",
                &self.source.upgrade().map(|source| source.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for H1Selection {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_source(&self.source, owner);
        }
    }
}

/// Accepted HTTP/1 exchange awaiting a reusable message boundary.
///
/// This value follows the response lifecycle and may later move into an
/// owner-runtime readiness task. Dropping it means reuse was not proven.
pub(in crate::client::pool) struct H1Exchange {
    /// Non-retaining reference to the cell that owns the selected record.
    source: Weak<OriginCell>,
    /// Sender held until Hyper proves it may return.
    owner: Option<OwnedH1>,
}

impl H1Exchange {
    /// Returns whether Hyper already permits another request.
    #[cfg(feature = "default-client")]
    pub(in crate::client::pool) fn is_ready(&self) -> bool {
        self.owner
            .as_ref()
            .expect("HTTP/1 exchange consumed more than once")
            .sender
            .is_ready()
    }

    /// Polls Hyper for proof that the sender can accept another request.
    #[cfg(feature = "default-client")]
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

    /// Begins source-owned return after Hyper proves a complete exchange.
    pub(in crate::client::pool) fn into_offer(mut self) -> Option<H1ReturnOffer> {
        let mut owner = self
            .owner
            .take()
            .expect("HTTP/1 exchange consumed more than once");
        owner.mark_reused();
        if self
            .source
            .upgrade()
            .is_some_and(|source| source.begin_h1_return(owner.id()))
        {
            Some(H1ReturnOffer {
                source: self.source.clone(),
                owner: Some(owner),
            })
        } else {
            retire_at_source(&self.source, owner, CloseReason::ProtocolClosed);
            None
        }
    }

    /// Retires the sender instead of returning it to the source cell.
    pub(in crate::client::pool) fn retire(mut self, reason: CloseReason) {
        if let Some(owner) = self.owner.take() {
            let upgrade = (reason == CloseReason::Upgraded).then(|| owner.connection().clone());
            retire_at_source(&self.source, owner, reason);
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
                "source",
                &self.source.upgrade().map(|source| source.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for H1Exchange {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            retire_at_source(&self.source, owner, CloseReason::IncompleteH1Exchange);
        }
    }
}

/// Source-owned reusable sender awaiting local or admission return policy.
///
/// Until this value resolves, its installed record is `Returning` and cannot
/// be selected. Dropping the offer performs ordinary local return.
pub(in crate::client::pool) struct H1ReturnOffer {
    /// Non-retaining reference to the cell that owns the returning record.
    source: Weak<OriginCell>,
    /// Reusable sender being offered.
    owner: Option<OwnedH1>,
}

impl H1ReturnOffer {
    /// Runs ordinary source return now.
    pub(in crate::client::pool) fn resolve(mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_source(&self.source, owner);
        }
    }
}

impl fmt::Debug for H1ReturnOffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("H1ReturnOffer")
            .field(
                "source",
                &self.source.upgrade().map(|source| source.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for H1ReturnOffer {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_source(&self.source, owner);
        }
    }
}

/// Returning H1 sender detached for one claim or delivery attempt.
///
/// Rejection or cancellation drops back into ordinary source return.
pub(in crate::client::pool) struct ProvisionalH1 {
    /// Non-retaining source whose record remains in `Returning`.
    source: Weak<OriginCell>,
    /// Sender reserved by the provisional action.
    owner: Option<OwnedH1>,
}

impl ProvisionalH1 {
    /// Creates provisional ownership for a sender extracted by a source claim.
    pub(super) fn new(source: &Arc<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            source: Weak::from_arc(source),
            owner: Some(owner),
        }
    }

    /// Detaches the source and sender for a source-cell claim transition.
    pub(super) fn into_parts(mut self) -> (Weak<OriginCell>, OwnedH1) {
        let owner = self
            .owner
            .take()
            .expect("provisional HTTP/1 sender consumed more than once");
        (self.source.clone(), owner)
    }

    /// Rebuilds provisional ownership after a failed claim transition.
    pub(super) fn from_parts(source: Weak<OriginCell>, owner: OwnedH1) -> Self {
        Self {
            source,
            owner: Some(owner),
        }
    }
}

impl fmt::Debug for ProvisionalH1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProvisionalH1")
            .field(
                "source",
                &self.source.upgrade().map(|source| source.id().clone()),
            )
            .field("connection_id", &self.owner.as_ref().map(OwnedH1::id))
            .finish()
    }
}

impl Drop for ProvisionalH1 {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            return_to_source(&self.source, owner);
        }
    }
}

/// Non-retaining authority to retire one installed H1 generation.
#[derive(Clone, Debug)]
pub(in crate::client::pool) struct H1CloseHandle {
    /// Source cell used to remove dispatch eligibility before logical close.
    source: Weak<OriginCell>,
    /// Core fallback when the source cell no longer exists.
    connection: Weak<ConnectionState>,
    /// Generation identity rejected by stale close actions.
    id: ConnectionId,
}

impl H1CloseHandle {
    /// Creates a close handle for a newly installed H1 record.
    pub(in crate::client::pool) fn new(
        source: &Arc<OriginCell>,
        connection: &Arc<ConnectionState>,
    ) -> Self {
        Self {
            source: Weak::from_arc(source),
            connection: Weak::from_arc(connection),
            id: connection.id(),
        }
    }

    /// Begins logical close and returns whether this signal won.
    pub(in crate::client::pool) fn close(&self, reason: CloseReason) -> bool {
        if let Some(source) = self.source.upgrade() {
            return OriginCell::close_h1(&source, self.id, reason);
        }
        self.connection
            .upgrade()
            .is_some_and(|connection| connection.logical_close(reason))
    }
}

/// Driver-owned fallback that closes its H1 generation on termination.
#[derive(Debug)]
pub(in crate::client::pool) struct H1DriverGuard {
    /// Non-retaining generation close authority.
    close: H1CloseHandle,
    /// Whether drop still represents owner-runtime shutdown.
    active: bool,
}

impl H1DriverGuard {
    /// Arms driver-lifecycle cleanup for an installed H1 generation.
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
}

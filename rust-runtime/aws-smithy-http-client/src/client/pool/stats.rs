/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Per-(partition, authority) connection counters and a pool-level inverted index.
//!
//! Counters are maintained with `Relaxed` atomics. A snapshot observes each counter
//! independently; transient mutual inconsistency is expected (e.g. `active` may
//! briefly exceed `established`). The index never blocks the connection hot path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use super::connection::Authority;
use super::partition::PartitionId;

// Protocol-tag encoding for the cell. A cell is per-(partition, authority);
// protocol is per-connection, so a cell CAN observe both over its life
// (multi-endpoint authority, server reconfig, proxy). The tag latches to
// MIXED once it sees two different protocols and never leaves it, so
// `capacity_hint` only answers `Some` for a uniformly-H1 cell and never
// overstates reusable capacity.
pub(crate) const PROTO_UNSET: u8 = 0;
pub(crate) const PROTO_H1: u8 = 1;
pub(crate) const PROTO_H2: u8 = 2;
const PROTO_MIXED: u8 = 3;

/// Per-(partition, authority) connection counts maintained with `Relaxed` atomics.
///
/// Each counter is loaded independently; concurrent reads may observe transiently
/// inconsistent combinations (e.g. `active` > `established`). Intended for heuristic
/// reads that tolerate stale or momentarily inconsistent values.
#[derive(Debug, Default)]
pub(crate) struct ConnectionCounters {
    /// Connections that have completed handshake and exist (idle + active).
    pub(crate) established: AtomicUsize,
    /// Handshakes in flight.
    pub(crate) establishing: AtomicUsize,
    /// Connections/streams currently checked out.
    pub(crate) active: AtomicUsize,
    /// Protocol tag for this cell (monotonic toward MIXED).
    protocol: AtomicU8,
}

/// Tracks a connection committed to handshaking (TCP + TLS + protocol).
///
/// Construction increments `establishing`. [`promote`](Self::promote) transitions to
/// an [`EstablishedGuard`] (established++ then establishing--) on success; any other
/// drop path (failure, cancel, panic) decrements `establishing`. Exactly-once by
/// construction: `promote` consumes `self`.
pub(crate) struct EstablishingGuard {
    counters: Arc<ConnectionCounters>,
    promoted: bool,
}

impl EstablishingGuard {
    pub(crate) fn new(counters: Arc<ConnectionCounters>) -> Self {
        counters.establishing.fetch_add(1, Ordering::Relaxed);
        Self {
            counters,
            promoted: false,
        }
    }

    /// Handshake succeeded: transition establishing → established.
    ///
    /// `established` is incremented BEFORE `establishing` is decremented so a
    /// concurrent reader may observe a transient overcount. The overcount is in the
    /// direction of over-reporting readiness, never under-reporting.
    pub(crate) fn promote(mut self, proto: u8) -> EstablishedGuard {
        self.counters.observe_protocol(proto);
        self.counters.established.fetch_add(1, Ordering::Relaxed);
        self.counters.establishing.fetch_sub(1, Ordering::Relaxed);
        self.promoted = true;
        EstablishedGuard {
            counters: self.counters.clone(),
        }
    }
}

impl Drop for EstablishingGuard {
    fn drop(&mut self) {
        if !self.promoted {
            self.counters.establishing.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Owns one connection's contribution to `established`. Held on the
/// Arc-shared inner of a `ManagedConnection`, so for H2 (N clones share
/// one connection) it fires `established--` exactly once — when the last
/// clone drops. Non-`Clone` by design: single ownership is compiler-
/// enforced.
pub(crate) struct EstablishedGuard {
    counters: Arc<ConnectionCounters>,
}

impl Drop for EstablishedGuard {
    fn drop(&mut self) {
        self.counters.established.fetch_sub(1, Ordering::Relaxed);
    }
}

impl ConnectionCounters {
    pub(crate) fn incr_active(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the active count.
    ///
    /// Every `incr_active` is paired with exactly one `decr_active` via RAII checkout
    /// guards, so `active` is non-negative by construction. A saturating sub here
    /// would mask a broken-pairing bug; saturation belongs only on the cross-atomic
    /// READ (`idle() = established.saturating_sub(active)`) where two independent
    /// relaxed loads may transiently cross.
    pub(crate) fn decr_active(&self) {
        let prev = self.active.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev > 0, "active underflow: decr without matching incr");
    }

    /// Record the negotiated protocol for a connection in this cell.
    ///
    /// Latches monotonically toward `MIXED`: `UNSET` → observed protocol; same
    /// protocol → no-op; different protocol → `MIXED` (terminal). A cell reaches
    /// `MIXED` when one authority negotiates differently across connections
    /// (multi-endpoint DNS, server reconfiguration, an intermediary).
    ///
    /// The sole purpose of this tag is to keep `capacity_hint` from overstating:
    /// `capacity_hint` returns `Some` only for a cell known to be uniformly one
    /// protocol it can reason about (HTTP/1). Accurate per-connection protocol and
    /// stream-limit accounting (per-connection indexing) is future work that would
    /// make this cell-level latch unnecessary.
    pub(crate) fn observe_protocol(&self, proto: u8) {
        let mut cur = self.protocol.load(Ordering::Relaxed);
        loop {
            let next = match cur {
                PROTO_UNSET => proto,
                c if c == proto => return,
                PROTO_MIXED => return,
                _ => PROTO_MIXED,
            };
            match self.protocol.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => cur = actual,
            }
        }
    }

    pub(crate) fn protocol(&self) -> u8 {
        self.protocol.load(Ordering::Relaxed)
    }
}

/// One authority's row in the inverted index: per-partition weak references to counters.
#[derive(Debug, Default)]
pub(crate) struct AuthorityCounters {
    pub(crate) by_partition: HashMap<PartitionId, Weak<ConnectionCounters>>,
}

/// Pool-level inverted index: authority → per-partition counters.
///
/// Non-owning projection: holds `Weak<ConnectionCounters>` references. Strong owners
/// (pool entries, live checkouts) keep cells alive; once all strong references drop,
/// the `Weak` goes dead and is pruned on the next `snapshot`. The index never extends
/// connection lifetime and never blocks the per-request hot path.
#[derive(Debug, Default)]
pub(crate) struct StatsIndex {
    inner: std::sync::Mutex<HashMap<Authority, AuthorityCounters>>,
}

impl StatsIndex {
    /// Register a cell's counters at first-touch of (authority, partition).
    /// Stores a `Weak` reference; the caller retains the strong `Arc`.
    /// Idempotent: re-registration overwrites the previous entry.
    pub(crate) fn register(
        &self,
        authority: Authority,
        partition: PartitionId,
        counters: &Arc<ConnectionCounters>,
    ) {
        let mut idx = self.inner.lock().expect("stats index poisoned");
        idx.entry(authority)
            .or_default()
            .by_partition
            .insert(partition, Arc::downgrade(counters));
    }

    /// Drop the `(authority, partition)` cell if its counters are no longer
    /// referenced, removing the authority entry when its last partition is
    /// pruned. A no-op if the cell is still strongly held (e.g. a checkout
    /// is in flight when the host's idle connections are evicted), so it is
    /// safe to call from the eviction path. Reconstructs nothing the caller
    /// does not already hold.
    pub(crate) fn prune_if_dead(&self, authority: &Authority, partition: PartitionId) {
        let mut idx = self.inner.lock().expect("stats index poisoned");
        if let Some(a) = idx.get_mut(authority) {
            if a.by_partition
                .get(&partition)
                .is_some_and(|w| w.strong_count() == 0)
            {
                a.by_partition.remove(&partition);
            }
            if a.by_partition.is_empty() {
                idx.remove(authority);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.inner.lock().expect("stats index poisoned").len()
    }

    #[cfg(test)]
    pub(crate) fn established_for(&self, authority: &Authority, partition: PartitionId) -> usize {
        let idx = self.inner.lock().expect("stats index poisoned");
        idx.get(authority)
            .and_then(|a| a.by_partition.get(&partition))
            .and_then(|w| w.upgrade())
            .map(|c| c.established.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn establishing_for(&self, authority: &Authority, partition: PartitionId) -> usize {
        let idx = self.inner.lock().expect("stats index poisoned");
        idx.get(authority)
            .and_then(|a| a.by_partition.get(&partition))
            .and_then(|w| w.upgrade())
            .map(|c| c.establishing.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Partitions with at least one idle connection to `authority`, as
    /// `(partition, idle_count)`. Advisory: a relaxed snapshot that
    /// *narrows* reclaim/borrow candidates — the cache pop is the
    /// authoritative confirmation. Prunes dead `Weak`s under the lock,
    /// same as [`Self::snapshot`].
    pub(crate) fn idle_partitions_for(&self, authority: &Authority) -> Vec<(PartitionId, usize)> {
        let handles: Vec<(PartitionId, Arc<ConnectionCounters>)> = {
            let mut idx = self.inner.lock().expect("stats index poisoned");
            match idx.get_mut(authority) {
                Some(a) => {
                    let mut live = Vec::new();
                    a.by_partition.retain(|partition, weak| {
                        if let Some(strong) = weak.upgrade() {
                            live.push((*partition, strong));
                            true
                        } else {
                            false
                        }
                    });
                    if a.by_partition.is_empty() {
                        idx.remove(authority);
                    }
                    live
                }
                None => Vec::new(),
            }
        }; // lock released before loading atomics
        handles
            .into_iter()
            .filter_map(|(p, c)| {
                let established = c.established.load(Ordering::Relaxed);
                let active = c.active.load(Ordering::Relaxed);
                let idle = established.saturating_sub(active);
                (idle > 0).then_some((p, idle))
            })
            .collect()
    }

    /// All `(authority, partition)` cells with at least one idle
    /// connection. Drives `Global`-constraint reclaim, where a freed
    /// permit is fungible across authorities. Advisory/narrowing, same
    /// contract as [`Self::idle_partitions_for`].
    pub(crate) fn idle_cells(&self) -> Vec<(Authority, PartitionId)> {
        let handles: Vec<(Authority, PartitionId, Arc<ConnectionCounters>)> = {
            let mut idx = self.inner.lock().expect("stats index poisoned");
            let mut live = Vec::new();
            idx.retain(|authority, a| {
                a.by_partition.retain(|partition, weak| {
                    if let Some(strong) = weak.upgrade() {
                        live.push((authority.clone(), *partition, strong));
                        true
                    } else {
                        false
                    }
                });
                !a.by_partition.is_empty()
            });
            live
        }; // lock released before loading atomics
        handles
            .into_iter()
            .filter_map(|(authority, p, c)| {
                let established = c.established.load(Ordering::Relaxed);
                let active = c.active.load(Ordering::Relaxed);
                (established.saturating_sub(active) > 0).then_some((authority, p))
            })
            .collect()
    }

    /// Snapshot one authority's per-partition counters.
    ///
    /// Under the lock: upgrades each `Weak`, prunes dead entries (removing the
    /// authority entirely if all its partitions are dead). Releases the lock before
    /// loading atomics into the returned snapshot.
    pub(crate) fn snapshot(&self, authority: &Authority) -> AuthorityStats {
        let handles: Vec<(PartitionId, Arc<ConnectionCounters>)> = {
            let mut idx = self.inner.lock().expect("stats index poisoned");
            match idx.get_mut(authority) {
                Some(a) => {
                    let mut live = Vec::new();
                    a.by_partition.retain(|partition, weak| {
                        if let Some(strong) = weak.upgrade() {
                            live.push((*partition, strong));
                            true
                        } else {
                            false
                        }
                    });
                    if a.by_partition.is_empty() {
                        idx.remove(authority);
                    }
                    live
                }
                None => Vec::new(),
            }
        }; // lock released here
        let by_partition = handles
            .into_iter()
            .map(|(p, c)| {
                (
                    p,
                    PartitionStats {
                        established: c.established.load(Ordering::Relaxed),
                        establishing: c.establishing.load(Ordering::Relaxed),
                        active: c.active.load(Ordering::Relaxed),
                        protocol: c.protocol(),
                    },
                )
            })
            .collect();
        AuthorityStats { by_partition }
    }
}

/// Point-in-time snapshot of one (partition, authority) cell's connection counts.
///
/// Plain `usize` values from relaxed loads; cheap and `Copy`. Each field is loaded
/// independently and may be transiently inconsistent with the others.
/// `#[non_exhaustive]` permits future additions (e.g. per-connection stream depth).
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct PartitionStats {
    /// Connections that have completed handshake (idle + active).
    pub established: usize,
    /// Handshakes in flight.
    pub establishing: usize,
    /// Connections/streams currently checked out.
    pub active: usize,
    // Private: protocol tag for capacity_hint. Not public — implementation
    // detail of the hint that would otherwise freeze an internal encoding
    // into the API.
    protocol: u8,
}

impl PartitionStats {
    /// Connections not currently checked out: `established.saturating_sub(active)`.
    ///
    /// Exact for HTTP/1 (one stream per connection). For HTTP/2 a positive value
    /// means connections with no active streams; saturated multiplexed connections
    /// contribute 0. The saturating subtraction handles transient inconsistency
    /// between the two independently-loaded atomics.
    pub fn idle(&self) -> usize {
        self.established.saturating_sub(self.active)
    }

    /// Spare stream capacity, if determinable from current state.
    ///
    /// - Uniformly HTTP/1 cell: `Some(idle)` (one stream per connection).
    /// - HTTP/2, mixed-protocol, or not-yet-handshaken cell: `None` (per-connection
    ///   stream limits are not indexed).
    ///
    /// When `Some`, still a relaxed snapshot — treat as a hint.
    pub fn capacity_hint(&self) -> Option<usize> {
        match self.protocol {
            PROTO_H1 => Some(self.idle()),
            _ => None,
        }
    }
}

/// Point-in-time, per-partition snapshot of connection counts for one authority.
///
/// Sparse: only partitions that have opened a connection to the authority appear.
/// Returned by [`super::SharedPool::stats`].
pub struct AuthorityStats {
    by_partition: Vec<(PartitionId, PartitionStats)>,
}

impl AuthorityStats {
    /// Stats for a specific partition, if it has opened a connection to this authority.
    pub fn get(&self, partition: PartitionId) -> Option<PartitionStats> {
        self.by_partition
            .iter()
            .find(|(p, _)| *p == partition)
            .map(|(_, s)| *s)
    }

    /// Iterate (partition, stats) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (PartitionId, PartitionStats)> + '_ {
        self.by_partition.iter().copied()
    }

    /// Number of partitions that have opened a connection to this authority.
    pub fn len(&self) -> usize {
        self.by_partition.len()
    }

    /// Whether no partition has connected to this authority.
    pub fn is_empty(&self) -> bool {
        self.by_partition.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_incremented_on_checkout_decremented_on_drop() {
        let counters = Arc::new(ConnectionCounters::default());
        assert_eq!(counters.active.load(Ordering::Relaxed), 0);

        counters.incr_active();
        assert_eq!(counters.active.load(Ordering::Relaxed), 1);

        counters.decr_active();
        assert_eq!(counters.active.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn establishing_guard_promote_increments_established_and_clears_establishing() {
        let counters = Arc::new(ConnectionCounters::default());

        let guard = EstablishingGuard::new(counters.clone());
        assert_eq!(counters.establishing.load(Ordering::Relaxed), 1);
        assert_eq!(counters.established.load(Ordering::Relaxed), 0);

        let established = guard.promote(PROTO_H1);
        assert_eq!(counters.establishing.load(Ordering::Relaxed), 0);
        assert_eq!(counters.established.load(Ordering::Relaxed), 1);

        drop(established);
        assert_eq!(counters.established.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn establishing_guard_drop_without_promote_decrements() {
        let counters = Arc::new(ConnectionCounters::default());

        let guard = EstablishingGuard::new(counters.clone());
        assert_eq!(counters.establishing.load(Ordering::Relaxed), 1);

        drop(guard);
        assert_eq!(counters.establishing.load(Ordering::Relaxed), 0);
        assert_eq!(counters.established.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn stats_index_registers_counters_arc() {
        let index = StatsIndex::default();
        assert_eq!(index.len(), 0);

        let counters = Arc::new(ConnectionCounters::default());
        let authority_a = Authority::new("a.example.com:443");
        let partition = PartitionId::from_index(0);
        index.register(authority_a, partition, &counters);
        assert_eq!(index.len(), 1);

        // Different authority → new entry
        let authority_b = Authority::new("b.example.com:443");
        let counters2 = Arc::new(ConnectionCounters::default());
        index.register(authority_b, partition, &counters2);
        assert_eq!(index.len(), 2);

        // Same authority, different partition → same entry (len unchanged)
        let authority_a2 = Authority::new("a.example.com:443");
        let counters3 = Arc::new(ConnectionCounters::default());
        let partition2 = PartitionId::from_index(1);
        index.register(authority_a2, partition2, &counters3);
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn stats_index_prunes_dead_cells() {
        let index = StatsIndex::default();
        let authority = Authority::new("ephemeral.example.com:443");
        let partition = PartitionId::from_index(0);

        let counters = Arc::new(ConnectionCounters::default());
        index.register(authority.clone(), partition, &counters);
        assert_eq!(index.len(), 1);

        // Drop the only strong reference — the Weak in the index is now dead
        drop(counters);

        // Snapshot triggers pruning; dead cell is removed
        let snap = index.snapshot(&authority);
        assert!(snap.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn prune_if_dead_keeps_live_cell_removes_dead_cell() {
        let index = StatsIndex::default();
        let authority = Authority::new("host.example.com:443");
        let partition = PartitionId::from_index(0);
        let counters = Arc::new(ConnectionCounters::default());
        index.register(authority.clone(), partition, &counters);
        assert_eq!(index.len(), 1);

        // Cell is still strongly held (mirrors a checkout in flight when the
        // host's idle connections are evicted): prune is a no-op.
        index.prune_if_dead(&authority, partition);
        assert_eq!(index.len(), 1);

        // Strong ref gone (entry + checkouts dropped): prune removes the cell
        // and, as its last partition, the authority entry.
        drop(counters);
        index.prune_if_dead(&authority, partition);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn capacity_hint_h1_some_h2_none_mixed_none() {
        // H1-only cell: capacity_hint == Some(idle)
        let s = PartitionStats {
            established: 5,
            establishing: 0,
            active: 2,
            protocol: PROTO_H1,
        };
        assert_eq!(s.capacity_hint(), Some(3));

        // H2-only cell: None
        let s = PartitionStats {
            established: 5,
            establishing: 0,
            active: 2,
            protocol: PROTO_H2,
        };
        assert_eq!(s.capacity_hint(), None);

        // UNSET cell: None
        let s = PartitionStats {
            established: 0,
            establishing: 1,
            active: 0,
            protocol: PROTO_UNSET,
        };
        assert_eq!(s.capacity_hint(), None);

        // Mixed cell via observe_protocol transitions: None
        let counters = Arc::new(ConnectionCounters::default());
        counters.observe_protocol(PROTO_H1);
        assert_eq!(counters.protocol(), PROTO_H1);
        counters.observe_protocol(PROTO_H2);
        assert_eq!(counters.protocol(), PROTO_MIXED);
        let s = PartitionStats {
            established: 5,
            establishing: 0,
            active: 2,
            protocol: counters.protocol(),
        };
        assert_eq!(s.capacity_hint(), None);
    }

    #[test]
    fn idle_is_established_minus_active_saturating() {
        let s = PartitionStats {
            established: 3,
            establishing: 0,
            active: 1,
            protocol: PROTO_H1,
        };
        assert_eq!(s.idle(), 2);

        // H2 over-subscription: active > established (multiple streams per conn)
        let s = PartitionStats {
            established: 1,
            establishing: 0,
            active: 5,
            protocol: PROTO_H2,
        };
        assert_eq!(s.idle(), 0);
    }
}

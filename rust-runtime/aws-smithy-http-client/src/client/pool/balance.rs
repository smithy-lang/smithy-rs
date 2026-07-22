/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Per-authority admission and fairness for the connection pool.
//!
//! One [`Waitlist`] exists per authority (host:port), shared across every
//! partition that talks to that host. It owns two things that must be
//! decided together:
//!
//! * **The capacity counter** (`available`): how many connection permits to
//!   this authority are free. This replaces the per-host tokio semaphore —
//!   a permit is a [`Token`], and dropping a `Token` returns its capacity
//!   through the same fairness decision as everything else.
//! * **The FIFO of parked requesters** (`queue`): requests that found no
//!   local idle connection and no free permit, ordered oldest-first.
//!
//! A parked requester is woken by exactly one of two events, whichever
//! happens first, delivered into its slot:
//!
//! * [`Payload::Warm`] — a peer partition in the same NIC group returned a
//!   warm connection; it is loaned to this waiter (no permit moves).
//! * [`Payload::Grant`] — a permit was freed (a connection died); the
//!   waiter receives a [`Token`] and connects its own connection.
//!
//! This module is generic over the loaned payload `T` and knows nothing
//! about connections, caches, or NIC binding beyond an opaque [`NicGroup`]
//! label. That keeps the concurrency model testable in isolation.

// Spike: the pool does not yet call into this module; suppress dead-code
// lints until the integration slice wires it in.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Opaque NIC-group label. Two partitions with the same `NicGroup` can
/// share warm connections (same physical interface); different groups
/// cannot. Interned from the pool's `by_nic` map at build time; this
/// module only ever compares them for equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NicGroup(pub(crate) u16);

/// What a woken waiter receives in its slot: either a loaned warm
/// connection (same group, no permit moves) or a freed permit to connect
/// its own.
enum Payload<T> {
    Warm(T),
    Grant(Token<T>),
}

/// A unit of connection capacity to an authority.
///
/// Held for the lifetime of a connection (or an in-flight connect). On
/// drop it re-enters the [`Waitlist`]: if a requester is parked, the
/// capacity is handed to the oldest one as a [`Payload::Grant`]; otherwise
/// it returns to the free counter. The drop discipline *is* the capacity
/// accounting — there is no separate release path to forget.
pub(crate) struct Token<T> {
    waitlist: Arc<Waitlist<T>>,
    /// Cleared when the token is consumed into a slot (grant reissue) so
    /// its `Drop` does not double-release.
    live: bool,
}

impl<T> Token<T> {
    fn new(waitlist: Arc<Waitlist<T>>) -> Self {
        Self {
            waitlist,
            live: true,
        }
    }
}

impl<T> Drop for Token<T> {
    fn drop(&mut self) {
        if self.live {
            self.waitlist.clone().release_one();
        }
    }
}

/// The outcome of admission for a requester.
pub(crate) enum Admitted<T> {
    /// A permit was acquired; connect a fresh connection and let the
    /// `Token` ride it.
    Token(Token<T>),
    /// A warm connection was loaned from a same-group peer; dispatch on it
    /// directly (no connect).
    Warm(T),
}

/// The outcome of offering a returning warm connection to the waitlist.
pub(crate) enum Reserved<T> {
    /// The connection was placed in the oldest same-group waiter's slot
    /// and that waiter was woken. Do not return it to the idle set.
    Placed,
    /// No eligible waiter; the connection is handed back to be idled as
    /// normal.
    Idle(T),
}

struct Waiter<T> {
    id: u64,
    group: NicGroup,
    slot: Option<Payload<T>>,
    waker: Option<Waker>,
}

struct Inner<T> {
    /// Free permits: capacity that can be handed out immediately.
    available: usize,
    /// Parked requesters, oldest first.
    queue: VecDeque<Waiter<T>>,
    next_id: u64,
}

impl<T> Inner<T> {
    /// True when no parked requester is still waiting for a slot. A
    /// waiter whose slot is already filled (served, not yet polled) does
    /// not count — its capacity is already committed.
    fn no_unfilled(&self) -> bool {
        self.queue.iter().all(|w| w.slot.is_some())
    }
}

/// Per-authority admission structure. Construct with [`Waitlist::new`] and
/// hold behind the returned `Arc`.
pub(crate) struct Waitlist<T> {
    inner: Mutex<Inner<T>>,
    /// Count of waiters still awaiting a slot. Mirrors
    /// `Inner::queue`'s unfilled count, maintained under the lock, exposed
    /// for a lock-free fast path on the connection-return hot path
    /// ([`Waitlist::waiter_count`]): a return with no waiters skips the
    /// lock entirely. Never under-reports relative to a committed
    /// registration (incremented under the lock before the registering
    /// thread releases it).
    waiter_count: AtomicUsize,
}

impl<T> Waitlist<T> {
    /// Create a waitlist with `cap` free permits.
    pub(crate) fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                available: cap,
                queue: VecDeque::new(),
                next_id: 0,
            }),
            waiter_count: AtomicUsize::new(0),
        })
    }

    /// Lock-free count of requesters still awaiting a slot. Used by the
    /// return path to skip the lock when nobody is waiting.
    pub(crate) fn waiter_count(&self) -> usize {
        self.waiter_count.load(Ordering::Acquire)
    }

    /// Free-permit count. Test/telemetry accessor.
    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        self.inner.lock().expect("waitlist poisoned").available
    }

    /// Begin admission. The returned future resolves when this requester
    /// either acquires a permit (fast path, or a later grant) or is loaned
    /// a warm connection.
    pub(crate) fn admit(self: &Arc<Self>, group: NicGroup) -> Admit<T> {
        Admit {
            waitlist: self.clone(),
            group,
            id: None,
            done: false,
        }
    }

    /// Offer a returning warm connection to the oldest same-group waiter.
    ///
    /// On a hit the connection is placed in that waiter's slot and it is
    /// woken; on a miss the connection is handed back via [`Reserved::Idle`]
    /// so the caller can return it to the idle set. Always takes the lock;
    /// callers gate on [`Waitlist::waiter_count`] first for the common
    /// no-waiter case.
    pub(crate) fn try_reserve_same_group(&self, group: NicGroup, conn: T) -> Reserved<T> {
        let mut inner = self.inner.lock().expect("waitlist poisoned");
        match inner
            .queue
            .iter_mut()
            .find(|w| w.slot.is_none() && w.group == group)
        {
            Some(w) => {
                w.slot = Some(Payload::Warm(conn));
                if let Some(waker) = w.waker.take() {
                    waker.wake();
                }
                self.waiter_count.fetch_sub(1, Ordering::Release);
                Reserved::Placed
            }
            None => Reserved::Idle(conn),
        }
    }

    /// Release one permit's worth of capacity. Called from [`Token::drop`].
    /// Hands the capacity to the oldest still-waiting requester as a grant
    /// (regardless of NIC group — a permit lets a requester connect on its
    /// own interface), or returns it to the free counter if none waits.
    fn release_one(self: Arc<Self>) {
        let mut inner = self.inner.lock().expect("waitlist poisoned");
        if let Some(w) = inner.queue.iter_mut().find(|w| w.slot.is_none()) {
            // Reissue a fresh live token into the waiter's slot. The token
            // being dropped (our caller) is already marked non-live by the
            // Drop impl path, so total outstanding capacity is unchanged.
            let token = Token::new(self.clone());
            w.slot = Some(Payload::Grant(token));
            if let Some(waker) = w.waker.take() {
                waker.wake();
            }
            self.waiter_count.fetch_sub(1, Ordering::Release);
        } else {
            inner.available += 1;
        }
    }
}

/// Admission future returned by [`Waitlist::admit`]. Its `Drop` is the
/// cancellation path: an un-consumed registration is removed, and a slot
/// filled after the requester gave up is re-funnelled so its capacity is
/// never stranded.
pub(crate) struct Admit<T> {
    waitlist: Arc<Waitlist<T>>,
    group: NicGroup,
    /// `Some` once registered in the queue; cleared when the outcome is
    /// consumed so `Drop` is a no-op on the resolved path.
    id: Option<u64>,
    done: bool,
}

impl<T> Future for Admit<T> {
    type Output = Admitted<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut inner = this.waitlist.inner.lock().expect("waitlist poisoned");

        match this.id {
            None => {
                // First poll: fast-admit if a permit is free and no one is
                // ahead of us awaiting a slot (FIFO — never barge).
                if inner.available > 0 && inner.no_unfilled() {
                    inner.available -= 1;
                    this.done = true;
                    return Poll::Ready(Admitted::Token(Token::new(this.waitlist.clone())));
                }
                // Register at the tail and park.
                let id = inner.next_id;
                inner.next_id += 1;
                inner.queue.push_back(Waiter {
                    id,
                    group: this.group,
                    slot: None,
                    waker: Some(cx.waker().clone()),
                });
                this.waitlist.waiter_count.fetch_add(1, Ordering::Release);
                this.id = Some(id);
                Poll::Pending
            }
            Some(id) => {
                let pos = inner
                    .queue
                    .iter()
                    .position(|w| w.id == id)
                    .expect("registered waiter present until consumed or cancelled");
                if inner.queue[pos].slot.is_some() {
                    // Served: consume the slot and leave the queue.
                    let payload = inner.queue.remove(pos).unwrap().slot.unwrap();
                    this.id = None;
                    this.done = true;
                    Poll::Ready(match payload {
                        Payload::Warm(conn) => Admitted::Warm(conn),
                        Payload::Grant(token) => Admitted::Token(token),
                    })
                } else {
                    // Still waiting: refresh the waker (task may have moved).
                    inner.queue[pos].waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }
    }
}

impl<T> Drop for Admit<T> {
    fn drop(&mut self) {
        let Some(id) = self.id else {
            return; // never registered, or already consumed
        };
        // Remove our registration. If a payload was placed after we gave
        // up, extract it and re-funnel it OUTSIDE the lock (dropping a
        // Grant token re-enters `release_one`, which re-locks — doing that
        // under the guard would self-deadlock on the non-reentrant mutex).
        let salvaged = {
            let mut inner = self.waitlist.inner.lock().expect("waitlist poisoned");
            match inner.queue.iter().position(|w| w.id == id) {
                Some(pos) => {
                    let waiter = inner.queue.remove(pos).unwrap();
                    match waiter.slot {
                        None => {
                            // Unfilled: our registration counted; drop it.
                            self.waitlist.waiter_count.fetch_sub(1, Ordering::Release);
                            None
                        }
                        // Filled after we gave up: waiter_count was already
                        // decremented at fill time. Salvage the payload.
                        Some(payload) => Some(payload),
                    }
                }
                None => None,
            }
        };

        // Lock released. A salvaged Grant's token drop re-funnels its
        // capacity to the next waiter; a salvaged Warm is dropped (in
        // production the payload re-homes to its owning cache on drop).
        drop(salvaged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    const G0: NicGroup = NicGroup(0);
    const G1: NicGroup = NicGroup(1);

    /// A waker that counts wakes, so tests can assert a park was actually
    /// woken (no lost wakeup) without a runtime.
    struct CountingWaker {
        count: AtomicUsize,
    }
    impl Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn counting() -> (Waker, Arc<CountingWaker>) {
        let inner = Arc::new(CountingWaker {
            count: AtomicUsize::new(0),
        });
        (Waker::from(inner.clone()), inner)
    }

    /// Poll a future once with a given waker.
    fn poll_once<F: Future + Unpin>(fut: &mut F, waker: &Waker) -> Poll<F::Output> {
        let mut cx = Context::from_waker(waker);
        Pin::new(fut).poll(&mut cx)
    }

    fn unwrap_token<T>(a: Admitted<T>) -> Token<T> {
        match a {
            Admitted::Token(t) => t,
            Admitted::Warm(_) => panic!("expected Token, got Warm"),
        }
    }
    fn unwrap_warm<T>(a: Admitted<T>) -> T {
        match a {
            Admitted::Warm(c) => c,
            Admitted::Token(_) => panic!("expected Warm, got Token"),
        }
    }

    // ---- fast path ------------------------------------------------------

    #[test]
    fn fast_admit_takes_a_permit_without_parking() {
        let wl = Waitlist::<u32>::new(2);
        let (w, _) = counting();

        let mut a = wl.admit(G0);
        let t = match poll_once(&mut a, &w) {
            Poll::Ready(out) => unwrap_token(out),
            Poll::Pending => panic!("should fast-admit with a free permit"),
        };
        assert_eq!(wl.available(), 1, "one permit consumed");
        assert_eq!(wl.waiter_count(), 0, "no one parked");
        drop(t);
        assert_eq!(wl.available(), 2, "permit returned on token drop");
    }

    #[test]
    fn fast_admit_exhausts_then_parks() {
        let wl = Waitlist::<u32>::new(1);
        let (w, _) = counting();

        let mut a1 = wl.admit(G0);
        let _t1 = unwrap_token(match poll_once(&mut a1, &w) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!("first should admit"),
        });

        let mut a2 = wl.admit(G0);
        assert!(
            poll_once(&mut a2, &w).is_pending(),
            "second parks (no free permit)"
        );
        assert_eq!(wl.waiter_count(), 1);
    }

    // ---- grant path (permit freed wakes the head) -----------------------

    #[test]
    fn token_drop_grants_to_parked_waiter_and_wakes() {
        let wl = Waitlist::<u32>::new(1);
        let (w1, wake1) = counting();
        let (w2, wake2) = counting();

        let mut a1 = wl.admit(G0);
        let t1 = unwrap_token(match poll_once(&mut a1, &w1) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });

        let mut a2 = wl.admit(G0);
        assert!(poll_once(&mut a2, &w2).is_pending());
        assert_eq!(wl.waiter_count(), 1);

        // Free the permit -> should be granted to a2 and wake it.
        drop(t1);
        assert_eq!(wake2.count.load(Ordering::SeqCst), 1, "parked waiter woken");
        assert_eq!(wake1.count.load(Ordering::SeqCst), 0, "holder not woken");
        assert_eq!(wl.waiter_count(), 0, "waiter served");

        let t2 = unwrap_token(match poll_once(&mut a2, &w2) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!("granted waiter should be ready"),
        });
        assert_eq!(wl.available(), 0, "permit rode the grant, not returned");
        drop(t2);
        assert_eq!(wl.available(), 1, "permit finally freed, no waiters");
    }

    // ---- warm reserve path (same-group return loans, no permit moves) ---

    #[test]
    fn reserve_same_group_loans_warm_and_wakes() {
        let wl = Waitlist::<u32>::new(1);
        let (w1, _) = counting();
        let (w2, wake2) = counting();

        // Hold the only permit; park a same-group waiter.
        let mut a1 = wl.admit(G0);
        let _t1 = unwrap_token(match poll_once(&mut a1, &w1) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        let mut a2 = wl.admit(G0);
        assert!(poll_once(&mut a2, &w2).is_pending());

        // A peer returns a warm conn (id 42) -> loaned to a2.
        match wl.try_reserve_same_group(G0, 42) {
            Reserved::Placed => {}
            Reserved::Idle(_) => panic!("should place into the waiter"),
        }
        assert_eq!(wake2.count.load(Ordering::SeqCst), 1);
        assert_eq!(wl.waiter_count(), 0);

        let conn = unwrap_warm(match poll_once(&mut a2, &w2) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        assert_eq!(conn, 42, "got the loaned connection");
        assert_eq!(
            wl.available(),
            0,
            "warm loan moved no permit (still held by t1)"
        );
    }

    #[test]
    fn reserve_no_waiter_hands_back_for_idle() {
        let wl = Waitlist::<u32>::new(4);
        match wl.try_reserve_same_group(G0, 7) {
            Reserved::Idle(c) => assert_eq!(c, 7),
            Reserved::Placed => panic!("no waiter — must hand back"),
        }
    }

    #[test]
    fn reserve_skips_cross_group_waiter() {
        let wl = Waitlist::<u32>::new(1);
        let (w1, _) = counting();
        let (w2, _) = counting();
        let _t1 = unwrap_token(match poll_once(&mut wl.admit(G0), &w1) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        // Park a G1 waiter; a G0 return must NOT be loaned to it.
        let mut a2 = wl.admit(G1);
        assert!(poll_once(&mut a2, &w2).is_pending());
        match wl.try_reserve_same_group(G0, 99) {
            Reserved::Idle(c) => assert_eq!(c, 99, "cross-group return handed back"),
            Reserved::Placed => panic!("must not loan across NIC groups"),
        }
    }

    // ---- FIFO ordering --------------------------------------------------

    #[test]
    fn grants_go_to_oldest_waiter_first() {
        let wl = Waitlist::<u32>::new(1);
        let (w0, _) = counting();
        let t = unwrap_token(match poll_once(&mut wl.admit(G0), &w0) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });

        let (wa, _) = counting();
        let (wb, _) = counting();
        let mut a = wl.admit(G0);
        let mut b = wl.admit(G0);
        assert!(poll_once(&mut a, &wa).is_pending());
        assert!(poll_once(&mut b, &wb).is_pending());
        assert_eq!(wl.waiter_count(), 2);

        // First freed permit -> oldest (a). b still parked.
        drop(t);
        let ta = unwrap_token(match poll_once(&mut a, &wa) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!("oldest should be served first"),
        });
        assert!(
            poll_once(&mut b, &wb).is_pending(),
            "younger still waits"
        );

        // Next freed permit -> b.
        drop(ta);
        let _tb = unwrap_token(match poll_once(&mut b, &wb) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!("second waiter should now be served"),
        });
        assert_eq!(wl.waiter_count(), 0);
    }

    // ---- cancellation ---------------------------------------------------

    #[test]
    fn cancel_unfilled_waiter_deregisters() {
        let wl = Waitlist::<u32>::new(1);
        let (w0, _) = counting();
        let _t = unwrap_token(match poll_once(&mut wl.admit(G0), &w0) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        let (w1, _) = counting();
        let mut a = wl.admit(G0);
        assert!(poll_once(&mut a, &w1).is_pending());
        assert_eq!(wl.waiter_count(), 1);
        drop(a); // give up
        assert_eq!(wl.waiter_count(), 0, "cancel deregisters");
    }

    #[test]
    fn cancel_after_grant_refunnels_to_next_waiter() {
        // The double-serve tail: a waiter is granted a permit but gives up
        // before consuming it; the grant must re-funnel to the next waiter,
        // never stranding capacity.
        let wl = Waitlist::<u32>::new(1);
        let (w0, _) = counting();
        let t = unwrap_token(match poll_once(&mut wl.admit(G0), &w0) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });

        let (wa, _) = counting();
        let (wb, wake_b) = counting();
        let mut a = wl.admit(G0);
        let mut b = wl.admit(G0);
        assert!(poll_once(&mut a, &wa).is_pending());
        assert!(poll_once(&mut b, &wb).is_pending());

        // Free a permit: granted to `a` (oldest). Do NOT poll `a`.
        drop(t);
        assert_eq!(wl.waiter_count(), 1, "a served (slot filled), b still waits");

        // `a` gives up WITHOUT consuming its grant -> must re-funnel to `b`.
        drop(a);
        assert_eq!(
            wake_b.count.load(Ordering::SeqCst),
            1,
            "b woken by the re-funnelled grant"
        );
        assert_eq!(wl.waiter_count(), 0, "b now served");

        let _tb = unwrap_token(match poll_once(&mut b, &wb) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!("b should receive the re-funnelled permit"),
        });
        // Conservation: exactly one permit exists; b holds it now.
        assert_eq!(wl.available(), 0);
        drop(_tb);
        assert_eq!(wl.available(), 1, "capacity conserved end to end");
    }

    #[test]
    fn cancel_after_warm_reserve_drops_payload_not_capacity() {
        let wl = Waitlist::<u32>::new(1);
        let (w0, _) = counting();
        let _t = unwrap_token(match poll_once(&mut wl.admit(G0), &w0) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        let (w1, _) = counting();
        let mut a = wl.admit(G0);
        assert!(poll_once(&mut a, &w1).is_pending());

        // Loan a warm conn into a's slot, then a gives up before consuming.
        assert!(matches!(
            wl.try_reserve_same_group(G0, 5),
            Reserved::Placed
        ));
        assert_eq!(wl.waiter_count(), 0);
        drop(a); // warm payload dropped; no permit was involved
        // The held permit (_t) is untouched; capacity unchanged.
        assert_eq!(wl.available(), 0, "warm loan never moved a permit");
    }

    // ---- no-barge invariant --------------------------------------------

    #[test]
    fn fast_admit_does_not_barge_past_a_parked_waiter() {
        let wl = Waitlist::<u32>::new(1);
        let (w0, _) = counting();
        let t = unwrap_token(match poll_once(&mut wl.admit(G0), &w0) {
            Poll::Ready(o) => o,
            Poll::Pending => panic!(),
        });
        let (w1, _) = counting();
        let mut a = wl.admit(G0);
        assert!(poll_once(&mut a, &w1).is_pending(), "a parks");

        // Free the permit AND have a newcomer try to fast-admit before `a`
        // is repolled. The permit was granted to `a` on drop, so
        // available is 0 and the newcomer must park behind `a`.
        drop(t);
        let (w2, _) = counting();
        let mut c = wl.admit(G0);
        assert!(
            poll_once(&mut c, &w2).is_pending(),
            "newcomer must not barge the granted-but-unconsumed waiter"
        );
        assert_eq!(wl.available(), 0);
    }
}

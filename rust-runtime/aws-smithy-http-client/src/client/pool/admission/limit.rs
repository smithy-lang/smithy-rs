/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection cap enforcement: reserves per-host then global semaphore
//! permits in `poll_ready` and consumes them in `call`, providing faithful
//! tower backpressure.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use aws_smithy_runtime_api::box_error::BoxError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::PollSemaphore;
use tower::Service;

use super::super::connection::{ConnectCtx, ConnectionPermit, EstablishedConnection};
use super::super::stats::EstablishingGuard;
use super::super::PeerReclaimHandle;

/// Enforces the pool's connection cap by reserving per-host then global
/// semaphore permits in [`poll_ready`](Service::poll_ready) and consuming
/// them in [`call`](Service::call).
///
/// Assembles [`EstablishedConnection`] on the unwind by attaching the
/// permits to the inner service's `(IO, EstablishingGuard)` output.
/// Implements faithful tower backpressure: callers block at the service
/// boundary rather than piling up inside `call`.
pub(crate) struct ConnectionLimit<S> {
    inner: S,
    global: Option<PollSemaphore>,
    per_host: Option<PollSemaphore>,
    global_permit: Option<OwnedSemaphorePermit>,
    per_host_permit: Option<OwnedSemaphorePermit>,
    /// Cross-partition active reclaim, applied only to the global cap. When
    /// the global semaphore is exhausted, this frees a fungible permit by
    /// dropping an over-supplied peer's idle connection before parking —
    /// preventing a starved partition from waiting on permits stranded in
    /// other partitions' idle sets. `None` when reclaim is unavailable
    /// (no registry / single-partition pool): `poll_ready` then simply
    /// parks on the semaphore as before.
    reclaim: Option<PeerReclaimHandle>,
}

impl<S> ConnectionLimit<S> {
    pub(crate) fn new(
        inner: S,
        global: Option<Arc<Semaphore>>,
        per_host: Option<Arc<Semaphore>>,
        reclaim: Option<PeerReclaimHandle>,
    ) -> Self {
        Self {
            inner,
            global: global.map(PollSemaphore::new),
            per_host: per_host.map(PollSemaphore::new),
            global_permit: None,
            per_host_permit: None,
            reclaim,
        }
    }
}

impl<S: Clone> Clone for ConnectionLimit<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            global: self.global.clone(),
            per_host: self.per_host.clone(),
            global_permit: None,
            per_host_permit: None,
            reclaim: self.reclaim.clone(),
        }
    }
}

impl<S, IO> Service<ConnectCtx> for ConnectionLimit<S>
where
    S: Service<ConnectCtx, Response = (IO, EstablishingGuard)> + Clone + Send + 'static,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
    IO: Send + 'static,
{
    type Response = EstablishedConnection<IO>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Acquire per-host FIRST, then global. Never hold a global permit
        // while waiting on a per-host permit.
        if self.per_host_permit.is_none() {
            if let Some(ref mut sem) = self.per_host {
                self.per_host_permit = ready!(sem.poll_acquire(cx));
                debug_assert!(
                    self.per_host_permit.is_some(),
                    "per-host semaphore is never closed"
                );
            }
        }

        if self.global_permit.is_none() {
            if let Some(ref mut sem) = self.global {
                match sem.poll_acquire(cx) {
                    Poll::Ready(permit) => {
                        debug_assert!(permit.is_some(), "global semaphore is never closed");
                        self.global_permit = permit;
                    }
                    Poll::Pending => {
                        // The global cap is exhausted. Under keep-alive the
                        // held permits sit on *idle* connections that only
                        // return their permit on drop (idle-timeout), so a
                        // partition holding none of them can wait many
                        // timeout cycles — the cross-partition starvation
                        // tail. Before surrendering to that wait, try to
                        // reclaim: drop an over-supplied peer's idle
                        // connection, which returns its fungible permit to
                        // this same semaphore.
                        //
                        // On a successful reclaim, re-poll immediately: the
                        // freed permit is available now, so `poll_acquire`
                        // typically returns `Ready` on this same wake. It is
                        // not guaranteed — a concurrent acquirer may take the
                        // freed permit first — but that is benign: the
                        // re-poll then returns `Pending` with our waker
                        // registered, so we are woken on the next release
                        // exactly as an un-reclaimed park would be. We do not
                        // loop on reclaim: one freed permit satisfies one
                        // waiter, and repeated eviction under contention would
                        // churn peers' warm connections.
                        let reclaimed = self
                            .reclaim
                            .as_ref()
                            .is_some_and(PeerReclaimHandle::try_free_global);
                        if reclaimed {
                            self.global_permit = ready!(sem.poll_acquire(cx));
                            debug_assert!(
                                self.global_permit.is_some(),
                                "global semaphore is never closed"
                            );
                        } else {
                            return Poll::Pending;
                        }
                    }
                }
            }
        }

        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        // Permits are RESERVED in `poll_ready` and consumed here — the
        // faithful tower backpressure path: a capped worker parks in
        // `poll_ready` and never reaches `call`, so it starts no connect and
        // orphans nothing.
        //
        // The cache drives this layer's `poll_ready` before `call` on its
        // connect race, so the reservation is present here on the H1 cache
        // path; the `debug_assert`s below encode that contract.
        //
        // The `None` branches remain as a defensive fallback for any caller
        // that reaches `call` without a prior `poll_ready` (e.g. a connect
        // path not yet audited against this contract). They acquire inline
        // BEFORE starting the connect, so even the fallback never starts an
        // unpermitted connect — it just blocks in `call` instead of
        // `poll_ready`.
        let reserved_per_host = self.per_host_permit.take();
        let reserved_global = self.global_permit.take();

        debug_assert!(
            self.per_host.is_none() || reserved_per_host.is_some(),
            "ConnectionLimit::call reached without a reserved per-host permit; \
             poll_ready must be driven before call"
        );
        debug_assert!(
            self.global.is_none() || reserved_global.is_some(),
            "ConnectionLimit::call reached without a reserved global permit; \
             poll_ready must be driven before call"
        );

        let per_host_sem = self.per_host.as_ref().map(|ps| ps.clone_inner());
        let global_sem = self.global.as_ref().map(|gs| gs.clone_inner());

        // Clone inner but DO NOT call it yet: the connect (and the
        // `EstablishingGuard` born inside it) must not start until the permit
        // is in hand, or a permit-blocked future would hold a guard and inflate
        // `establishing` — the accounting half of the explosion bug. On the
        // reserved path this is a no-op wait (permit already held).
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Acquire per-host THEN global (never hold global while waiting on
            // per-host), reusing any permit already reserved by `poll_ready`.
            let per_host_permit = match reserved_per_host {
                Some(p) => Some(p),
                None => match per_host_sem {
                    Some(sem) => Some(
                        sem.acquire_owned()
                            .await
                            .map_err(|_| -> BoxError { "pool closed".into() })?,
                    ),
                    None => None,
                },
            };
            let global_permit = match reserved_global {
                Some(p) => Some(p),
                None => match global_sem {
                    Some(sem) => Some(
                        sem.acquire_owned()
                            .await
                            .map_err(|_| -> BoxError { "pool closed".into() })?,
                    ),
                    None => None,
                },
            };

            let permit = Arc::new(ConnectionPermit::new(global_permit, per_host_permit));
            // Permit held: only now start the connect (and its
            // `EstablishingGuard`).
            let (io, establishing) = inner.call(ctx).await.map_err(Into::into)?;
            Ok(EstablishedConnection {
                io,
                permit,
                establishing,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::pool::stats::{ConnectionCounters, EstablishingGuard};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::Semaphore;
    use tower_test::mock::Spawn;

    /// Minimal IO stand-in for tests.
    #[derive(Debug)]
    struct MockIo;

    /// A cloneable mock inner service that produces `(MockIo, EstablishingGuard)`.
    #[derive(Clone)]
    struct MockInner {
        calls: Arc<AtomicUsize>,
        counters: Arc<ConnectionCounters>,
    }

    impl MockInner {
        fn new() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                counters: Arc::new(ConnectionCounters::default()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Service<ConnectCtx> for MockInner {
        type Response = (MockIo, EstablishingGuard);
        type Error = BoxError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _ctx: ConnectCtx) -> Self::Future {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let counters = self.counters.clone();
            Box::pin(async move { Ok((MockIo, EstablishingGuard::new(counters))) })
        }
    }

    fn test_ctx() -> ConnectCtx {
        ConnectCtx::new("http://example.com".parse().unwrap(), None)
    }

    /// Global sem capacity 1: first service reserves in poll_ready; a second
    /// Spawn on the same sem returns Pending; dropping the first's permit wakes
    /// the second.
    #[tokio::test]
    async fn poll_ready_pending_when_global_exhausted() {
        let global = Arc::new(Semaphore::new(1));
        let inner = MockInner::new();

        let svc1 = ConnectionLimit::new(inner.clone(), Some(global.clone()), None, None);
        let svc2 = ConnectionLimit::new(inner.clone(), Some(global.clone()), None, None);

        let mut spawn1 = Spawn::new(svc1);
        let mut spawn2 = Spawn::new(svc2);

        // First service acquires the sole global permit in poll_ready.
        assert!(
            spawn1.poll_ready().is_ready(),
            "first service should be Ready (global permit available)"
        );

        // Second service sees the global exhausted -> Pending.
        assert!(
            spawn2.poll_ready().is_pending(),
            "second service should be Pending (global exhausted)"
        );

        // Drop the first service (releases the permit) -> wakes the second.
        drop(spawn1);
        assert!(
            spawn2.is_woken(),
            "second service should be woken after first drops its permit"
        );
        assert!(
            spawn2.poll_ready().is_ready(),
            "second service should be Ready after wake"
        );
    }

    /// Per-host sem capacity 1: same pattern as global test.
    #[tokio::test]
    async fn poll_ready_pending_when_per_host_exhausted() {
        let per_host = Arc::new(Semaphore::new(1));
        let inner = MockInner::new();

        let svc1 = ConnectionLimit::new(inner.clone(), None, Some(per_host.clone()), None);
        let svc2 = ConnectionLimit::new(inner.clone(), None, Some(per_host.clone()), None);

        let mut spawn1 = Spawn::new(svc1);
        let mut spawn2 = Spawn::new(svc2);

        assert!(
            spawn1.poll_ready().is_ready(),
            "first service should be Ready (per-host permit available)"
        );
        assert!(
            spawn2.poll_ready().is_pending(),
            "second service should be Pending (per-host exhausted)"
        );

        drop(spawn1);
        assert!(
            spawn2.is_woken(),
            "second service should be woken after first drops its permit"
        );
        assert!(
            spawn2.poll_ready().is_ready(),
            "second service should be Ready after wake"
        );
    }

    /// Per-host acquired BEFORE global: with per-host exhausted and global
    /// large, poll_ready is Pending and no global permit is consumed.
    #[tokio::test]
    async fn per_host_acquired_before_global() {
        let per_host = Arc::new(Semaphore::new(0)); // exhausted
        let global = Arc::new(Semaphore::new(10));
        let inner = MockInner::new();

        let svc = ConnectionLimit::new(inner, Some(global.clone()), Some(per_host.clone()), None);
        let mut spawn = Spawn::new(svc);

        assert!(
            spawn.poll_ready().is_pending(),
            "should be Pending (per-host exhausted)"
        );
        assert_eq!(
            global.available_permits(),
            10,
            "global permits unchanged (per-host blocks first)"
        );
    }

    /// After a Ready poll_ready, call runs inner once and the returned
    /// EstablishedConnection holds the permits. A fresh poll_ready is needed
    /// before the next call.
    #[tokio::test]
    async fn permit_consumed_once_in_call() {
        let global = Arc::new(Semaphore::new(2));
        let per_host = Arc::new(Semaphore::new(2));
        let inner = MockInner::new();

        let svc = ConnectionLimit::new(
            inner.clone(),
            Some(global.clone()),
            Some(per_host.clone()),
            None,
        );
        let mut spawn = Spawn::new(svc);

        assert!(spawn.poll_ready().is_ready());
        // After poll_ready, permits should be reserved (not yet in semaphore).
        assert_eq!(global.available_permits(), 1, "global permit reserved");
        assert_eq!(per_host.available_permits(), 1, "per-host permit reserved");

        let ctx = test_ctx();
        let established = spawn.call(ctx).await.unwrap();
        assert_eq!(inner.calls(), 1, "inner called exactly once");

        // Permits are still held on the EstablishedConnection.
        assert_eq!(global.available_permits(), 1);
        assert_eq!(per_host.available_permits(), 1);

        // Drop the established connection -> permits return.
        drop(established);
        assert_eq!(global.available_permits(), 2);
        assert_eq!(per_host.available_permits(), 2);
    }

    /// Per-host capacity 1, global capacity 0: poll_ready reserves per-host
    /// then Pends on global; drop the service; assert per-host released.
    #[tokio::test]
    async fn half_acquire_released_on_drop() {
        let per_host = Arc::new(Semaphore::new(1));
        let global = Arc::new(Semaphore::new(0)); // exhausted
        let inner = MockInner::new();

        let svc = ConnectionLimit::new(inner, Some(global.clone()), Some(per_host.clone()), None);
        let mut spawn = Spawn::new(svc);

        // Per-host is acquired, then blocks on global.
        assert!(spawn.poll_ready().is_pending());
        // Per-host permit should be held (reserved in poll_ready).
        assert_eq!(
            per_host.available_permits(),
            0,
            "per-host permit held by the service"
        );

        // Drop the service -> per-host permit released.
        drop(spawn);
        assert_eq!(
            per_host.available_permits(),
            1,
            "per-host permit released on drop"
        );
    }

    /// After reserving in poll_ready, cloning the service yields a clone
    /// whose permit fields are None (must re-acquire).
    #[tokio::test]
    async fn clone_resets_permits() {
        let global = Arc::new(Semaphore::new(2));
        let inner = MockInner::new();

        let svc = ConnectionLimit::new(inner, Some(global.clone()), None, None);
        let mut spawn = Spawn::new(svc);

        assert!(spawn.poll_ready().is_ready());
        // One permit reserved.
        assert_eq!(global.available_permits(), 1);

        // Clone via get_ref (which is the inner service).
        let cloned = spawn.get_ref().clone();
        let mut spawn_clone = Spawn::new(cloned);

        // The clone has no reserved permit yet.
        // poll_ready on clone reserves a second permit.
        assert!(spawn_clone.poll_ready().is_ready());
        assert_eq!(
            global.available_permits(),
            0,
            "both original and clone hold permits"
        );
    }

    /// Both sems None -> poll_ready forwards to inner, call passes through,
    /// inner runs once.
    #[tokio::test]
    async fn unbounded_passes_through() {
        let inner = MockInner::new();

        let svc = ConnectionLimit::new(inner.clone(), None, None, None);
        let mut spawn = Spawn::new(svc);

        assert!(spawn.poll_ready().is_ready());
        let ctx = test_ctx();
        let _established = spawn.call(ctx).await.unwrap();
        assert_eq!(inner.calls(), 1);
    }
}

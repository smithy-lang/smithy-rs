/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/*
 * Portions of this file are derived from hyper-util
 * (https://github.com/hyperium/hyper-util), licensed under MIT:
 *
 *   Copyright (c) 2023-2025 Sean McArthur
 *
 *   Permission is hereby granted, free of charge, to any person obtaining
 *   a copy of this software and associated documentation files (the
 *   "Software"), to deal in the Software without restriction, including
 *   without limitation the rights to use, copy, modify, merge, publish,
 *   distribute, sublicense, and/or sell copies of the Software, and to
 *   permit persons to whom the Software is furnished to do so, subject
 *   to the following conditions:
 *
 *   The above copyright notice and this permission notice shall be
 *   included in all copies or substantial portions of the Software.
 *
 *   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 *   EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 *   MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
 *   IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 *   CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
 *   TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
 *   SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 *
 * Modifications by Amazon.com, Inc. or its affiliates:
 *   Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *   SPDX-License-Identifier: Apache-2.0
 *
 * The derivative work as a whole is licensed under Apache-2.0 as part of the
 * smithy-rs project. The MIT notice above applies to the original portions
 * as required by that license.
 *
 * Source:   hyper-util src/client/pool/cache.rs
 * Upstream: https://github.com/hyperium/hyper-util
 * Commit:   e1c5a6c89bfaed11fb34bd483fe9ba616f403791
 *
 * Modifications from upstream:
 *   1. Changed `pub use self::internal::builder;` to `pub(crate) use ...`
 *      and dropped the three `#[cfg(docsrs)] pub use` lines. The composable
 *      pool internals are `pub(crate)` in this crate — no types from this
 *      file are exposed in the smithy-rs public API.
 *   2. Added `#![allow(dead_code, unreachable_pub)]` so the file can be
 *      kept close to upstream even when individual items aren't used yet.
 *   3. Dropped the module- and struct-level rustdoc sections that reference
 *      "Unnameable" (that rustdoc pattern is specific to hyper-util's public
 *      API; these types aren't public here).
 *   4. Added `Cached::discard(self)` — consumes self and prevents
 *      reinsertion into the pool on drop. See `// SDK MODIFICATION` marker
 *      below. Used to drop connections that a caller has learned are bad
 *      (poisoned, GOAWAY, etc.) between checkout and `poll_ready`. The
 *      upstream API requires the inner service's `poll_ready` to error
 *      in order to skip reinsertion, which overloads `poll_ready` semantics.
 */

//! A cache of services
//!
//! The cache is a single list of cached services, bundled with a `MakeService`.
//! Calling the cache returns either an existing service, or makes a new one.
//! The returned `impl Service` can be used to send requests, and when dropped,
//! it will try to be returned back to the cache.

#![allow(dead_code, unreachable_pub)]

pub(crate) use self::internal::{builder, Cached};

mod internal {
    use std::collections::VecDeque;
    use std::fmt;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, Weak};
    use std::task::{self, ready, Poll};

    use futures_util::future;
    use tokio::sync::oneshot;
    use tower::util::Oneshot;
    use tower_service::Service;

    use super::events;

    /// Start a builder to construct a `Cache` pool.
    pub fn builder() -> Builder<events::Ignore> {
        Builder {
            events: events::Ignore,
        }
    }

    /// A cache pool of services from the inner make service.
    #[derive(Debug)]
    pub struct Cache<M, Dst, Ev>
    where
        M: Service<Dst>,
    {
        connector: M,
        shared: Arc<Mutex<Shared<M::Response>>>,
        events: Ev,
    }

    /// A builder to configure a `Cache`.
    #[derive(Debug)]
    pub struct Builder<Ev> {
        events: Ev,
    }

    /// A cached service returned from a [`Cache`].
    ///
    /// Implements `Service` by delegating to the inner service. Once dropped,
    /// tries to reinsert into the `Cache`.
    pub struct Cached<S> {
        is_closed: bool,
        inner: Option<S>,
        shared: Weak<Mutex<Shared<S>>>,
        // todo: on_idle
    }

    pub enum CacheFuture<M, Dst, Ev>
    where
        M: Service<Dst>,
    {
        // SDK MODIFICATION: the racing connect arm is `Oneshot<M, Dst>`
        // (reserve-then-call), NOT a bare `M::Future`. This drives the
        // connector's `poll_ready` inside the race so the connector's
        // readiness contract is honored on the idle->miss transition (the
        // upstream cache returns `Ready` from its own `poll_ready` on an
        // idle hit without polling the connector, so a later miss can reach
        // `connector.call` unreserved). Driving readiness here also lets the
        // `select` wait on BOTH wake channels at once: the waiter (idle
        // return) and the connector's `poll_ready` (e.g. a connection-cap
        // permit freeing).
        Racing {
            shared: Arc<Mutex<Shared<M::Response>>>,
            select: future::Select<oneshot::Receiver<M::Response>, Oneshot<M, Dst>>,
            events: Ev,
        },
        Connecting {
            // TODO: could be Weak even here...
            shared: Arc<Mutex<Shared<M::Response>>>,
            future: Oneshot<M, Dst>,
        },
        Cached {
            svc: Option<Cached<M::Response>>,
        },
    }

    // shouldn't be pub
    #[derive(Debug)]
    pub struct Shared<S> {
        services: Vec<S>,
        // SDK MODIFICATION: `VecDeque` (was `Vec`) so waiters are served
        // FIFO. `Shared::put` wakes via `pop_front` (oldest first); `call`
        // registers via `push_back`. Upstream uses `Vec` + `pop` (LIFO),
        // which starves the oldest waiters under sustained oversubscription
        // (each freed connection goes to the most-recently-parked worker) —
        // the cause of the conn-cap P999 tail (0.1% of requests wait the
        // whole run). FIFO bounds the worst-case wait to arrival order.
        waiters: VecDeque<oneshot::Sender<S>>,
    }

    // impl Builder

    impl<Ev> Builder<Ev> {
        /// Provide a `Future` executor to be used by the `Cache`.
        pub fn executor<E>(self, exec: E) -> Builder<events::WithExecutor<E>> {
            Builder {
                events: events::WithExecutor(exec),
            }
        }

        /// Build a `Cache` pool around the `connector`.
        pub fn build<M, Dst>(self, connector: M) -> Cache<M, Dst, Ev>
        where
            M: Service<Dst>,
        {
            Cache {
                connector,
                events: self.events,
                shared: Arc::new(Mutex::new(Shared {
                    services: Vec::new(),
                    waiters: VecDeque::new(),
                })),
            }
        }
    }

    // impl Cache

    impl<M, Dst, Ev> Cache<M, Dst, Ev>
    where
        M: Service<Dst>,
    {
        /// Retain all cached services indicated by the predicate.
        pub fn retain<F>(&mut self, predicate: F)
        where
            F: FnMut(&mut M::Response) -> bool,
        {
            self.shared.lock().unwrap().services.retain_mut(predicate);
        }

        /// Check whether this cache has no cached services.
        pub fn is_empty(&self) -> bool {
            self.shared.lock().unwrap().services.is_empty()
        }

        // SDK MODIFICATION: added `try_pop_idle` so a caller can remove an
        // idle cached service to free whatever resource it holds, instead
        // of waiting for it to be re-handed-out or evicted.
        /// Remove and return one idle cached service, if any.
        ///
        /// Unlike [`Service::call`], which wraps the taken service in a
        /// [`Cached`] that returns to the pool on drop, this hands back the
        /// raw service with no return-to-pool wrapper: dropping it drops the
        /// service outright. Serialized against [`Self::retain`] by the
        /// shared `Mutex`, so a popped service is removed before a retain
        /// pass can observe it.
        pub fn try_pop_idle(&self) -> Option<M::Response> {
            self.shared.lock().unwrap().services.pop()
        }

        // SDK MODIFICATION: added `try_checkout_idle` so a caller can take an
        // idle cached service for one use and have it return to the pool on
        // drop, without going through `Service::call` (which may also start a
        // new connection when none is idle).
        /// Take one idle cached service, if any, wrapped so it returns to the
        /// pool on drop.
        ///
        /// Unlike [`Self::try_pop_idle`], which hands back the raw service
        /// (dropping it drops the service), this returns the same
        /// [`Cached`] wrapper [`Service::call`] produces: dropping it
        /// re-inserts the service into the pool. Unlike [`Service::call`],
        /// it never starts a new connection — it returns `None` when no
        /// service is idle. Serialized against [`Self::retain`] and
        /// [`Self::try_pop_idle`] by the shared `Mutex`.
        pub fn try_checkout_idle(&self) -> Option<Cached<M::Response>> {
            let inner = self.shared.lock().unwrap().services.pop()?;
            Some(Cached::new(inner, Arc::downgrade(&self.shared)))
        }
    }

    impl<M, Dst, Ev> Service<Dst> for Cache<M, Dst, Ev>
    where
        // SDK MODIFICATION: `M: Clone` (was absent). The racing connect arm
        // is now `Oneshot<M, Dst>`, which owns a connector clone so it can
        // drive `poll_ready` then `call` across awaits. The `Events` bound's
        // `CF` is correspondingly `Oneshot<M, Dst>` (the lost-race future),
        // not `M::Future`.
        M: Service<Dst> + Clone,
        M::Future: Unpin,
        M::Response: Unpin,
        Ev: events::Events<BackgroundConnect<Oneshot<M, Dst>, M::Response>> + Clone + Unpin,
    {
        type Response = Cached<M::Response>;
        type Error = M::Error;
        type Future = CacheFuture<M, Dst, Ev>;

        // SDK MODIFICATION: `poll_ready` no longer forwards to the connector.
        // Readiness (and thus permit reservation, for a limiter connector) is
        // driven inside the racing future (`Oneshot`), so driving it here too
        // would double-reserve. The cache is always ready to accept a
        // checkout; whether a connection is reused or established (and whether
        // establishment must wait on a permit) is resolved in the returned
        // future.
        fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, target: Dst) -> Self::Future {
            // 1. If already cached, easy!
            let waiter = {
                let mut locked = self.shared.lock().unwrap();
                if let Some(found) = locked.take() {
                    return CacheFuture::Cached {
                        svc: Some(Cached::new(found, Arc::downgrade(&self.shared))),
                    };
                }

                let (tx, rx) = oneshot::channel();
                locked.waiters.push_back(tx);
                rx
            };

            // 2. Otherwise, race an idle-return waiter against a
            //    reserve-then-connect (`Oneshot` drives the connector's
            //    `poll_ready` before `call`). SDK MODIFICATION: was
            //    `future::select(waiter, self.connector.call(target))`, which
            //    called the connector WITHOUT first driving `poll_ready` —
            //    the source of the connection-cap TOCTOU.
            CacheFuture::Racing {
                shared: self.shared.clone(),
                select: future::select(waiter, Oneshot::new(self.connector.clone(), target)),
                events: self.events.clone(),
            }
        }
    }

    impl<M, Dst, Ev> Clone for Cache<M, Dst, Ev>
    where
        M: Service<Dst> + Clone,
        Ev: Clone,
    {
        fn clone(&self) -> Self {
            Self {
                connector: self.connector.clone(),
                events: self.events.clone(),
                shared: self.shared.clone(),
            }
        }
    }

    impl<M, Dst, Ev> Future for CacheFuture<M, Dst, Ev>
    where
        // SDK MODIFICATION: `M: Clone` + the `Events` CF is `Oneshot<M, Dst>`
        // (see the `Service` impl above for rationale).
        M: Service<Dst> + Clone,
        M::Future: Unpin,
        M::Response: Unpin,
        Ev: events::Events<BackgroundConnect<Oneshot<M, Dst>, M::Response>> + Unpin,
    {
        type Output = Result<Cached<M::Response>, M::Error>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
            loop {
                match &mut *self.as_mut() {
                    CacheFuture::Racing {
                        shared,
                        select,
                        events,
                    } => {
                        match ready!(Pin::new(select).poll(cx)) {
                            future::Either::Left((Err(_pool_closed), connecting)) => {
                                // pool was dropped, so we'll never get it from a waiter,
                                // but if this future still exists, then the user still
                                // wants a connection. just wait for the connecting
                                *self = CacheFuture::Connecting {
                                    shared: shared.clone(),
                                    future: connecting,
                                };
                            }
                            future::Either::Left((Ok(pool_got), connecting)) => {
                                events.on_race_lost(BackgroundConnect {
                                    future: connecting,
                                    shared: Arc::downgrade(&shared),
                                });
                                return Poll::Ready(Ok(Cached::new(
                                    pool_got,
                                    Arc::downgrade(&shared),
                                )));
                            }
                            future::Either::Right((connected, _waiter)) => {
                                let inner = connected?;
                                return Poll::Ready(Ok(Cached::new(
                                    inner,
                                    Arc::downgrade(&shared),
                                )));
                            }
                        }
                    }
                    CacheFuture::Connecting { shared, future } => {
                        let inner = ready!(Pin::new(future).poll(cx))?;
                        return Poll::Ready(Ok(Cached::new(inner, Arc::downgrade(&shared))));
                    }
                    CacheFuture::Cached { svc } => {
                        return Poll::Ready(Ok(svc.take().unwrap()));
                    }
                }
            }
        }
    }

    // impl Cached

    impl<S> Cached<S> {
        fn new(inner: S, shared: Weak<Mutex<Shared<S>>>) -> Self {
            Cached {
                is_closed: false,
                inner: Some(inner),
                shared,
            }
        }

        // TODO: inner()? looks like `tower` likes `get_ref()` and `get_mut()`.

        /// Get a reference to the inner service.
        pub fn inner(&self) -> &S {
            self.inner.as_ref().expect("inner only taken in drop")
        }

        /// Get a mutable reference to the inner service.
        pub fn inner_mut(&mut self) -> &mut S {
            self.inner.as_mut().expect("inner only taken in drop")
        }

        // SDK MODIFICATION: added `discard` so callers can prevent a bad
        // connection from returning to the pool without having to cause a
        // synthetic `poll_ready` error.
        /// Prevent this cached service from being returned to the pool.
        ///
        /// Consumes `self`; the inner service is dropped without
        /// reinsertion, regardless of whether it is still healthy.
        pub fn discard(mut self) {
            self.is_closed = true;
        }
    }

    impl<S, Req> Service<Req> for Cached<S>
    where
        S: Service<Req>,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = S::Future;

        fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.as_mut().unwrap().poll_ready(cx).map_err(|err| {
                self.is_closed = true;
                err
            })
        }

        fn call(&mut self, req: Req) -> Self::Future {
            self.inner.as_mut().unwrap().call(req)
        }
    }

    impl<S> Drop for Cached<S> {
        fn drop(&mut self) {
            if self.is_closed {
                return;
            }
            if let Some(value) = self.inner.take() {
                if let Some(shared) = self.shared.upgrade() {
                    if let Ok(mut shared) = shared.lock() {
                        shared.put(value);
                    }
                }
            }
        }
    }

    impl<S: fmt::Debug> fmt::Debug for Cached<S> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("Cached")
                .field(self.inner.as_ref().unwrap())
                .finish()
        }
    }

    // impl Shared

    impl<V> Shared<V> {
        fn put(&mut self, val: V) {
            let mut val = Some(val);
            while let Some(tx) = self.waiters.pop_front() {
                if !tx.is_closed() {
                    match tx.send(val.take().unwrap()) {
                        Ok(()) => break,
                        Err(v) => {
                            val = Some(v);
                        }
                    }
                }
            }

            if let Some(val) = val {
                self.services.push(val);
            }
        }

        fn take(&mut self) -> Option<V> {
            // TODO: take in a loop
            self.services.pop()
        }
    }

    pub struct BackgroundConnect<CF, S> {
        future: CF,
        shared: Weak<Mutex<Shared<S>>>,
    }

    impl<CF, S, E> Future for BackgroundConnect<CF, S>
    where
        CF: Future<Output = Result<S, E>> + Unpin,
    {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
            match ready!(Pin::new(&mut self.future).poll(cx)) {
                Ok(svc) => {
                    if let Some(shared) = self.shared.upgrade() {
                        if let Ok(mut locked) = shared.lock() {
                            locked.put(svc);
                        }
                    }
                    Poll::Ready(())
                }
                Err(_e) => Poll::Ready(()),
            }
        }
    }
}

mod events {
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct Ignore;

    #[derive(Clone, Debug)]
    pub struct WithExecutor<E>(pub(super) E);

    pub trait Events<CF> {
        fn on_race_lost(&self, fut: CF);
    }

    impl<CF> Events<CF> for Ignore {
        fn on_race_lost(&self, _fut: CF) {}
    }

    impl<E, CF> Events<CF> for WithExecutor<E>
    where
        E: hyper::rt::Executor<CF>,
    {
        fn on_race_lost(&self, fut: CF) {
            self.0.execute(fut);
        }
    }
}

#[cfg(test)]
mod tests {
    use futures_util::future;
    use tower_service::Service;
    use tower_test::assert_request_eq;

    #[tokio::test]
    async fn test_makes_svc_when_empty() {
        let (mock, mut handle) = tower_test::mock::pair();
        let mut cache = super::builder().build(mock);
        handle.allow(1);

        std::future::poll_fn(|cx| cache.poll_ready(cx))
            .await
            .unwrap();

        let f = cache.call(1);

        future::join(f, async move {
            assert_request_eq!(handle, 1).send_response("one");
        })
        .await
        .0
        .expect("call");
    }

    #[tokio::test]
    async fn test_reuses_after_idle() {
        let (mock, mut handle) = tower_test::mock::pair();
        let mut cache = super::builder().build(mock);

        // only 1 connection should ever be made
        handle.allow(1);

        std::future::poll_fn(|cx| cache.poll_ready(cx))
            .await
            .unwrap();
        let f = cache.call(1);
        let cached = future::join(f, async {
            assert_request_eq!(handle, 1).send_response("one");
        })
        .await
        .0
        .expect("call");
        drop(cached);

        std::future::poll_fn(|cx| cache.poll_ready(cx))
            .await
            .unwrap();
        let f = cache.call(1);
        let cached = f.await.expect("call");
        drop(cached);
    }

    // A connector that upholds the tower `Service` contract check: `call` must
    // be preceded by a `poll_ready` returning `Ready`. Panics otherwise (the
    // same check `tower::limit::ConcurrencyLimit` makes). `calls` counts how
    // many times the connector established a connection.
    #[derive(Clone)]
    struct ContractConnector {
        ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Service<u32> for ContractConnector {
        type Response = &'static str;
        type Error = std::convert::Infallible;
        type Future = future::Ready<Result<&'static str, std::convert::Infallible>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.ready.store(true, std::sync::atomic::Ordering::SeqCst);
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: u32) -> Self::Future {
            assert!(
                self.ready.swap(false, std::sync::atomic::Ordering::SeqCst),
                "call() invoked without a preceding ready poll_ready()"
            );
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            future::ready(Ok("conn"))
        }
    }

    // When `poll_ready` observes an idle connection and returns `Ready`, but
    // that connection is removed before `call` (e.g. taken by another caller),
    // the resulting miss must still drive the connector's `poll_ready` before
    // `call`. Otherwise the cache violates the connector's `Service` contract.
    #[tokio::test]
    async fn drives_connector_poll_ready_when_idle_taken_between_poll_ready_and_call() {
        let inner = ContractConnector {
            ready: Default::default(),
            calls: Default::default(),
        };
        let mut cache = super::builder().build(inner.clone());

        // Establish one connection and return it to the idle set.
        std::future::poll_fn(|cx| cache.poll_ready(cx)).await.unwrap();
        drop(cache.call(1).await.unwrap());
        assert!(!cache.is_empty());

        // poll_ready observes the idle connection and returns Ready.
        std::future::poll_fn(|cx| cache.poll_ready(cx)).await.unwrap();

        // Remove the idle connection before call, so this call misses and
        // reaches the connector.
        cache.retain(|_| false);
        assert!(cache.is_empty());

        // The miss must drive the connector's poll_ready before call.
        let _conn = cache.call(1).await.unwrap();

        assert_eq!(
            inner.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "two connections established (initial + post-drain miss)"
        );
    }

    // A capacity-limited connector mirroring the pool's `ConnectionLimit`:
    // reserves a semaphore permit in `poll_ready`, consumes it in `call`, and
    // attaches it to the response so the permit is released only when the
    // response (the "connection") is dropped. A clone resets its reservation
    // (each connect reserves independently against the shared semaphore).
    // `calls` counts establishments.
    struct CapConnector {
        sem: tokio_util::sync::PollSemaphore,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
    }

    // A "connection" that holds its permit for its lifetime.
    struct CapConn {
        _permit: tokio::sync::OwnedSemaphorePermit,
    }

    impl CapConnector {
        fn new(
            sem: std::sync::Arc<tokio::sync::Semaphore>,
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        ) -> Self {
            Self {
                sem: tokio_util::sync::PollSemaphore::new(sem),
                calls,
                permit: None,
            }
        }
    }

    impl Clone for CapConnector {
        fn clone(&self) -> Self {
            Self {
                sem: self.sem.clone(),
                calls: self.calls.clone(),
                permit: None,
            }
        }
    }

    impl Service<u32> for CapConnector {
        type Response = CapConn;
        type Error = std::convert::Infallible;
        type Future = future::Ready<Result<CapConn, std::convert::Infallible>>;

        fn poll_ready(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            if self.permit.is_none() {
                match self.sem.poll_acquire(cx) {
                    std::task::Poll::Ready(p) => self.permit = p,
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                }
            }
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: u32) -> Self::Future {
            let permit = self
                .permit
                .take()
                .expect("poll_ready (permit reservation) must precede call");
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            future::ready(Ok(CapConn { _permit: permit }))
        }
    }

    // At a connection cap, a checkout that misses the idle set must NOT start a
    // new connect (it parks in the connector's `poll_ready` on the exhausted
    // semaphore), and when an in-use connection returns to the idle set the
    // parked checkout must wake and REUSE it rather than establish a new one.
    // This is the property the connection cap depends on: driving the
    // connector's `poll_ready` inside the race means the parked checkout is
    // simultaneously waiting on an idle return (the cache waiter) and a freed
    // permit (the connector), so an idle return wakes it without a new connect.
    #[tokio::test]
    async fn at_cap_a_miss_parks_without_connecting_and_wakes_on_idle_return() {
        use std::future::Future;
        use std::sync::atomic::Ordering::SeqCst;

        // cap = 1.
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut cache = super::builder().build(CapConnector::new(sem.clone(), calls.clone()));

        // First checkout: reserves the only permit and establishes.
        std::future::poll_fn(|cx| cache.poll_ready(cx)).await.unwrap();
        let first = cache.call(1).await.unwrap();
        assert_eq!(calls.load(SeqCst), 1, "first checkout establishes");
        assert_eq!(sem.available_permits(), 0, "first checkout holds the permit");

        // Second checkout while `first` is still out. The cache is always ready
        // (its poll_ready does not gate); the gating happens inside the returned
        // future. Poll it once with a noop waker: it must be Pending and must
        // NOT have started a second connect (parked in the connector's
        // poll_ready on the exhausted semaphore).
        let mut second = Box::pin(cache.call(1));
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(
            second.as_mut().poll(&mut cx).is_pending(),
            "at cap, the miss parks"
        );
        assert_eq!(
            calls.load(SeqCst),
            1,
            "at cap, the parked checkout starts NO new connect"
        );

        // Return the first connection to the idle set.
        drop(first);

        // The parked checkout wakes via the idle-return and reuses it — no new
        // establish, and the permit simply rides the reused connection.
        let _second = second.await.unwrap();
        assert_eq!(
            calls.load(SeqCst),
            1,
            "the parked checkout reused the returned idle connection, did not establish"
        );
        assert_eq!(
            sem.available_permits(),
            0,
            "the single permit rides the reused connection"
        );
    }

    // Waiters parked on a miss are woken in arrival order (FIFO). With three
    // checkouts parked behind an exhausted cap, returning connections one at a
    // time must hand them to the waiters oldest-first. Upstream wakes via
    // `Vec::pop` (LIFO), which serves newest-first and starves the oldest
    // waiter — the cause of the conn-cap P999 tail.
    #[tokio::test]
    async fn waiters_are_woken_in_fifo_arrival_order() {
        use std::future::Future;
        use std::sync::atomic::Ordering::SeqCst;

        // cap = 1: one connection exists; three more checkouts must park.
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut cache = super::builder().build(CapConnector::new(sem.clone(), calls.clone()));

        // Establish the single connection, then return it to idle.
        std::future::poll_fn(|cx| cache.poll_ready(cx)).await.unwrap();
        let conn = cache.call(1).await.unwrap();
        drop(conn);

        // Take the idle connection out so the next checkouts miss and park as
        // waiters (the permit is held by `held` for the duration).
        std::future::poll_fn(|cx| cache.poll_ready(cx)).await.unwrap();
        let held = cache.call(1).await.unwrap();
        assert_eq!(sem.available_permits(), 0, "the only permit is held");

        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);

        // Park three checkouts, in order A, B, C. Each misses (no idle, no
        // permit) and registers a waiter via push_back.
        let mut a = Box::pin(cache.call(1));
        assert!(a.as_mut().poll(&mut cx).is_pending());
        let mut b = Box::pin(cache.call(1));
        assert!(b.as_mut().poll(&mut cx).is_pending());
        let mut c = Box::pin(cache.call(1));
        assert!(c.as_mut().poll(&mut cx).is_pending());

        // Return the connection: it must wake the OLDEST waiter (A). Drop A's
        // connection back; it must wake B next; then C.
        drop(held);
        let conn_a = a.await.unwrap();
        assert!(b.as_mut().poll(&mut cx).is_pending(), "B not yet served");
        assert!(c.as_mut().poll(&mut cx).is_pending(), "C not yet served");

        drop(conn_a);
        let conn_b = b.await.unwrap();
        assert!(c.as_mut().poll(&mut cx).is_pending(), "C served last");

        drop(conn_b);
        let _conn_c = c.await.unwrap();

        // Only the original connection ever existed; all waiters reused it.
        assert_eq!(calls.load(SeqCst), 1, "no new connections established");
    }
}

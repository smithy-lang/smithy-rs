/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, PoisonError},
    time::{Instant, SystemTime},
};

use aws_smithy_async::time::SharedTimeSource;

use super::RequestMetrics;

/// A callback invoked once the request is finished.
pub(crate) type FinishCallback = Box<dyn FnOnce(Option<RequestMetrics>) + Send>;

/// Per-request metrics state, shared by every component handling the request.
///
/// `MetricsService` stores this in the request extensions. Components that need the state after
/// passing the request inward must clone it out of the extensions first. Clones share the same
/// state.
#[derive(Clone)]
pub(crate) struct MetricsState {
    inner: Arc<Inner>,
}

struct Inner {
    // Put the Option outside the Mutex, so we can see None without locking.
    metrics: Option<Mutex<InProgressMetrics>>,
    /// Registered by `RequestExt::on_finish`, in order of registration.
    callbacks: Mutex<Vec<FinishCallback>>,
}

impl MetricsState {
    /// The wall clock is read from `time_source`, only if `enabled`.
    pub(crate) fn new(enabled: bool, time_source: &SharedTimeSource) -> Self {
        Self {
            inner: Arc::new(Inner {
                metrics: enabled.then(|| Mutex::new(InProgressMetrics::new(time_source))),
                callbacks: Mutex::new(Vec::new()),
            }),
        }
    }

    pub(crate) fn push_callback(&self, callback: FinishCallback) {
        // Pushing cannot leave the list inconsistent, so a poisoned lock is safe to use.
        (self.inner.callbacks)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(callback);
    }

    /// Produces the final metrics and invokes the registered callbacks in order of registration.
    ///
    /// A panicking callback is logged and does not prevent the remaining callbacks from running.
    fn finish(&self) {
        let end_time = now();
        // Neither lock is held while taking the other, or while running callbacks.
        let callbacks = mem::take(&mut *(self.inner.callbacks).lock().unwrap_or_else(PoisonError::into_inner));
        let metrics = (self.inner.metrics)
            .as_ref()
            .map(|metrics| metrics.lock().unwrap_or_else(PoisonError::into_inner).finish(end_time));
        for callback in callbacks {
            let metrics = metrics.clone();
            if catch_unwind(AssertUnwindSafe(|| callback(metrics))).is_err() {
                tracing::error!("an `on_finish` callback panicked");
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn callback_count(&self) -> usize {
        self.inner.callbacks.lock().unwrap().len()
    }
}

/// The current time, from tokio's clock so that tests can control it.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// Standard metrics measured so far for a request that has not yet finished.
struct InProgressMetrics {
    start_time: Instant,
    start_system_time: SystemTime,
}

impl InProgressMetrics {
    fn new(time_source: &SharedTimeSource) -> Self {
        Self {
            start_time: now(),
            start_system_time: time_source.now(),
        }
    }

    fn finish(&self, end_time: Instant) -> RequestMetrics {
        RequestMetrics {
            start_time: self.start_time,
            end_time,
            start_system_time: self.start_system_time,
        }
    }
}

/// Finishes the request when [`finish`](Self::finish) is first called, or when dropped.
pub(crate) struct Finisher {
    state: Option<MetricsState>,
}

impl Finisher {
    pub(crate) fn new(state: MetricsState) -> Self {
        Self { state: Some(state) }
    }

    pub(crate) fn finish(&mut self) {
        if let Some(state) = self.state.take() {
            state.finish();
        }
    }
}

impl Drop for Finisher {
    fn drop(&mut self) {
        self.finish();
    }
}

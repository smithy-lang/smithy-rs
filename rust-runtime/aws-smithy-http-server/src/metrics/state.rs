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

use crate::{service::ServiceShape, shape_id::ShapeId};

use super::{Outcome, RequestMetrics};

/// A callback invoked once the request is finished.
pub(crate) type FinishCallback = Box<dyn FnOnce(Option<RequestMetrics>) + Send>;

/// Facts about the service that are the same for every request.
#[derive(Debug, Clone)]
pub(crate) struct ServiceIdentity {
    id: ShapeId,
    version: Option<&'static str>,
}

impl ServiceIdentity {
    pub(crate) fn of<Ser: ServiceShape>() -> Self {
        Self {
            id: Ser::ID,
            // Generated services report an empty string for models that do not specify one.
            version: Ser::VERSION.filter(|version| !version.is_empty()),
        }
    }
}

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
    pub(crate) fn new(enabled: bool, time_source: &SharedTimeSource, service: &ServiceIdentity) -> Self {
        Self {
            inner: Arc::new(Inner {
                metrics: enabled.then(|| Mutex::new(InProgressMetrics::new(time_source, service))),
                callbacks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Records how the request turned out, if metrics are enabled. Replaces any earlier outcome.
    pub(crate) fn record_outcome(&self, outcome: Outcome) {
        if let Some(metrics) = &self.inner.metrics {
            metrics.lock().unwrap_or_else(PoisonError::into_inner).outcome = Some(outcome);
        }
    }

    /// Records the operation the request was routed to, if metrics are enabled.
    pub(crate) fn record_operation(&self, operation: &ShapeId) {
        if let Some(metrics) = &self.inner.metrics {
            metrics.lock().unwrap_or_else(PoisonError::into_inner).operation = Some(operation.clone());
        }
    }

    fn is_enabled(&self) -> bool {
        self.inner.metrics.is_some()
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

/// Records the operation `request` was routed to, if standard metrics are being collected.
pub(crate) fn record_operation<B>(request: &http::Request<B>, operation: &ShapeId) {
    if let Some(state) = request.extensions().get::<MetricsState>() {
        state.record_operation(operation);
    }
}

/// Whether standard metrics are being collected for `request`.
pub(crate) fn collecting<B>(request: &http::Request<B>) -> bool {
    request
        .extensions()
        .get::<MetricsState>()
        .is_some_and(MetricsState::is_enabled)
}

/// The current time, from tokio's clock so that tests can control it.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// Standard metrics measured so far for a request that has not yet finished.
struct InProgressMetrics {
    start_time: Instant,
    start_system_time: SystemTime,
    service: ServiceIdentity,
    outcome: Option<Outcome>,
    operation: Option<ShapeId>,
}

impl InProgressMetrics {
    fn new(time_source: &SharedTimeSource, service: &ServiceIdentity) -> Self {
        Self {
            start_time: now(),
            start_system_time: time_source.now(),
            service: service.clone(),
            outcome: None,
            operation: None,
        }
    }

    fn finish(&self, end_time: Instant) -> RequestMetrics {
        RequestMetrics {
            start_time: self.start_time,
            end_time,
            start_system_time: self.start_system_time,
            service: self.service.id.clone(),
            service_version: self.service.version,
            outcome: self.outcome.clone(),
            operation: self.operation.clone(),
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

    /// Records how the request turned out, unless it has already finished.
    pub(crate) fn record_outcome(&self, outcome: Outcome) {
        if let Some(state) = &self.state {
            state.record_outcome(outcome);
        }
    }
}

impl Drop for Finisher {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::metrics::test_util::{TestService, UnversionedTestService};

    #[test]
    fn empty_service_version_is_none() {
        assert_eq!(ServiceIdentity::of::<TestService>().version, Some("2026-11-12"));
        assert_eq!(ServiceIdentity::of::<UnversionedTestService>().version, None);
    }
}

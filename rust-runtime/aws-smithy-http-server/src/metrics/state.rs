/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, PoisonError,
    },
    time::{Duration, Instant},
};

use aws_smithy_async::time::SharedTimeSource;

use crate::{service::ServiceShape, shape_id::ShapeId};

use super::{Outcome, RequestMetrics, TimeAnchor};

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

/// Invokes `callbacks` in order, each with a clone of `metrics`.
///
/// A panicking callback is logged and does not prevent the remaining callbacks from running.
fn run_callbacks(callbacks: Vec<FinishCallback>, metrics: Option<RequestMetrics>) {
    for callback in callbacks {
        let metrics = metrics.clone();
        if catch_unwind(AssertUnwindSafe(|| callback(metrics))).is_err() {
            tracing::error!("an `on_finish` callback panicked");
        }
    }
}

/// Per-request metrics state, shared by every component handling the request.
///
/// Stored in the request extensions, either by `MetricsService`, or by `RequestExt::on_finish` if
/// a callback is registered before the request reaches a `MetricsService`. A state created by
/// `on_finish` is uninitialized until a `MetricsService` takes it over. Components that need the
/// state after passing the request inward must clone it out of the extensions first. Clones share
/// the same state, as do copies of the request's extensions.
#[derive(Clone)]
pub(crate) struct MetricsState {
    inner: Arc<Inner>,
}

struct Inner {
    /// Set once, by the `MetricsService` that takes the request over: `None` if standard metrics
    /// are disabled. Put the Option outside the Mutex, so we can see None without locking.
    metrics: OnceLock<Option<Mutex<InProgressMetrics>>>,
    /// Registered by `RequestExt::on_finish`, in order of registration.
    callbacks: Mutex<Vec<FinishCallback>>,
}

impl Inner {
    /// The metrics being collected, if the state is initialized and metrics are enabled.
    fn metrics(&self) -> Option<&Mutex<InProgressMetrics>> {
        self.metrics.get().and_then(Option::as_ref)
    }
}

impl Drop for Inner {
    /// Invokes, with `None`, callbacks that the request never finished for: those registered on a
    /// state no `MetricsService` took over, or after the request finished.
    fn drop(&mut self) {
        let callbacks = mem::take(self.callbacks.get_mut().unwrap_or_else(PoisonError::into_inner));
        run_callbacks(callbacks, None);
    }
}

/// The metrics a `MetricsService` collects for a request it takes over: `None` if disabled.
type Initial = Option<Mutex<InProgressMetrics>>;

impl MetricsState {
    /// A provisional state that a `MetricsService` has not yet taken over. Until then, its only
    /// purpose is to store callbacks.
    pub(crate) fn uninitialized() -> Self {
        Self {
            inner: Arc::new(Inner {
                metrics: OnceLock::new(),
                callbacks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// An initialized state, as a `MetricsService` would create. See [`Self::take_over`].
    #[cfg(test)]
    pub(crate) fn new(
        enabled: bool,
        time_source: &SharedTimeSource,
        service: &ServiceIdentity,
        concurrent_requests_count: usize,
    ) -> Self {
        Self::initialized(Self::initial(enabled, time_source, service, concurrent_requests_count))
    }

    /// Takes over `request` for a `MetricsService` starting to handle it, and returns its state.
    ///
    /// If the request carries an uninitialized `MetricsState`, that state is initialized, keeping
    /// the callbacks already registered on it. Otherwise, a new state is initialized and stored in
    /// the request extensions, overwriting any `MetricsState` already stored. (The request might
    /// already carry an initialized `MetricsState` in the presence of middleware that does unusual
    /// things like cloning requests or passing a request object from one Smithy service to another
    /// Smithy service within the same process.)
    pub(crate) fn take_over<B>(
        request: &mut http::Request<B>,
        enabled: bool,
        time_source: &SharedTimeSource,
        service: &ServiceIdentity,
        concurrent_requests_count: usize,
    ) -> Self {
        let initial = Self::initial(enabled, time_source, service, concurrent_requests_count);
        let initial = match request.extensions().get::<MetricsState>() {
            Some(carried) => match carried.inner.metrics.set(initial) {
                Ok(()) => return carried.clone(),
                Err(initial) => initial,
            },
            None => initial,
        };
        let state = Self::initialized(initial);
        request.extensions_mut().insert(state.clone());
        state
    }

    fn initial(
        enabled: bool,
        time_source: &SharedTimeSource,
        service: &ServiceIdentity,
        concurrent_requests_count: usize,
    ) -> Initial {
        enabled.then(|| Mutex::new(InProgressMetrics::new(time_source, service, concurrent_requests_count)))
    }

    fn initialized(initial: Initial) -> Self {
        Self {
            inner: Arc::new(Inner {
                metrics: OnceLock::from(initial),
                callbacks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Records how the request turned out, if metrics are enabled. Replaces any earlier outcome.
    pub(crate) fn record_outcome(&self, outcome: Outcome) {
        if let Some(metrics) = self.inner.metrics() {
            metrics.lock().unwrap_or_else(PoisonError::into_inner).outcome = Some(outcome);
        }
    }

    /// Records the response's status code and outcome, if metrics are enabled.
    pub(crate) fn record_response(&self, status_code: http::StatusCode, outcome: Outcome) {
        if let Some(metrics) = self.inner.metrics() {
            let mut metrics = metrics.lock().unwrap_or_else(PoisonError::into_inner);
            metrics.status_code = Some(status_code);
            metrics.outcome = Some(outcome);
        }
    }

    /// Records that an execution of the operation's handler (and its model plugins) began. The
    /// execution ends when the returned guard is dropped.
    pub(crate) fn start_operation(&self) -> OperationGuard {
        if let Some(metrics) = self.inner.metrics() {
            metrics
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .operation_duration
                .start(now());
        }
        OperationGuard { state: self.clone() }
    }

    /// Records the request's [`ServerRequestId`](crate::request::request_id::ServerRequestId), if
    /// metrics are enabled.
    #[cfg(feature = "request-id")]
    pub(crate) fn record_request_id(&self, request_id: &crate::request::request_id::ServerRequestId) {
        if let Some(metrics) = self.inner.metrics() {
            metrics.lock().unwrap_or_else(PoisonError::into_inner).request_id = Some(request_id.clone());
        }
    }

    /// Records the operation the request was routed to, if metrics are enabled.
    pub(crate) fn record_operation(&self, operation: &ShapeId) {
        if let Some(metrics) = self.inner.metrics() {
            metrics.lock().unwrap_or_else(PoisonError::into_inner).operation = Some(operation.clone());
        }
    }

    fn is_enabled(&self) -> bool {
        self.inner.metrics().is_some()
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
        let metrics = (self.inner.metrics())
            .map(|metrics| metrics.lock().unwrap_or_else(PoisonError::into_inner).finish(end_time));
        run_callbacks(callbacks, metrics);
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

/// Records the [`ServerRequestId`](crate::request::request_id::ServerRequestId) `request` carries,
/// if it has one and standard metrics are being collected.
pub(crate) fn record_request_id<B>(request: &http::Request<B>) {
    #[cfg(feature = "request-id")]
    if let (Some(state), Some(request_id)) = (
        request.extensions().get::<MetricsState>(),
        request
            .extensions()
            .get::<crate::request::request_id::ServerRequestId>(),
    ) {
        state.record_request_id(request_id);
    }
    #[cfg(not(feature = "request-id"))]
    let _ = request;
}

/// The metrics state of `request`, if standard metrics are being collected for it.
pub(crate) fn collecting<B>(request: &http::Request<B>) -> Option<MetricsState> {
    request
        .extensions()
        .get::<MetricsState>()
        .filter(|state| state.is_enabled())
        .cloned()
}

/// The current time, from tokio's clock so that tests can control it.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// Standard metrics measured so far for a request that has not yet finished.
struct InProgressMetrics {
    start_time: TimeAnchor,
    service: ServiceIdentity,
    outcome: Option<Outcome>,
    operation: Option<ShapeId>,
    operation_duration: OperationDuration,
    concurrent_requests_count: usize,
    #[cfg(feature = "request-id")]
    request_id: Option<crate::request::request_id::ServerRequestId>,
    status_code: Option<http::StatusCode>,
}

impl InProgressMetrics {
    fn new(time_source: &SharedTimeSource, service: &ServiceIdentity, concurrent_requests_count: usize) -> Self {
        Self {
            concurrent_requests_count,
            #[cfg(feature = "request-id")]
            request_id: None,
            status_code: None,
            start_time: TimeAnchor {
                instant: now(),
                system_time: time_source.now(),
            },
            service: service.clone(),
            outcome: None,
            operation: None,
            operation_duration: OperationDuration::default(),
        }
    }

    fn finish(&self, end_time: Instant) -> RequestMetrics {
        RequestMetrics {
            concurrent_requests_count: self.concurrent_requests_count,
            #[cfg(feature = "request-id")]
            request_id: self.request_id.clone(),
            http: super::HttpRequestMetrics {
                status_code: self.status_code,
            },
            start_time: self.start_time,
            end_time,
            service: self.service.id.clone(),
            service_version: self.service.version,
            outcome: self.outcome.clone(),
            operation: self.operation.clone(),
            operation_duration: self.operation_duration.total(end_time),
        }
    }
}

/// Ends an execution started by [`MetricsState::start_operation`] when dropped: when the execution
/// returns, its future is dropped, or a panic unwinds through it.
pub(crate) struct OperationGuard {
    state: MetricsState,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Some(metrics) = self.state.inner.metrics() {
            metrics
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .operation_duration
                .finish(now());
        }
    }
}

/// Measures the total time during which at least one execution of the operation was in progress.
///
/// Overlapping executions count once. An execution still in progress when the request finishes
/// counts until then.
#[derive(Debug, Default)]
struct OperationDuration {
    /// Whether any execution has ever started.
    executed: bool,
    /// Executions that have started but not finished.
    in_progress: usize,
    /// When `in_progress` last rose from zero, if it is above zero.
    since: Option<Instant>,
    /// Time accumulated by intervals that have already ended.
    total: Duration,
}

impl OperationDuration {
    fn start(&mut self, now: Instant) {
        self.executed = true;
        if self.in_progress == 0 {
            self.since = Some(now);
        }
        self.in_progress += 1;
    }

    fn finish(&mut self, now: Instant) {
        self.in_progress = self.in_progress.saturating_sub(1);
        if self.in_progress == 0 {
            if let Some(since) = self.since.take() {
                self.total += now.saturating_duration_since(since);
            }
        }
    }

    /// The total, counting any interval still in progress up to `end`, or `None` if no execution
    /// ever started.
    fn total(&self, end: Instant) -> Option<Duration> {
        self.executed.then(|| match self.since {
            Some(since) => self.total + end.saturating_duration_since(since),
            None => self.total,
        })
    }
}

/// Counts the requests a service has started but not finished. Clones share the count.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConcurrentRequests(Arc<AtomicUsize>);

impl ConcurrentRequests {
    /// Counts a request as started. Returns the number of other requests in progress, and a guard
    /// that counts the request as finished when dropped.
    pub(crate) fn start(&self) -> (usize, ConcurrentRequestGuard) {
        let others = self.0.fetch_add(1, Ordering::Relaxed);
        (others, ConcurrentRequestGuard(self.0.clone()))
    }
}

/// Counts a request started by [`ConcurrentRequests::start`] as finished when dropped.
pub(crate) struct ConcurrentRequestGuard(Arc<AtomicUsize>);

impl Drop for ConcurrentRequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Finishes the request when [`finish`](Self::finish) is first called, or when dropped.
pub(crate) struct Finisher {
    state: Option<MetricsState>,
    concurrent_request: Option<ConcurrentRequestGuard>,
}

impl Finisher {
    /// `concurrent_request` stops counting the request as in progress when it finishes.
    pub(crate) fn new(state: MetricsState, concurrent_request: Option<ConcurrentRequestGuard>) -> Self {
        Self {
            state: Some(state),
            concurrent_request,
        }
    }

    pub(crate) fn finish(&mut self) {
        self.concurrent_request = None;
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

    /// Records the response's status code and outcome, unless the request has already finished.
    pub(crate) fn record_response(&self, status_code: http::StatusCode, outcome: Outcome) {
        if let Some(state) = &self.state {
            state.record_response(status_code, outcome);
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

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn operation_duration_is_none_if_the_operation_never_executes() {
        let base = now();
        assert_eq!(OperationDuration::default().total(at(base, 10)), None);
    }

    #[test]
    fn an_instantaneous_execution_has_a_zero_duration() {
        let base = now();
        let mut time = OperationDuration::default();
        time.start(at(base, 3));
        time.finish(at(base, 3));
        assert_eq!(time.total(at(base, 10)), Some(Duration::ZERO));
    }

    #[test]
    fn sequential_executions_are_summed() {
        let base = now();
        let mut time = OperationDuration::default();
        time.start(at(base, 0));
        time.finish(at(base, 3));
        time.start(at(base, 5));
        time.finish(at(base, 7));
        assert_eq!(time.total(at(base, 10)), Some(Duration::from_secs(5)));
    }

    #[test]
    fn overlapping_executions_count_once() {
        let base = now();
        let mut time = OperationDuration::default();
        time.start(at(base, 0));
        time.start(at(base, 2));
        time.finish(at(base, 4));
        time.finish(at(base, 6));
        assert_eq!(time.total(at(base, 10)), Some(Duration::from_secs(6)));
    }

    /// Finishes `state` and returns the operation duration its callbacks receive.
    fn finished_operation_duration(state: MetricsState) -> Option<Duration> {
        let seen = Arc::new(Mutex::new(None));
        state.push_callback(Box::new({
            let seen = seen.clone();
            move |metrics| *seen.lock().unwrap() = metrics
        }));
        state.finish();
        let metrics = seen.lock().unwrap().take();
        metrics.expect("metrics are enabled").operation_duration()
    }

    fn enabled_state() -> MetricsState {
        MetricsState::new(true, &Default::default(), &ServiceIdentity::of::<TestService>(), 0)
    }

    #[test]
    fn callbacks_registered_after_the_request_finishes_receive_none_when_the_state_is_dropped() {
        let state = enabled_state();
        state.finish();
        let received = Arc::new(Mutex::new(Vec::new()));
        state.push_callback(Box::new({
            let received = received.clone();
            move |metrics| received.lock().unwrap().push(metrics.is_some())
        }));
        assert!(
            received.lock().unwrap().is_empty(),
            "the state still holds the callback"
        );
        drop(state);
        assert_eq!(*received.lock().unwrap(), [false]);
    }

    #[test]
    fn taking_over_initializes_a_carried_uninitialized_state() {
        let carried = MetricsState::uninitialized();
        assert!(!carried.is_enabled());
        let mut request = http::Request::new(());
        request.extensions_mut().insert(carried.clone());
        let taken = MetricsState::take_over(
            &mut request,
            true,
            &Default::default(),
            &ServiceIdentity::of::<TestService>(),
            0,
        );
        assert!(Arc::ptr_eq(&taken.inner, &carried.inner));
        assert!(carried.is_enabled());
    }

    #[test]
    fn taking_over_replaces_an_initialized_state() {
        let carried = enabled_state();
        let mut request = http::Request::new(());
        request.extensions_mut().insert(carried.clone());
        let taken = MetricsState::take_over(
            &mut request,
            false,
            &Default::default(),
            &ServiceIdentity::of::<TestService>(),
            0,
        );
        assert!(!Arc::ptr_eq(&taken.inner, &carried.inner));
        let in_request = request.extensions().get::<MetricsState>().expect("a state");
        assert!(Arc::ptr_eq(&in_request.inner, &taken.inner));
        assert!(carried.is_enabled() && !taken.is_enabled());
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_guard_ends_the_execution() {
        let state = enabled_state();
        let execution = state.start_operation();
        tokio::time::advance(Duration::from_secs(2)).await;
        drop(execution);
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(finished_operation_duration(state), Some(Duration::from_secs(2)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_execution_ends_when_it_unwinds() {
        let state = enabled_state();
        let execution = state.start_operation();
        tokio::time::advance(Duration::from_secs(3)).await;
        let panicked = catch_unwind(AssertUnwindSafe(move || {
            let _execution = execution;
            panic!("the handler panicked");
        }));
        assert!(panicked.is_err());
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(finished_operation_duration(state), Some(Duration::from_secs(3)));
    }

    #[test]
    fn execution_in_progress_counts_until_the_request_finishes() {
        let base = now();
        let mut time = OperationDuration::default();
        time.start(at(base, 0));
        time.finish(at(base, 2));
        time.start(at(base, 4));
        // A concurrent execution that finishes while the earlier one is still in progress.
        time.start(at(base, 5));
        time.finish(at(base, 6));
        assert_eq!(time.total(at(base, 10)), Some(Duration::from_secs(8)));
    }

    #[test]
    fn empty_service_version_is_none() {
        assert_eq!(ServiceIdentity::of::<TestService>().version, Some("2026-11-12"));
        assert_eq!(ServiceIdentity::of::<UnversionedTestService>().version, None);
    }
}

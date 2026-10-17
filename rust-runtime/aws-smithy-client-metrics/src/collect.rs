/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Interceptor hooks that build a [`RequestRecord`] for each invocation.
//!
//! # Timeouts
//!
//! The orchestrator enforces timeouts by dropping futures, which skips hooks:
//!
//! - When an **attempt timeout** fires, `read_after_attempt` doesn't run for
//!   that attempt. The attempt is marked as timed out when the next attempt
//!   starts or when the call finishes.
//! - When an **operation timeout** fires, or the caller drops the `send()`
//!   future, `read_after_execution` doesn't run. The record is then completed
//!   and routed from [`InvocationState`]'s `Drop`, which runs when the call's
//!   `ConfigBag` is dropped.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_runtime::client::retries::classifiers::run_classifiers_on_ctx;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::http::telemetry::{
    CaptureHttpAttemptTelemetry, ConnectionUsage,
};
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
    BeforeTransmitInterceptorContextRef, FinalizerInterceptorContextRef,
};
use aws_smithy_runtime_api::client::interceptors::{dyn_dispatch_hint, Intercept};
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::client::retries::classifiers::{RetryAction, RetryReason};
use aws_smithy_runtime_api::client::retries::RequestAttempts;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use aws_smithy_types::retry::ErrorKind;
use aws_smithy_types::timeout::TimeoutConfig;
use metrique_util::{MetricsPool, MetricsPoolHandle};

use crate::emit::{emit, ResolvedDestination};
use crate::record::{AttemptRecord, CallEnd, ErrorType, RequestRecord};
use crate::{ClientMetrics, Destination, Inner, MissingPoolBehavior};

/// Per-invocation state, stored in the interceptor-state layer so it survives
/// retries. Completes and routes the record on drop if the normal path didn't.
struct InvocationState {
    /// Identifies the `ClientMetrics` instance that owns this invocation, so a
    /// second instance on the same call doesn't record it twice.
    owner: usize,
    config: Arc<Inner>,
    pending: Mutex<Pending>,
}

struct Pending {
    record: RequestRecord,
    /// Pool selected when the invocation started.
    captured_pool: Option<MetricsPoolHandle>,
    /// The current attempt's telemetry capture, kept from transmit time.
    attempt_capture: Option<CaptureHttpAttemptTelemetry>,
    /// Captured once runtime components are available, for use in `Drop`.
    time_source: Option<SharedTimeSource>,
    /// The operation timeout, when one is configured and can fire.
    operation_timeout: Option<Duration>,
    attempt_timeout: Option<Duration>,
    /// Number of the last attempt started by the orchestrator. This keeps
    /// duplicate registrations of the same `ClientMetrics` idempotent.
    last_started_attempt: Option<u32>,
    finished: bool,
}

impl std::fmt::Debug for InvocationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvocationState")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

impl Storable for InvocationState {
    type Storer = StoreReplace<Self>;
}

impl InvocationState {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Completes the record and applies its routing policy, at most once.
    fn finish(&self, pending: &mut Pending, now: SystemTime, end: CallEnd) {
        if pending.finished {
            return;
        }
        pending.finished = true;

        let attempt_timeout = pending.attempt_timeout;
        let capture = pending.attempt_capture.take();
        let record = &mut pending.record;
        record.end = end;
        record.duration = elapsed(record.started, now);
        if let Some(last) = record.attempts.last_mut().filter(|a| a.duration.is_none()) {
            // The last attempt never reached `read_after_attempt`. Keep what
            // the HTTP client recorded about its connection.
            apply_telemetry(last, capture.as_ref());
            match end {
                // The call completed, so the attempt timeout cut it short.
                CallEnd::Completed => mark_timed_out(last, attempt_timeout),
                CallEnd::OperationTimeout | CallEnd::Cancelled => {
                    let ran = elapsed(last.started, now);
                    match (ran, attempt_timeout) {
                        // An attempt can't outlive its timeout. If more time
                        // than that has passed, the attempt timeout already
                        // fired, and the call was dropped later, for example
                        // during the backoff before the next attempt.
                        (Some(ran), Some(limit)) if ran >= limit => {
                            mark_timed_out(last, attempt_timeout)
                        }
                        // Otherwise the attempt was still in flight.
                        _ => last.duration = ran,
                    }
                }
            }
        }

        let destination = match pending.captured_pool.clone() {
            Some(pool) => ResolvedDestination::Pool(pool),
            None => match &self.config.destination {
                Destination::CurrentPool => match self.config.missing_pool_behavior {
                    MissingPoolBehavior::EmitStandalone => {
                        warn_missing_pool(record, "writing a standalone entry");
                        ResolvedDestination::Standalone { fallback: true }
                    }
                    MissingPoolBehavior::Drop => {
                        warn_missing_pool(record, "dropping the metrics entry");
                        return;
                    }
                },
                Destination::Standalone => ResolvedDestination::Standalone { fallback: false },
                Destination::Pool(_) => {
                    unreachable!("the configured metrics pool should have been captured")
                }
            },
        };
        emit(
            record,
            self.config.qualifier,
            self.config.prefix,
            destination,
            self.config.standalone_sink.as_ref(),
            now,
        );
    }
}

fn warn_missing_pool(record: &RequestRecord, action: &'static str) {
    metrique::writer::rate_limit::rate_limited!(
        Duration::from_secs(60),
        tracing::warn!(
            service = record.service,
            operation = record.operation,
            action,
            "aws-smithy-client-metrics: expected a metrics pool, but none is in scope"
        )
    );
}

impl Drop for InvocationState {
    fn drop(&mut self) {
        let pending = self
            .pending
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pending.finished {
            return;
        }
        let now = pending.time_source.clone().unwrap_or_default().now();
        // The orchestrator doesn't say why the future was dropped. If the
        // operation timeout had elapsed, it is the likely cause; otherwise the
        // caller dropped the call.
        let end = match (
            pending.operation_timeout,
            elapsed(pending.record.started, now),
        ) {
            (Some(timeout), Some(took)) if took + TIMEOUT_TOLERANCE >= timeout => {
                CallEnd::OperationTimeout
            }
            _ => CallEnd::Cancelled,
        };
        let mut pending = std::mem::replace(pending, Pending::empty());
        self.finish(&mut pending, now, end);
    }
}

/// Allowance for clock granularity when deciding whether the operation timeout fired.
const TIMEOUT_TOLERANCE: Duration = Duration::from_millis(1);

impl Pending {
    fn empty() -> Self {
        Self {
            record: RequestRecord::new("Unknown", "Unknown"),
            captured_pool: None,
            attempt_capture: None,
            time_source: None,
            operation_timeout: None,
            attempt_timeout: None,
            last_started_attempt: None,
            finished: true,
        }
    }
}

impl ClientMetrics {
    fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    fn owned_state<'a>(&self, cfg: &'a ConfigBag) -> Option<&'a InvocationState> {
        let id = self.id();
        cfg.load::<InvocationState>()
            .filter(|state| state.owner == id)
    }
}

#[dyn_dispatch_hint]
impl Intercept for ClientMetrics {
    fn name(&self) -> &'static str {
        "ClientMetrics"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if cfg.load::<InvocationState>().is_some() {
            metrique::writer::rate_limit::rate_limited!(
                Duration::from_secs(60),
                tracing::warn!(
                    "aws-smithy-client-metrics: more than one `ClientMetrics` is installed on this \
                     call; only the first records it"
                )
            );
            return Ok(());
        }

        let record = cfg.load::<Metadata>().map_or_else(
            || RequestRecord::new("Unknown", "Unknown"),
            |m| RequestRecord::new(m.service(), m.name()),
        );
        let captured_pool = match &self.inner.destination {
            Destination::CurrentPool => MetricsPool::current(),
            Destination::Pool(pool) => Some(pool.clone()),
            Destination::Standalone => None,
        };
        cfg.interceptor_state().store_put(InvocationState {
            owner: self.id(),
            config: self.inner.clone(),
            pending: Mutex::new(Pending {
                record,
                captured_pool,
                attempt_capture: None,
                time_source: None,
                operation_timeout: None,
                attempt_timeout: None,
                last_started_attempt: None,
                finished: false,
            }),
        });
        Ok(())
    }

    fn read_before_serialization(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let Some(state) = self.owned_state(cfg) else {
            return Ok(());
        };
        let mut pending = state.lock();
        if let Some(metadata) = cfg.load::<Metadata>() {
            pending
                .record
                .set_identity(metadata.service(), metadata.name());
        }
        if pending.record.started.is_some() {
            return Ok(());
        }
        let time_source = runtime_components.time_source().unwrap_or_default();
        pending.record.started = Some(time_source.now());
        pending.time_source = Some(time_source);
        // Timeouts only fire when a sleep implementation is configured.
        if runtime_components.sleep_impl().is_some() {
            if let Some(timeouts) = cfg.load::<TimeoutConfig>() {
                pending.operation_timeout = timeouts.operation_timeout();
                pending.attempt_timeout = timeouts.operation_attempt_timeout();
            }
        }
        Ok(())
    }

    fn read_before_attempt(
        &self,
        _context: &BeforeTransmitInterceptorContextRef<'_>,
        runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let Some(state) = self.owned_state(cfg) else {
            return Ok(());
        };
        let Some(attempt_number) = cfg.load::<RequestAttempts>().map(RequestAttempts::attempts)
        else {
            metrique::writer::rate_limit::rate_limited!(
                Duration::from_secs(60),
                tracing::warn!(
                    "aws-smithy-client-metrics: no request attempt number is available; \
                     skipping attempt metrics"
                )
            );
            return Ok(());
        };

        let mut pending = state.lock();
        if pending
            .last_started_attempt
            .is_some_and(|last| attempt_number <= last)
        {
            return Ok(());
        }
        pending.last_started_attempt = Some(attempt_number);

        let attempt_timeout = pending.attempt_timeout;
        let capture = pending.attempt_capture.take();
        // A previous attempt that never reached `read_after_attempt` was
        // cut short by the attempt timeout.
        if let Some(previous) = pending
            .record
            .attempts
            .last_mut()
            .filter(|a| a.duration.is_none())
        {
            apply_telemetry(previous, capture.as_ref());
            mark_timed_out(previous, attempt_timeout);
        }
        pending.record.attempts.push(AttemptRecord {
            started: Some(now(runtime_components)),
            ..Default::default()
        });
        Ok(())
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Ask a compatible HTTP client to report connection facts for this attempt.
        // Request extensions are reset on retry, so this runs for every attempt.
        let Some(state) = self.owned_state(cfg) else {
            return Ok(());
        };
        let capture = match context.request().extension::<CaptureHttpAttemptTelemetry>() {
            Some(existing) => existing.clone(),
            None => {
                let capture = CaptureHttpAttemptTelemetry::new();
                context.request_mut().add_extension(capture.clone());
                capture
            }
        };
        state.lock().attempt_capture = Some(capture);
        Ok(())
    }

    fn read_after_attempt(
        &self,
        context: &FinalizerInterceptorContextRef<'_>,
        runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let Some(state) = self.owned_state(cfg) else {
            return Ok(());
        };
        let mut pending = state.lock();
        let pending = &mut *pending;
        let Some(attempt) = pending.record.attempts.last_mut() else {
            return Ok(());
        };
        if attempt.duration.is_some() {
            return Ok(());
        }

        let capture = pending.attempt_capture.take();
        attempt.duration =
            elapsed(attempt.started, now(runtime_components)).or(Some(Duration::ZERO));
        attempt.status = context.response().map(|r| r.status().as_u16());
        attempt.success = matches!(context.output_or_error(), Some(Ok(_)));
        // This reruns the configured classifiers. Replace it with the SDK's
        // recorded retry decision when that producer becomes available.
        attempt.throttled = matches!(
            run_classifiers_on_ctx(runtime_components.retry_classifiers(), context.inner()),
            RetryAction::RetryIndicated(RetryReason::RetryableError {
                kind: ErrorKind::ThrottlingError,
                ..
            })
        );
        if !attempt.success {
            attempt.error_type = Some(classify(context.inner(), attempt.throttled));
        }
        apply_telemetry(attempt, capture.as_ref());
        Ok(())
    }

    fn read_after_execution(
        &self,
        context: &FinalizerInterceptorContextRef<'_>,
        runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let Some(state) = self.owned_state(cfg) else {
            return Ok(());
        };
        let mut pending = state.lock();
        if pending.finished {
            return Ok(());
        }
        if let Some(metadata) = cfg.load::<Metadata>() {
            pending
                .record
                .set_identity(metadata.service(), metadata.name());
        }
        if pending.record.attempts.is_empty() && matches!(context.output_or_error(), Some(Err(_))) {
            pending.record.pre_attempt_error = Some(classify(context.inner(), false));
        }
        state.finish(&mut pending, now(runtime_components), CallEnd::Completed);
        Ok(())
    }
}

/// Classifies why an attempt (or a call that never sent one) failed.
///
/// An error response is checked first, so a modeled service error is a
/// `ServerError` whatever the deserializer reported.
fn classify(ctx: &InterceptorContext, throttled: bool) -> ErrorType {
    if throttled {
        return ErrorType::Throttling;
    }
    if ctx
        .response()
        .is_some_and(|r| r.status().is_client_error() || r.status().is_server_error())
    {
        return ErrorType::ServerError;
    }
    let Some(Err(err)) = ctx.output_or_error() else {
        return ErrorType::Other;
    };
    if err.is_timeout_error() {
        return ErrorType::ConfiguredTimeout;
    }
    if let Some(connector) = err.as_connector_error() {
        if connector.is_timeout() {
            return ErrorType::ConfiguredTimeout;
        }
        if connector.is_io() {
            return ErrorType::Io;
        }
    }
    ErrorType::Other
}

/// Copies what the HTTP client recorded about the attempt's connection.
fn apply_telemetry(attempt: &mut AttemptRecord, capture: Option<&CaptureHttpAttemptTelemetry>) {
    let Some(telemetry) = capture.map(CaptureHttpAttemptTelemetry::get) else {
        return;
    };
    attempt.connection_reused = telemetry
        .acquisition()
        .map(|a| a.usage() == ConnectionUsage::Reused);
    attempt.connection_acquire_duration = telemetry.acquisition().and_then(|a| a.duration());
    attempt.connector_call_duration = telemetry.connector_call_duration();
}

/// Marks an attempt that was cut short by the attempt timeout. Its duration is
/// the configured timeout, since that is when it was cancelled.
fn mark_timed_out(attempt: &mut AttemptRecord, attempt_timeout: Option<Duration>) {
    attempt.timed_out = true;
    attempt.duration = attempt_timeout;
}

/// The client's configured time source, or the system clock when none is set.
fn now(runtime_components: &RuntimeComponents) -> SystemTime {
    runtime_components.time_source().unwrap_or_default().now()
}

/// Elapsed time from `start` to `now`, or `None` if `start` is missing or the
/// clock went backwards.
fn elapsed(start: Option<SystemTime>, now: SystemTime) -> Option<Duration> {
    start.and_then(|start| now.duration_since(start).ok())
}

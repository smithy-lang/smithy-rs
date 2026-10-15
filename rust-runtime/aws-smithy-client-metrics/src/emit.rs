/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Converts a [`RequestRecord`] into metrique entries.

use std::time::{Duration, SystemTime};

use metrique::unit_of_work::metrics;
use metrique::writer::{AttachGlobalEntrySink, BoxEntrySink};
use metrique::{append_and_close, ServiceMetrics};
use metrique_util::MetricsPoolHandle;

use crate::record::{CallEnd, RequestRecord};

/// Fields contributed for one SDK invocation.
///
/// In a pool these are prefixed with `Sdk{Service}{Qualifier}{Operation}`, or
/// `{prefix}{Operation}` when a prefix is set.
/// The pool supplies the entry timestamp, so this type has none.
#[metrics(subfield_owned)]
pub(crate) struct InvocationMetrics {
    invocation_duration: Option<Duration>,
    attempts: u32,
    retry_count: u32,
    http_status_code: Option<String>,
    success: bool,
    client_error: bool,
    server_error: bool,
    throttles: u32,
    /// The cause of the failure. Present only on failed calls that weren't cancelled.
    error_type: Option<&'static str>,
    /// Time spent between attempts: retry backoff and rate-limiter waits.
    retry_delay: Option<Duration>,
    /// Attempts cut short by the attempt timeout.
    attempt_timeouts: u32,
    /// The operation timeout fired before the call completed.
    operation_timeout: bool,
    /// The caller dropped the call before it completed.
    cancelled: bool,
    connection_reused: Option<bool>,
    connection_acquire_duration: Option<Duration>,
    connector_call_duration: Option<Duration>,
}

impl InvocationMetrics {
    pub(crate) fn from_record(record: &RequestRecord) -> Self {
        let last = record.final_attempt();
        let status = last.and_then(|a| a.status);
        Self {
            invocation_duration: record.duration,
            attempts: record.attempts.len() as u32,
            retry_count: record.retry_count(),
            http_status_code: status.map(|s| s.to_string()),
            success: last.is_some_and(|a| a.success),
            client_error: status.is_some_and(|s| (400..500).contains(&s)),
            server_error: status.is_some_and(|s| (500..600).contains(&s)),
            throttles: record.throttles(),
            error_type: record.error_type().map(|e| e.as_str()),
            retry_delay: record.retry_delay(),
            attempt_timeouts: record.timed_out_attempts(),
            operation_timeout: record.end == CallEnd::OperationTimeout,
            cancelled: record.end == CallEnd::Cancelled,
            connection_reused: last.and_then(|a| a.connection_reused),
            connection_acquire_duration: last.and_then(|a| a.connection_acquire_duration),
            connector_call_duration: last.and_then(|a| a.connector_call_duration),
        }
    }
}

/// A standalone entry for one SDK invocation, used when no pool is in use.
#[metrics(rename_all = "PascalCase")]
struct StandaloneInvocationMetrics {
    #[metrics(timestamp)]
    timestamp: SystemTime,
    sdk_service: &'static str,
    qualifier: Option<&'static str>,
    operation: &'static str,
    /// `true` when a pool was expected but none was in scope. Omitted when no
    /// pool was expected (`standalone` mode), so it is only ever present as `1`.
    metrics_pool_fallback: Option<bool>,
    #[metrics(flatten)]
    invocation: InvocationMetrics,
}

/// Where a finished record goes after routing is resolved.
pub(crate) enum ResolvedDestination {
    Pool(MetricsPoolHandle),
    Standalone { fallback: bool },
}

pub(crate) fn emit(
    record: &RequestRecord,
    qualifier: Option<&'static str>,
    prefix: Option<&'static str>,
    destination: ResolvedDestination,
    standalone_sink: Option<&BoxEntrySink>,
    now: SystemTime,
) {
    let invocation = InvocationMetrics::from_record(record);
    match destination {
        ResolvedDestination::Pool(pool) => {
            let leading: Vec<&'static str> = match prefix {
                Some(prefix) => vec![prefix],
                None => ["sdk", record.service]
                    .into_iter()
                    .chain(qualifier)
                    .collect(),
            };
            let segments = leading.into_iter().chain(std::iter::once(record.operation));
            pool.with_prefix(segments).append(invocation);
        }
        ResolvedDestination::Standalone { fallback } => {
            let Some(sink) = standalone_sink.cloned().or_else(ServiceMetrics::try_sink) else {
                metrique::writer::rate_limit::rate_limited!(
                    Duration::from_secs(60),
                    tracing::warn!(
                        service = record.service,
                        operation = record.operation,
                        "aws-smithy-client-metrics: no standalone metrics sink configured \
                         (set `ClientMetricsBuilder::standalone_sink` or attach `ServiceMetrics`); \
                         dropping entry"
                    )
                );
                return;
            };
            let entry = StandaloneInvocationMetrics {
                timestamp: now,
                sdk_service: record.service,
                qualifier,
                operation: record.operation,
                metrics_pool_fallback: fallback.then_some(true),
                invocation,
            };
            drop(append_and_close(entry, sink));
        }
    }
}

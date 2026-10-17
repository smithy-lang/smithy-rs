/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

//! Per-request client metrics for smithy-rs clients, emitted through [metrique].
//!
//! Install [`ClientMetrics`] once on a client. Every SDK call (one `send()`,
//! including its retries) records one set of fields: duration, attempts, retries,
//! status, throttles and connection reuse. The routing configuration determines
//! where that record is emitted.
//!
//! # Install
//!
//! ```rust,ignore
//! use aws_smithy_client_metrics::ClientMetrics;
//!
//! let config = aws_sdk_s3::config::Builder::from(&sdk_config)
//!     .interceptor(ClientMetrics::new())
//!     .build();
//! let s3 = aws_sdk_s3::Client::from_conf(config);
//! ```
//!
//! [`ClientMetrics`] implements both [`Intercept`] and [`RuntimePlugin`]. AWS SDK
//! clients only accept interceptors from users, so use `.interceptor(..)` there.
//! Other smithy-rs clients can also use `.runtime_plugin(..)`; both behave the same.
//!
//! # Where the fields go
//!
//! By default, fields go into the [`MetricsPool`] that is in scope when `send()`
//! is called (see [`with_metrics_pool`]). The pool's parent entry then carries
//! the SDK fields contributed while handling that request:
//!
//! ```rust,ignore
//! use metrique::unit_of_work::metrics;
//! use metrique_util::{with_metrics_pool, MetricsPool};
//!
//! #[metrics(rename_all = "PascalCase")]
//! struct RequestMetrics {
//!     route: &'static str,
//!     #[metrics(flatten)]
//!     sdk: MetricsPool,
//! }
//!
//! let request = RequestMetrics { route: "/get", sdk: MetricsPool::new() }.append_on_drop(sink);
//! with_metrics_pool(request.sdk.handle(), async {
//!     s3.get_object().bucket("b").key("k").send().await
//! })
//! .await?;
//! // `request` emits `Route` plus `SdkS3GetObjectRetryCount`, `SdkS3GetObjectApiCallSuccessful`, ...
//! ```
//!
//! Pooled fields are prefixed with `Sdk`, the service, the optional
//! [qualifier](ClientMetricsBuilder::qualifier) and the operation, as in
//! `SdkS3AuditGetObjectRetryCount`. [`prefix`](ClientMetricsBuilder::prefix)
//! replaces everything before the operation. If the same operation runs more
//! than once for one parent entry, the last call wins. A default pool retains
//! 128 child entries and evicts the oldest when full; appends after the parent
//! closes are discarded. See [`MetricsPool`] for these inherited limits.
//!
//! [`Destination`] selects where each call's fields go. The default,
//! [`Destination::CurrentPool`], uses the pool in scope when `send()` starts.
//! [`MissingPoolBehavior`] independently controls what happens when that pool
//! is unavailable. By default, a rate-limited warning is logged and the call
//! writes a standalone entry with `MetricsPoolFallback = 1`.
//!
//! Standalone entries contain the same fields without a prefix, plus
//! `ServiceId`, `Qualifier` and `OperationName`. They go to the
//! [standalone sink](ClientMetricsBuilder::standalone_sink), or to the global
//! [`ServiceMetrics`] sink when none is set. If neither exists, the entry is
//! dropped with a warning.
//!
//! | Configuration | Behavior |
//! |---|---|
//! | [`ClientMetrics::new`] | Use the current pool; warn and emit standalone if it is missing |
//! | [`destination`](ClientMetricsBuilder::destination) and [`on_missing_pool`](ClientMetricsBuilder::on_missing_pool) | Configure destination and missing-pool behavior separately |
//! | [`require_current_pool`](ClientMetricsBuilder::require_current_pool) | Use the current pool; warn and drop the metrics entry if it is missing |
//! | [`metrics_pool`](ClientMetricsBuilder::metrics_pool) | Always use the given pool |
//! | [`standalone`](ClientMetricsBuilder::standalone) | Route every call to a standalone entry |
//!
//! Pool scope does not follow `tokio::spawn`. Wrap spawned futures with
//! [`metrique_util::propagate_current`] to keep their SDK calls in the request's entry.
//!
//! # Fields
//!
//! Names follow the [AWS SDK for Java 2.x metrics] where this crate records an
//! equivalent concept. Java attempt-level names (`HttpStatusCode`, `ErrorType`,
//! `ConcurrencyAcquireDuration` and `ServiceCallDuration`) describe the final
//! attempt here. `BackoffDelayDuration` is the call's approximate total between
//! attempts. Fields without a Java equivalent are extensions from this crate.
//!
//! | Field | Meaning |
//! |---|---|
//! | `ApiCallDuration` | Time from immediately before request serialization until execution completes, across all attempts. Excludes earlier interceptor work and reading a streaming response body. Omitted if timing never starts or the time source moves backwards. |
//! | `Attempts` / `RetryCount` | Number of attempts, and attempts minus one |
//! | `HttpStatusCode` | HTTP status of the final attempt, when a response was received |
//! | `ApiCallSuccessful` | The final attempt produced a successful output |
//! | `ClientError` / `ServerError` | The final status was 4xx / 5xx |
//! | `Throttles` | Attempts that the client's retry classifiers marked as throttling. This interceptor reruns the classifiers after each attempt. |
//! | `ErrorType` | Why a failed call failed: `Throttling`, `ServerError` (any non-throttling error response), `ConfiguredTimeout`, `IO` or `Other`. Omitted on success and on cancellation. |
//! | `BackoffDelayDuration` | Elapsed time between attempts. Approximates retry backoff and rate-limiter waits, and can include interceptor or orchestrator overhead. |
//! | `AttemptTimeouts` | Attempts cut short by the attempt timeout |
//! | `OperationTimeout` | The operation timeout fired before the call completed |
//! | `Cancelled` | The caller dropped the `send()` future before the call completed |
//! | `ConcurrencyAcquireDuration` | The final attempt's time to get a connection from the HTTP client (when reported) |
//! | `ConnectionReused` | The final attempt used a previously used connection (when the HTTP client reports it) |
//! | `ServiceCallDuration` | The final attempt's HTTP connector time, up to the response head (when reported) |
//!
//! # Timeouts and cancellation
//!
//! Timeouts and cancellation do not bypass the configured routing policy:
//!
//! - An attempt cut short by the attempt timeout counts in `AttemptTimeouts`,
//!   with the configured timeout as its duration. A retry after it is recorded
//!   as usual.
//! - When the operation timeout fires, or the caller drops the `send()` future,
//!   the orchestrator's final hooks don't run. The record is completed and
//!   routed when the call's state is dropped, with `OperationTimeout` or
//!   `Cancelled` set and `ApiCallSuccessful = 0`. A call is reported as an
//!   operation timeout when the configured operation timeout had elapsed;
//!   otherwise as cancelled.
//!
//! [metrique]: https://docs.rs/metrique
//! [`Intercept`]: aws_smithy_runtime_api::client::interceptors::Intercept
//! [`RuntimePlugin`]: aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin
//! [`MetricsPool`]: metrique_util::MetricsPool
//! [`with_metrics_pool`]: metrique_util::with_metrics_pool
//! [`ServiceMetrics`]: metrique::ServiceMetrics
//! [AWS SDK for Java 2.x metrics]: https://docs.aws.amazon.com/sdk-for-java/latest/developer-guide/metrics-list.html

mod collect;
mod emit;
mod record;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use aws_smithy_runtime_api::client::interceptors::SharedInterceptor;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use metrique::writer::{BoxEntry, BoxEntrySink, EntrySink};
use metrique_util::MetricsPoolHandle;

/// Records per-request metrics for every call made by a smithy-rs client.
///
/// See the [crate documentation](crate) for usage.
#[derive(Clone)]
pub struct ClientMetrics {
    inner: Arc<Inner>,
}

struct Inner {
    qualifier: Option<&'static str>,
    prefix: Option<&'static str>,
    destination: Destination,
    missing_pool_behavior: MissingPoolBehavior,
    standalone_sink: Option<BoxEntrySink>,
}

/// Selects where client metrics are written.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Destination {
    /// Use the [`MetricsPool`](metrique_util::MetricsPool) in scope when the
    /// call starts.
    ///
    /// [`MissingPoolBehavior`] controls what happens when no pool is in scope.
    CurrentPool,

    /// Always use this pool, regardless of which pool is in scope.
    Pool(MetricsPoolHandle),

    /// Route every call to a standalone entry.
    ///
    /// Emission still requires a configured standalone or global sink.
    Standalone,
}

/// Controls what happens when [`Destination::CurrentPool`] finds no pool.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum MissingPoolBehavior {
    /// Warn, then write a standalone entry with `MetricsPoolFallback = 1`.
    #[default]
    EmitStandalone,

    /// Warn and drop the metrics entry without affecting the SDK call.
    Drop,
}

impl fmt::Debug for ClientMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientMetrics")
            .field("qualifier", &self.inner.qualifier)
            .field("prefix", &self.inner.prefix)
            .field("destination", &self.inner.destination)
            .field("missing_pool_behavior", &self.inner.missing_pool_behavior)
            .field(
                "standalone_sink",
                &self.inner.standalone_sink.as_ref().map(|_| "BoxEntrySink"),
            )
            .finish()
    }
}

impl Default for ClientMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientMetrics {
    /// Creates `ClientMetrics` with the default behavior: write to the pool in
    /// scope, or warn and write a standalone entry when there is none.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Returns a builder for configuring `ClientMetrics`.
    pub fn builder() -> ClientMetricsBuilder {
        ClientMetricsBuilder::default()
    }
}

impl RuntimePlugin for ClientMetrics {
    fn runtime_components(
        &self,
        _current_components: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        Cow::Owned(
            RuntimeComponentsBuilder::new("ClientMetrics")
                .with_interceptor(SharedInterceptor::new(self.clone())),
        )
    }
}

/// Builder for [`ClientMetrics`].
#[derive(Default)]
pub struct ClientMetricsBuilder {
    qualifier: Option<&'static str>,
    prefix: Option<&'static str>,
    destination: Option<Destination>,
    missing_pool_behavior: MissingPoolBehavior,
    standalone_sink: Option<BoxEntrySink>,
}

impl fmt::Debug for ClientMetricsBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientMetricsBuilder")
            .field("qualifier", &self.qualifier)
            .field("prefix", &self.prefix)
            .field("destination", &self.destination)
            .field("missing_pool_behavior", &self.missing_pool_behavior)
            .field(
                "standalone_sink",
                &self.standalone_sink.as_ref().map(|_| "BoxEntrySink"),
            )
            .finish()
    }
}

impl ClientMetricsBuilder {
    /// Distinguishes this client from other clients for the same service.
    ///
    /// The qualifier appears between the service and operation in pooled keys.
    /// For example, `.qualifier("Audit")` changes `SdkS3GetObject...` to
    /// `SdkS3AuditGetObject...`. Standalone entries record it as
    /// `Qualifier = "Audit"`.
    ///
    /// Use this when one request may call the same operation through multiple
    /// clients for a service. If [`prefix`](Self::prefix) is set, the qualifier
    /// appears only in standalone entries.
    pub fn qualifier(mut self, qualifier: &'static str) -> Self {
        self.qualifier = Some(qualifier);
        self
    }

    /// Sets the part of pooled keys before the operation name.
    ///
    /// This replaces the default `Sdk{Service}{Qualifier}` prefix. For example,
    /// `.prefix("AuditLog")` produces `AuditLogGetObjectRetryCount`. Use a
    /// unique prefix to avoid collisions with other fields in the entry.
    ///
    /// This does not affect standalone entries.
    pub fn prefix(mut self, prefix: &'static str) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Selects where metrics are written.
    ///
    /// The default is [`Destination::CurrentPool`]. Use
    /// [`on_missing_pool`](Self::on_missing_pool) to configure what happens
    /// when the current pool is unavailable.
    pub fn destination(mut self, destination: Destination) -> Self {
        self.destination = Some(destination);
        self
    }

    /// Sets the behavior when [`Destination::CurrentPool`] finds no pool.
    ///
    /// A missing current pool always logs a rate-limited warning. This setting
    /// controls whether the record is emitted or dropped. The default is
    /// [`MissingPoolBehavior::EmitStandalone`]. It has no effect for the other
    /// destinations.
    pub fn on_missing_pool(mut self, behavior: MissingPoolBehavior) -> Self {
        self.missing_pool_behavior = behavior;
        self
    }

    /// Requires a current pool for metrics emission.
    ///
    /// When no pool is in scope, this logs a rate-limited warning and drops the
    /// metrics entry without affecting the SDK call. This is equivalent to
    /// selecting [`Destination::CurrentPool`] with
    /// [`MissingPoolBehavior::Drop`].
    pub fn require_current_pool(mut self) -> Self {
        self.destination = Some(Destination::CurrentPool);
        self.missing_pool_behavior = MissingPoolBehavior::Drop;
        self
    }

    /// Always writes to `pool`, ignoring the pool in scope.
    ///
    /// The pool usually belongs to one request, so this fits a client that lives
    /// as long as the pool.
    pub fn metrics_pool(mut self, pool: MetricsPoolHandle) -> Self {
        self.destination = Some(Destination::Pool(pool));
        self
    }

    /// Routes every call to a standalone entry and never looks for a pool.
    ///
    /// Use this for code without request entries, such as batch jobs or workers.
    /// Entries are dropped with a warning if no standalone or global sink is
    /// available.
    pub fn standalone(mut self) -> Self {
        self.destination = Some(Destination::Standalone);
        self
    }

    /// Sets the sink for standalone entries. Defaults to the global
    /// [`ServiceMetrics`](metrique::ServiceMetrics) sink.
    ///
    /// This sink is used by [`Destination::Standalone`] and by missing-pool
    /// behaviors that emit a standalone entry. Pooled entries do not use it.
    pub fn standalone_sink(
        mut self,
        sink: impl EntrySink<BoxEntry> + Send + Sync + 'static,
    ) -> Self {
        self.standalone_sink = Some(BoxEntrySink::new(sink));
        self
    }

    /// Builds the [`ClientMetrics`].
    pub fn build(self) -> ClientMetrics {
        ClientMetrics {
            inner: Arc::new(Inner {
                qualifier: self.qualifier,
                prefix: self.prefix,
                destination: self.destination.unwrap_or(Destination::CurrentPool),
                missing_pool_behavior: self.missing_pool_behavior,
                standalone_sink: self.standalone_sink,
            }),
        }
    }
}

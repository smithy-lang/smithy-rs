/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Default metrics types and configuration.
//!
//! This module contains the types [`MetricsLayer`](crate::MetricsLayer) uses to publish the
//! standard server metrics collected by `aws-smithy-http-server`.

use std::fmt::Debug;
use std::time::Duration;

use metrique::unit_of_work::metrics;
use metrique_util::MetricsPool;

use crate::types::aws_smithy_http_server::metrics::{Fault, RequestMetrics};

/// Container for default metrics and the request metrics pool.
///
/// This type is not intended for direct use. See [`MetricsLayer`](crate::MetricsLayer).
#[metrics]
#[derive(Default)]
pub struct SmithyMetrics {
    #[doc(hidden)]
    #[metrics(flatten)]
    pub(crate) default_metrics: DefaultMetrics,
    /// Pool that collects heterogeneous child metrics contributed by middleware,
    /// handlers, and libraries during the request. Flattened into this entry when
    /// the request finishes.
    #[doc(hidden)]
    #[metrics(flatten)]
    pub(crate) metrics_pool: MetricsPool,
}

impl Debug for SmithyMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmithyMetrics")
            .field("default_metrics", &self.default_metrics)
            .field("metrics_pool", &self.metrics_pool)
            .finish()
    }
}

/// A metrics entry that exposes a flattened [`SmithyMetrics`] block, letting the
/// metrics layer reach the owned [`MetricsPool`] for a request, and fill in the
/// default metrics once the request finishes.
///
/// The layer uses the pool to hand out a producer handle, which it wraps in a
/// [`SmithyMetricsPool`](crate::SmithyMetricsPool) and stores in the request's
/// extensions. Independently-owned middleware and libraries can then contribute
/// child metrics that flatten into this entry when the request finishes.
/// Operation handlers should instead record metrics on their own
/// `#[smithy_metrics(operation)]` field.
///
/// Implemented for [`SmithyMetrics`] and generated for structs annotated with
/// `#[smithy_metrics]`.
pub trait HasSmithyMetrics {
    /// Return this entry's [`SmithyMetrics`] block.
    fn smithy_metrics(&self) -> &SmithyMetrics;

    /// Return this entry's [`SmithyMetrics`] block, mutably.
    fn smithy_metrics_mut(&mut self) -> &mut SmithyMetrics;
}

impl HasSmithyMetrics for SmithyMetrics {
    fn smithy_metrics(&self) -> &SmithyMetrics {
        self
    }

    fn smithy_metrics_mut(&mut self) -> &mut SmithyMetrics {
        self
    }
}

/// Default metrics published for each request.
///
/// This type is not intended for direct use. See [`MetricsLayer`](crate::MetricsLayer).
#[metrics]
#[derive(Debug, Default)]
pub struct DefaultMetrics {
    pub(crate) service: Option<String>,
    pub(crate) service_version: Option<String>,
    pub(crate) operation: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) outstanding_requests: Option<usize>,
    pub(crate) http_status_code: Option<String>,
    /// Whether the request succeeded.
    pub(crate) success: Option<bool>,
    /// Whether the request failed because of the client.
    pub(crate) client_error: Option<bool>,
    /// Whether the request failed because of the server.
    pub(crate) server_error: Option<bool>,
    /// Time spent executing the operation's handler and model plugins.
    pub(crate) operation_time: Option<Duration>,
}

impl DefaultMetrics {
    /// The default metrics for a request, as enabled by `config`.
    pub(crate) fn new(metrics: &RequestMetrics, config: &DefaultMetricsConfig) -> Self {
        if config.disable_all {
            return Self::default();
        }
        // `None` if there was no response, `Some(None)` if the request succeeded, and
        // `Some(Some(fault))` if it failed.
        let fault = metrics.outcome().map(|outcome| {
            outcome
                .result
                .as_ref()
                .err()
                .map(|error| error.fault.clone())
        });
        Self {
            service: (!config.disable_service).then(|| metrics.service().name().to_owned()),
            service_version: metrics
                .service_version()
                .filter(|_| !config.disable_service_version)
                .map(str::to_owned),
            operation: metrics
                .operation()
                .filter(|_| !config.disable_operation)
                .map(|operation| operation.name().to_owned()),
            request_id: metrics
                .request_id()
                .filter(|_| !config.disable_request_id)
                .map(str::to_owned),
            // Count this request too.
            outstanding_requests: (!config.disable_outstanding_requests)
                .then(|| metrics.concurrent_requests_count() + 1),
            http_status_code: metrics
                .status_code()
                .filter(|_| !config.disable_http_status_code)
                .map(|status| status.as_str().to_owned()),
            success: fault
                .as_ref()
                .filter(|_| !config.disable_success)
                .map(Option::is_none),
            client_error: fault
                .as_ref()
                .filter(|_| !config.disable_client_error)
                .map(|fault| *fault == Some(Fault::Client)),
            server_error: fault
                .as_ref()
                .filter(|_| !config.disable_server_error)
                .map(|fault| *fault == Some(Fault::Server)),
            operation_time: (!config.disable_operation_time).then(|| metrics.operation_duration()),
        }
    }
}

/// Configuration for disabling specific default metrics.
///
/// This type is not intended for direct use. See [`MetricsLayer`](crate::MetricsLayer).
#[derive(Default, Debug, Clone)]
pub struct DefaultMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_service: bool,
    pub(crate) disable_service_version: bool,
    pub(crate) disable_operation: bool,
    pub(crate) disable_request_id: bool,
    pub(crate) disable_outstanding_requests: bool,
    pub(crate) disable_http_status_code: bool,
    pub(crate) disable_success: bool,
    pub(crate) disable_client_error: bool,
    pub(crate) disable_server_error: bool,
    pub(crate) disable_operation_time: bool,
}

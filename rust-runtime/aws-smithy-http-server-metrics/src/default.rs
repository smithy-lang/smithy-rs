/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Default metrics types and configuration.
//!
//! This module contains the types used by [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin)
//! to collect standard metrics automatically.

use std::fmt::Debug;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use metrique::unit_of_work::metrics;
use metrique::Slot;
use metrique::SlotGuard;

/// Container for keeping track of state across all requests for the service (all operations) for default metrics
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Debug, Default, Clone)]
pub struct DefaultMetricsServiceState {
    pub(crate) outstanding_requests_counter: Arc<AtomicUsize>,
}

/// Container for default request and response metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[metrics]
#[derive(Default)]
pub struct DefaultMetrics {
    #[metrics(flatten)]
    pub(crate) default_request_metrics: Option<Slot<DefaultRequestMetrics>>,
    #[metrics(flatten)]
    pub(crate) default_response_metrics: Option<Slot<DefaultResponseMetrics>>,
}
// Slot currently doesn't impl debug: https://github.com/awslabs/metrique/issues/190
impl Debug for DefaultMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultMetrics")
            .field("default_request_metrics", &())
            .field("default_response_metrics", &())
            .finish()
    }
}

/// Default request metrics collected
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[metrics]
#[derive(Debug, Default)]
pub struct DefaultRequestMetrics {
    pub(crate) service_name: Option<String>,
    pub(crate) service_version: Option<String>,
    pub(crate) operation_name: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) outstanding_requests: Option<usize>,
}

/// Default response metrics collected automatically.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[metrics]
#[derive(Default, Debug)]
pub struct DefaultResponseMetrics {
    pub(crate) http_status_code: Option<u16>,
    /// Client error indicator (1 if 4xx status code, 0 otherwise)
    pub(crate) error: Option<u64>,
    /// Server fault indicator (1 if 5xx status code, 0 otherwise)
    pub(crate) fault: Option<u64>,
    /// Wallclock time from pre-deserialization of the model input to post-serialization of the model output
    pub(crate) operation_time: Option<Duration>,
}

/// Configuration for disabling specific default request metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Default, Debug, Clone)]
pub struct DefaultRequestMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_request_id: bool,
    pub(crate) disable_operation_name: bool,
    pub(crate) disable_service_name: bool,
    pub(crate) disable_service_version: bool,
    pub(crate) disable_outstanding_requests: bool,
}

/// Configuration for disabling specific default response metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Default, Debug, Clone)]
pub struct DefaultResponseMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_http_status_code: bool,
    pub(crate) disable_error: bool,
    pub(crate) disable_fault: bool,
    pub(crate) disable_operation_time: bool,
}

/// Extension for accessing default request metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Clone)]
pub struct DefaultRequestMetricsExtension {
    pub(crate) metrics: Arc<Mutex<SlotGuard<DefaultRequestMetrics>>>,
    pub(crate) config: DefaultRequestMetricsConfig,
}

/// Extension for accessing default response metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Clone)]
pub struct DefaultResponseMetricsExtension {
    pub(crate) metrics: Arc<Mutex<SlotGuard<DefaultResponseMetrics>>>,
    pub(crate) config: DefaultResponseMetricsConfig,
}

/// Extension containing both request and response metrics.
///
/// This type is not intended for direct use. See [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin).
#[derive(Clone)]
pub struct DefaultMetricsExtension {
    pub(crate) request_ext: DefaultRequestMetricsExtension,
    pub(crate) response_ext: DefaultResponseMetricsExtension,
    pub(crate) service_state: DefaultMetricsServiceState,
}
impl DefaultMetricsExtension {
    #[doc(hidden)]
    pub fn __macro_new(
        request_metrics: Arc<Mutex<SlotGuard<DefaultRequestMetrics>>>,
        response_metrics: Arc<Mutex<SlotGuard<DefaultResponseMetrics>>>,
        request_metrics_config: DefaultRequestMetricsConfig,
        response_metrics_config: DefaultResponseMetricsConfig,
        service_state: DefaultMetricsServiceState,
    ) -> Self {
        Self {
            request_ext: DefaultRequestMetricsExtension {
                metrics: request_metrics,
                config: request_metrics_config,
            },
            response_ext: DefaultResponseMetricsExtension {
                metrics: response_metrics,
                config: response_metrics_config,
            },
            service_state,
        }
    }
}

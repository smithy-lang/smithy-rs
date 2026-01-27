/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use metrique::unit_of_work::metrics;
use metrique::Slot;
use metrique::SlotGuard;

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
            .field("request_metrics", &())
            .field("response_metrics", &())
            .finish()
    }
}

#[metrics]
#[derive(Debug, Default)]
pub struct DefaultRequestMetrics {
    pub(crate) service_name: Option<String>,
    pub(crate) service_version: Option<String>,
    pub(crate) operation_name: Option<String>,
    pub(crate) request_id: Option<String>,
}

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

#[derive(Default, Debug, Clone)]
pub struct DefaultRequestMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_request_id: bool,
    pub(crate) disable_operation_name: bool,
    pub(crate) disable_service_name: bool,
    pub(crate) disable_service_version: bool,
}

#[derive(Default, Debug, Clone)]
pub struct DefaultResponseMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_http_status_code: bool,
    pub(crate) disable_error: bool,
    pub(crate) disable_fault: bool,
    pub(crate) disable_operation_time: bool,
}

#[derive(Clone)]
pub struct DefaultRequestMetricsExtension {
    pub(crate) metrics: Arc<Mutex<SlotGuard<DefaultRequestMetrics>>>,
    pub(crate) config: DefaultRequestMetricsConfig,
}

#[derive(Clone)]
pub struct DefaultResponseMetricsExtension {
    pub(crate) metrics: Arc<Mutex<SlotGuard<DefaultResponseMetrics>>>,
    pub(crate) config: DefaultResponseMetricsConfig,
}

#[derive(Clone)]
pub struct DefaultMetricsExtension {
    pub(crate) request_ext: DefaultRequestMetricsExtension,
    pub(crate) response_ext: DefaultResponseMetricsExtension,
}
impl DefaultMetricsExtension {
    #[doc(hidden)]
    pub fn __macro_new(
        request_metrics: Arc<Mutex<SlotGuard<DefaultRequestMetrics>>>,
        response_metrics: Arc<Mutex<SlotGuard<DefaultResponseMetrics>>>,
        request_metrics_config: DefaultRequestMetricsConfig,
        response_metrics_config: DefaultResponseMetricsConfig,
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
        }
    }
}

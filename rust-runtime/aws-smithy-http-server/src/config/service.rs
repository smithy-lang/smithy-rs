/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{ServiceEventStreamConfig, ServiceRequestBodyConfig};
use std::time::Duration;

/// Typed service-wide configuration shared by every protocol.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ServiceConfig {
    /// Request-head deadline applied by the generated SDK's `serve` function.
    ///
    /// Defaults to one minute. `None` disables it. Covers the first request head
    /// for HTTP/1 and HTTP/2, and later HTTP/1 heads; later HTTP/2 streams are not covered.
    pub header_read_timeout: Option<Duration>,
    /// Global request body collection allowances, poll budget, and per-operation overrides.
    ///
    /// Generated serde reads the immutable SDK configuration.
    /// Operation overrides replace the whole allowance record.
    pub request_body: ServiceRequestBodyConfig,
    /// Event-stream read deadlines and per-operation overrides.
    pub event_stream: ServiceEventStreamConfig,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            header_read_timeout: Some(crate::settings::DEFAULT_HEADER_READ_TIMEOUT),
            request_body: ServiceRequestBodyConfig::default(),
            event_stream: ServiceEventStreamConfig::default(),
        }
    }
}

impl ServiceConfig {
    /// Sets the request-head deadline. `None` or zero disables it.
    pub fn with_header_read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.header_read_timeout = timeout.filter(|timeout| !timeout.is_zero());
        self
    }

    pub fn with_event_stream(mut self, event_stream: ServiceEventStreamConfig) -> Self {
        self.event_stream = event_stream;
        self
    }

    /// Sets the request body collection and streaming poll configuration.
    pub fn with_request_body(mut self, request_body: ServiceRequestBodyConfig) -> Self {
        self.request_body = request_body;
        self
    }
}

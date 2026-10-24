/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{ServiceEventStreamConfig, ServiceRequestBodyConfig};

/// Typed service-wide configuration shared by every protocol.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ServiceConfig {
    /// Global request body collection allowances, poll budget, and per-operation overrides.
    ///
    /// Body-first routing uses [`ServiceRequestBodyConfig::for_routing`], enforced
    /// by the routing service. Operation overrides replace the whole allowance record.
    pub request_body: ServiceRequestBodyConfig,
    /// Event-stream read deadlines and per-operation overrides.
    pub event_stream: ServiceEventStreamConfig,
}

impl ServiceConfig {
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

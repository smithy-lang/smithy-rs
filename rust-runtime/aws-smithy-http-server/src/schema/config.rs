/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::ServiceRequestBodyConfig;

/// Typed service-wide configuration shared by every protocol.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ServiceConfig {
    /// Global request body collection allowances and per-operation overrides.
    ///
    /// Body-first routing uses [`ServiceRequestBodyConfig::for_routing`], enforced
    /// by the routing service. Operation overrides replace the whole allowance record.
    pub request_body: ServiceRequestBodyConfig,
}

impl ServiceConfig {
    /// Sets the request body collection configuration.
    pub fn with_request_body(mut self, request_body: ServiceRequestBodyConfig) -> Self {
        self.request_body = request_body;
        self
    }
}

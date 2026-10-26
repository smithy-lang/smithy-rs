/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed service configuration shared by generated and schema-based implementations.

use crate::body::__private::DEFAULT_FRAME_BUDGET;
use aws_smithy_schema::ShapeId;
use std::{collections::HashMap, num::NonZeroUsize, time::Duration};

/// Typed service-wide configuration shared by every protocol.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ServiceConfig {
    /// Global request body collection allowances, poll budget, and per-operation overrides.
    ///
    /// Body-first routing uses [`ServiceRequestBodyConfig::for_routing`], enforced
    /// by the routing service. Operation overrides replace the whole allowance record.
    pub request_body: ServiceRequestBodyConfig,
}

impl ServiceConfig {
    /// Sets the request body collection and streaming poll configuration.
    pub fn with_request_body(mut self, request_body: ServiceRequestBodyConfig) -> Self {
        self.request_body = request_body;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RequestBodyCollectionConfig {
    pub max_bytes: Option<NonZeroUsize>,
    pub read_timeout: Option<Duration>,
    /// Consecutive ready HTTP body frame polls before yielding, for collection and streaming.
    /// Defaults to 32; `None` disables forced yielding.
    pub read_poll_budget: Option<NonZeroUsize>,
}

impl Default for RequestBodyCollectionConfig {
    fn default() -> Self {
        Self {
            max_bytes: None,
            read_timeout: None,
            read_poll_budget: Some(DEFAULT_FRAME_BUDGET),
        }
    }
}

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct ServiceRequestBodyConfig {
    pub global: RequestBodyCollectionConfig,
    pub per_operation: HashMap<String, RequestBodyCollectionConfig>,
}

impl RequestBodyCollectionConfig {
    /// Sets the byte limit; `None` allows any size.
    pub fn with_max_bytes(mut self, max_bytes: Option<NonZeroUsize>) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Sets the read timeout; `None` disables it.
    pub fn with_read_timeout(mut self, read_timeout: Option<Duration>) -> Self {
        self.read_timeout = read_timeout;
        self
    }

    /// Sets the frame poll budget for collection and streaming; `None` disables forced yielding.
    pub fn with_read_poll_budget(mut self, read_poll_budget: Option<NonZeroUsize>) -> Self {
        self.read_poll_budget = read_poll_budget;
        self
    }
}

impl ServiceRequestBodyConfig {
    /// Sets the default body-read allowances.
    pub fn with_global(mut self, global: RequestBodyCollectionConfig) -> Self {
        self.global = global;
        self
    }

    /// Sets operation overrides, keyed by operation shape ID.
    pub fn with_per_operation(mut self, per_operation: HashMap<String, RequestBodyCollectionConfig>) -> Self {
        self.per_operation = per_operation;
        self
    }

    /// Computes the provisional allowance before the operation is known.
    /// An absent operation override inherits `global`; an unlimited field dominates the maximum.
    pub fn for_routing(&self) -> RequestBodyCollectionConfig {
        let mut maximum = self.global;
        for config in self.per_operation.values() {
            maximum.max_bytes = match (maximum.max_bytes, config.max_bytes) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
            maximum.read_timeout = match (maximum.read_timeout, config.read_timeout) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
            maximum.read_poll_budget = match (maximum.read_poll_budget, config.read_poll_budget) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
        }
        maximum
    }

    pub fn for_operation(&self, operation: &ShapeId<'_>) -> RequestBodyCollectionConfig {
        self.per_operation
            .get(operation.as_str())
            .copied()
            .unwrap_or(self.global)
    }
}

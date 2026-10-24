/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::settings::{
    DEFAULT_EVENT_STREAM_IDLE_TIMEOUT, DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT,
    DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT, DEFAULT_REQUEST_BODY_MAX_BYTES,
    DEFAULT_REQUEST_BODY_READ_POLL_BUDGET,
};
use aws_smithy_schema::ShapeId;
use std::{collections::HashMap, num::NonZeroUsize, time::Duration};

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
            max_bytes: NonZeroUsize::new(DEFAULT_REQUEST_BODY_MAX_BYTES),
            read_timeout: None,
            read_poll_budget: NonZeroUsize::new(DEFAULT_REQUEST_BODY_READ_POLL_BUDGET),
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

/// Independent event-stream read deadlines. `None` disables a deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventStreamTimeoutConfig {
    pub initial_message: Option<Duration>,
    pub message_completion: Option<Duration>,
    pub idle: Option<Duration>,
}

impl Default for EventStreamTimeoutConfig {
    fn default() -> Self {
        Self {
            initial_message: DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT,
            message_completion: DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT,
            idle: DEFAULT_EVENT_STREAM_IDLE_TIMEOUT,
        }
    }
}

impl EventStreamTimeoutConfig {
    pub fn with_initial_message(mut self, timeout: Option<Duration>) -> Self {
        self.initial_message = timeout.filter(|timeout| !timeout.is_zero());
        self
    }

    pub fn with_message_completion(mut self, timeout: Option<Duration>) -> Self {
        self.message_completion = timeout.filter(|timeout| !timeout.is_zero());
        self
    }

    pub fn with_idle(mut self, timeout: Option<Duration>) -> Self {
        self.idle = timeout.filter(|timeout| !timeout.is_zero());
        self
    }
}

/// Service defaults and complete per-operation event-stream timeout records.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct ServiceEventStreamConfig {
    pub global: EventStreamTimeoutConfig,
    pub per_operation: HashMap<String, EventStreamTimeoutConfig>,
}

impl ServiceEventStreamConfig {
    pub fn with_global(mut self, global: EventStreamTimeoutConfig) -> Self {
        self.global = global;
        self
    }

    pub fn with_per_operation(mut self, per_operation: HashMap<String, EventStreamTimeoutConfig>) -> Self {
        self.per_operation = per_operation;
        self
    }

    pub fn for_operation(&self, operation: &ShapeId<'_>) -> EventStreamTimeoutConfig {
        self.per_operation
            .get(operation.as_str())
            .copied()
            .unwrap_or(self.global)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::settings::{
    DEFAULT_EVENT_STREAM_IDLE_TIMEOUT, DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT,
    DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT,
};
use aws_smithy_schema::ShapeId;
use std::{collections::HashMap, time::Duration};

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

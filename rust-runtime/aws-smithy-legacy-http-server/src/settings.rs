/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime settings and defaults for HTTP servers.
//!
//! Generated code selects the policy appropriate to the modeled operation and passes
//! explicit configuration through. Omitted settings resolve to these runtime values;
//! explicit zero values disable the corresponding protection.

use std::time::Duration;

/// Default request-head deadline, for legacy callers to apply to their Hyper builder.
pub const DEFAULT_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Default body-read deadline for operations without an HTTP payload member.
pub const DEFAULT_NON_PAYLOAD_REQUEST_BODY_READ_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Default body-read deadline for non-streaming HTTP payload operations.
pub const DEFAULT_PAYLOAD_REQUEST_BODY_READ_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Default number of consecutive ready event-stream body frames before yielding.
pub const DEFAULT_REQUEST_BODY_READ_POLL_BUDGET: usize = 32;

/// Default RPC event-stream initial-message deadline; disabled unless configured.
pub const DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT: Option<Duration> = None;

/// Default event-stream message-completion deadline; disabled unless configured.
pub const DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT: Option<Duration> = None;

/// Default event-stream idle deadline; disabled unless configured.
pub const DEFAULT_EVENT_STREAM_IDLE_TIMEOUT: Option<Duration> = None;

/// Default maximum buffered request-body bytes; zero means unlimited.
pub const DEFAULT_REQUEST_BODY_MAX_BYTES: usize = 0;

/// Effective body-read deadline for one operation. Zero disables it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestBodyReadTimeoutConfig {
    pub(crate) timeout: Duration,
    pub(crate) operation: &'static str,
}

impl RequestBodyReadTimeoutConfig {
    pub(crate) fn new(timeout: Duration, operation: &'static str) -> Option<Self> {
        (!timeout.is_zero()).then_some(Self { timeout, operation })
    }

    pub(crate) fn default_for_operation(payload: bool, operation: &'static str) -> Option<Self> {
        let timeout = if payload {
            DEFAULT_PAYLOAD_REQUEST_BODY_READ_TIMEOUT
        } else {
            DEFAULT_NON_PAYLOAD_REQUEST_BODY_READ_TIMEOUT
        };
        Self::new(timeout, operation)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime settings and defaults for HTTP servers.
//!
//! Generated code selects the policy appropriate to the modeled operation and passes
//! explicit configuration through. Omitted settings resolve to these runtime values;
//! explicit zero values disable the corresponding protection.

use std::{sync::Arc, time::Duration};

use hyper_util::{
    rt::{TokioExecutor, TokioTimer},
    server::conn::auto::Builder,
};

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

/// Default maximum number of concurrently accepted TCP connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 16_384;

/// Default timeout for receiving a complete request head.
pub const DEFAULT_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Default interval between HTTP/2 keep-alive pings.
pub const DEFAULT_HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);

/// Default timeout for receiving an HTTP/2 keep-alive ping acknowledgment.
pub const DEFAULT_HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Graceful shutdown waits indefinitely unless a timeout is configured.
pub const DEFAULT_SHUTDOWN_TIMEOUT: Option<Duration> = None;

/// Creates a connection builder with the default HTTP/2 keep-alive settings.
pub(crate) fn default_connection_builder() -> Builder<TokioExecutor> {
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http2()
        .timer(TokioTimer::new())
        .keep_alive_interval(Some(DEFAULT_HTTP2_KEEP_ALIVE_INTERVAL))
        .keep_alive_timeout(DEFAULT_HTTP2_KEEP_ALIVE_TIMEOUT);
    builder
}

/// Settings owned by the built-in serve loop.
pub(crate) struct ServeSettings {
    pub(crate) hyper_builder: Option<Arc<Builder<TokioExecutor>>>,
    pub(crate) max_connections: Option<usize>,
    pub(crate) header_read_timeout: Option<Duration>,
    pub(crate) shutdown_timeout: Option<Duration>,
}

impl Default for ServeSettings {
    fn default() -> Self {
        Self {
            hyper_builder: None,
            max_connections: Some(DEFAULT_MAX_CONNECTIONS),
            header_read_timeout: Some(DEFAULT_HEADER_READ_TIMEOUT),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

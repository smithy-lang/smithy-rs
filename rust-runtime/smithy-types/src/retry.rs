/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines types that describe when to retry given a response.

use std::time::Duration;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum ErrorKind {
    /// A connection-level error.
    ///
    /// A `TransientError` can represent conditions such as socket timeouts, socket connection errors, or TLS negotiation timeouts.
    /// 
    /// `TransientError` is not modeled by Smithy and is instead determined through client-specific heuristics and response status codes.
    ///
    /// Typically these should never be applied for non-idempotent request types
    /// since in this scenario, it's impossible to know whether the operation had
    /// a side effect on the server.
    ///
    /// TransientErrors are not currently modeled. They are determined based on specific provider
    /// level errors & response status code.
    TransientError,

    /// An error where the server explicitly told the client to back off, such as a 429 or 503 HTTP error.
    ThrottlingError,

    /// Server error that isn't explicitly throttling but is considered by the client
    /// to be something that should be retried.
    ServerError,

    /// Doesn't count against any budgets. This could be something like a 401 challenge in Http.
    ClientError,
}

pub trait ProvideErrorKind {
    /// Returns the `ErrorKind`.
    ///
    /// If the error kind cannot be determined (eg. the error is unmodeled at the error kind depends
    /// on an HTTP status code, return `None`.
    fn error_kind(&self) -> Option<ErrorKind>;

    /// Returns the `code` for this error if one exists
    fn code(&self) -> Option<&str>;
}

pub enum RetryKind {
    /// Retry the associated request due to a known `ErrorKind`.
    Error(ErrorKind),

    /// An Explicit retry (eg. from `x-amz-retry-after`).
    ///
    /// Note: The specified `Duration` is considered a suggestion and may be ignored. For example:
    /// - No retry tokens are available.
    /// - The retry duration exceeds that maximum backoff configured by the client.
    Explicit(Duration),

    /// The response associated with this variant should not be retried.
    NotRetryable,
}

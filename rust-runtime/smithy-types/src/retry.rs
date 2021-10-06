/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines types that describe when to retry given a response.

use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[non_exhaustive]
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
    /// Returns the `ErrorKind` when the error is modeled as retryable
    ///
    /// If the error kind cannot be determined (eg. the error is unmodeled at the error kind depends
    /// on an HTTP status code, return `None`.
    fn retryable_error_kind(&self) -> Option<ErrorKind>;

    /// Returns the `code` for this error if one exists
    fn code(&self) -> Option<&str>;
}

/// `RetryKind` describes how a request MAY be retried for a given response
///
/// A `RetryKind` describes how a response MAY be retried; it does not mandate retry behavior.
/// The actual retry behavior is at the sole discretion of the RetryStrategy in place.
/// A RetryStrategy may ignore the suggestion for a number of reasons including but not limited to:
/// - Number of retry attempts exceeded
/// - The required retry delay exceeds the maximum backoff configured by the client
/// - No retry tokens are available due to service health
#[non_exhaustive]
#[derive(Eq, PartialEq, Debug)]
pub enum RetryKind {
    /// Retry the associated request due to a known `ErrorKind`.
    Error(ErrorKind),

    /// An Explicit retry (eg. from `x-amz-retry-after`).
    ///
    /// Note: The specified `Duration` is considered a suggestion and may be replaced or ignored.
    Explicit(Duration),

    /// The response associated with this variant should not be retried.
    NotRetryable,
}

#[non_exhaustive]
#[derive(Eq, PartialEq, Debug, Clone, Copy)]
pub enum RetryMode {
    Standard,
    Adaptive,
}

#[derive(Debug)]
pub struct RetryModeErr(String);

impl Display for RetryModeErr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error parsing string '{}' as RetryMode, valid options are 'standard' or 'adaptive'",
            self.0
        )
    }
}

impl std::error::Error for RetryModeErr {}

impl FromStr for RetryMode {
    type Err = RetryModeErr;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let string = string.trim();
        // eq_ignore_ascii_case is OK here because the only strings we need to check for are ASCII
        if string.eq_ignore_ascii_case("standard") {
            Ok(RetryMode::Standard)
        } else if string.eq_ignore_ascii_case("adaptive") {
            Ok(RetryMode::Adaptive)
        } else {
            Err(RetryModeErr(string.to_owned()))
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RetryConfig {
    mode: RetryMode,
    max_attempts: u32,
}

impl RetryConfig {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_retry_mode(mut self, retry_mode: RetryMode) -> Self {
        self.mode = retry_mode;
        self
    }

    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub fn mode(&self) -> RetryMode {
        self.mode
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            mode: RetryMode::Standard,
            max_attempts: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::retry::RetryMode;
    use std::str::FromStr;

    #[test]
    fn retry_mode_from_str_parses_valid_strings_regardless_of_casing() {
        assert_eq!(
            RetryMode::from_str("standard").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("STANDARD").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("StAnDaRd").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("adaptive").ok(),
            Some(RetryMode::Adaptive)
        );
        assert_eq!(
            RetryMode::from_str("ADAPTIVE").ok(),
            Some(RetryMode::Adaptive)
        );
        assert_eq!(
            RetryMode::from_str("aDaPtIvE").ok(),
            Some(RetryMode::Adaptive)
        );
    }

    #[test]
    fn retry_mode_from_str_ignores_whitespace_before_and_after() {
        assert_eq!(
            RetryMode::from_str("  standard ").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("   STANDARD  ").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("  StAnDaRd   ").ok(),
            Some(RetryMode::Standard)
        );
        assert_eq!(
            RetryMode::from_str("  adaptive  ").ok(),
            Some(RetryMode::Adaptive)
        );
        assert_eq!(
            RetryMode::from_str("   ADAPTIVE ").ok(),
            Some(RetryMode::Adaptive)
        );
        assert_eq!(
            RetryMode::from_str("  aDaPtIvE    ").ok(),
            Some(RetryMode::Adaptive)
        );
    }

    #[test]
    fn retry_mode_from_str_wont_parse_invalid_strings() {
        assert_eq!(RetryMode::from_str("std").ok(), None);
        assert_eq!(RetryMode::from_str("aws").ok(), None);
        assert_eq!(RetryMode::from_str("s t a n d a r d").ok(), None);
        assert_eq!(RetryMode::from_str("a d a p t i v e").ok(), None);
    }
}

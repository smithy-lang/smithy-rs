/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The per-invocation record.
//!
//! This module has no metrique types so collection remains separate from
//! emission. It can later be replaced by or adapted to backend-neutral capture
//! types without coupling SDK producers to metrique.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Facts about one SDK invocation (one `send()`, including retries).
#[derive(Clone, Debug)]
pub(crate) struct RequestRecord {
    pub(crate) service: &'static str,
    pub(crate) operation: &'static str,
    /// Set from the client's time source immediately before serialization.
    pub(crate) started: Option<SystemTime>,
    pub(crate) duration: Option<Duration>,
    pub(crate) attempts: Vec<AttemptRecord>,
    pub(crate) end: CallEnd,
    /// Error type for a call that failed before any attempt was made.
    pub(crate) pre_attempt_error: Option<ErrorType>,
}

/// The cause of a failed call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorType {
    /// The service responded with a throttling error.
    Throttling,
    /// The service responded with an error other than throttling (4xx or 5xx).
    ServerError,
    /// A configured client timeout fired: operation, attempt, connect or read.
    ConfiguredTimeout,
    /// An I/O error, such as a refused or reset connection.
    Io,
    /// Any other error.
    Other,
}

impl ErrorType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ErrorType::Throttling => "Throttling",
            ErrorType::ServerError => "ServerError",
            ErrorType::ConfiguredTimeout => "ConfiguredTimeout",
            ErrorType::Io => "IO",
            ErrorType::Other => "Other",
        }
    }
}

/// How a call ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CallEnd {
    /// The orchestrator ran its final hooks, with success or an error.
    #[default]
    Completed,
    /// The configured operation timeout fired, so the final hooks never ran.
    OperationTimeout,
    /// The caller dropped the call before it finished.
    Cancelled,
}

/// Facts about one attempt.
#[derive(Clone, Debug, Default)]
pub(crate) struct AttemptRecord {
    pub(crate) started: Option<SystemTime>,
    pub(crate) duration: Option<Duration>,
    pub(crate) status: Option<u16>,
    pub(crate) success: bool,
    pub(crate) throttled: bool,
    /// The attempt timeout cut this attempt short.
    pub(crate) timed_out: bool,
    /// Why the attempt failed; `None` when it succeeded or wasn't classified.
    pub(crate) error_type: Option<ErrorType>,
    pub(crate) connection_reused: Option<bool>,
    pub(crate) connection_acquire_duration: Option<Duration>,
    pub(crate) connector_call_duration: Option<Duration>,
}

impl RequestRecord {
    pub(crate) fn new(service: &str, operation: &str) -> Self {
        Self {
            service: intern(service),
            operation: intern(operation),
            started: None,
            duration: None,
            attempts: Vec::new(),
            end: CallEnd::Completed,
            pre_attempt_error: None,
        }
    }

    pub(crate) fn set_identity(&mut self, service: &str, operation: &str) {
        self.service = intern(service);
        self.operation = intern(operation);
    }

    pub(crate) fn final_attempt(&self) -> Option<&AttemptRecord> {
        self.attempts.last()
    }

    pub(crate) fn retry_count(&self) -> u32 {
        self.attempts.len().saturating_sub(1) as u32
    }

    pub(crate) fn throttles(&self) -> u32 {
        self.attempts.iter().filter(|a| a.throttled).count() as u32
    }

    pub(crate) fn timed_out_attempts(&self) -> u32 {
        self.attempts.iter().filter(|a| a.timed_out).count() as u32
    }

    /// Elapsed time between one attempt ending and the next starting. This
    /// approximates retry backoff and rate-limiter waits, and can also include
    /// interceptor or orchestrator overhead. `None` when no attempt was made.
    pub(crate) fn retry_delay(&self) -> Option<Duration> {
        if self.attempts.is_empty() {
            return None;
        }
        let gaps = self.attempts.windows(2).filter_map(|pair| {
            let ended = pair[0].started? + pair[0].duration?;
            pair[1].started?.duration_since(ended).ok()
        });
        Some(gaps.sum())
    }

    /// The cause of the failure, or `None` if the call succeeded or the caller
    /// cancelled it.
    pub(crate) fn error_type(&self) -> Option<ErrorType> {
        match self.end {
            CallEnd::Cancelled => return None,
            CallEnd::OperationTimeout => return Some(ErrorType::ConfiguredTimeout),
            CallEnd::Completed => {}
        }
        match self.final_attempt() {
            Some(last) if last.success => None,
            Some(last) if last.timed_out => Some(ErrorType::ConfiguredTimeout),
            Some(last) => Some(last.error_type.unwrap_or(ErrorType::Other)),
            None => Some(self.pre_attempt_error.unwrap_or(ErrorType::Other)),
        }
    }
}

/// Normalizes a service or operation name to ASCII alphanumerics and returns
/// a `'static` copy.
///
/// metrique pool prefixes must be `&'static str`. The set of names is bounded
/// by the services and operations a process calls, so each distinct name is
/// leaked once.
///
/// Smithy `sdkId`s contain spaces and punctuation (for example
/// `"CloudWatch Logs"`), which would produce awkward metric names.
pub(crate) fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let normalized: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let normalized = if normalized.is_empty() {
        "Unknown".to_owned()
    } else {
        normalized
    };
    let mut names = NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(name) = names.get(&normalized) {
        return name;
    }
    let leaked: &'static str = Box::leak(normalized.clone().into_boxed_str());
    names.insert(normalized, leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_normalizes_and_dedupes() {
        assert_eq!(intern("CloudWatch Logs"), "CloudWatchLogs");
        assert_eq!(intern("S3"), "S3");
        assert_eq!(intern(""), "Unknown");
        assert!(std::ptr::eq(intern("DynamoDB"), intern("DynamoDB")));
    }

    #[test]
    fn counts() {
        let mut record = RequestRecord::new("S3", "GetObject");
        assert_eq!(record.retry_count(), 0);
        record.attempts.push(AttemptRecord {
            throttled: true,
            ..Default::default()
        });
        record.attempts.push(AttemptRecord::default());
        assert_eq!(record.retry_count(), 1);
        assert_eq!(record.throttles(), 1);
    }

    #[test]
    fn retry_delay_sums_gaps_between_attempts() {
        let t0 = SystemTime::UNIX_EPOCH;
        let attempt = |start_ms: u64, dur_ms: u64| AttemptRecord {
            started: Some(t0 + Duration::from_millis(start_ms)),
            duration: Some(Duration::from_millis(dur_ms)),
            ..Default::default()
        };
        let mut record = RequestRecord::new("S3", "GetObject");
        assert_eq!(record.retry_delay(), None);
        record.attempts.push(attempt(0, 10));
        assert_eq!(record.retry_delay(), Some(Duration::ZERO));
        record.attempts.push(attempt(30, 10)); // 20ms gap
        record.attempts.push(attempt(45, 10)); // 5ms gap
        assert_eq!(record.retry_delay(), Some(Duration::from_millis(25)));
    }

    #[test]
    fn error_type_by_outcome() {
        let mut record = RequestRecord::new("S3", "GetObject");
        assert_eq!(record.error_type(), Some(ErrorType::Other));
        record.pre_attempt_error = Some(ErrorType::Io);
        assert_eq!(record.error_type(), Some(ErrorType::Io));

        record.attempts.push(AttemptRecord {
            success: true,
            ..Default::default()
        });
        assert_eq!(record.error_type(), None);

        record.attempts.push(AttemptRecord {
            timed_out: true,
            ..Default::default()
        });
        assert_eq!(record.error_type(), Some(ErrorType::ConfiguredTimeout));

        record.end = CallEnd::Cancelled;
        assert_eq!(record.error_type(), None);
        record.end = CallEnd::OperationTimeout;
        assert_eq!(record.error_type(), Some(ErrorType::ConfiguredTimeout));
    }
}

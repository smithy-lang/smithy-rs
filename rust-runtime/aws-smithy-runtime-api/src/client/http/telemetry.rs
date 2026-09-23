/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provider-neutral observations from one HTTP request attempt.
//!
//! A runtime installs [`CaptureHttpAttemptTelemetry`] on a request before
//! transmission. A compatible HTTP client records the facts it owns without
//! depending on metric instruments, exporters, or tracing providers.

use crate::client::connection::ConnectionMetadata;
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Whether the selected connection had accepted an earlier request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConnectionUsage {
    /// The connection had not accepted an earlier request.
    Fresh,
    /// The connection had accepted at least one earlier request.
    Reused,
}

/// Time and reuse state for the connection selected by one request attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ConnectionAcquisitionTelemetry {
    duration: Duration,
    usage: ConnectionUsage,
}

impl ConnectionAcquisitionTelemetry {
    /// Creates a completed connection-acquisition observation.
    pub fn new(duration: Duration, usage: ConnectionUsage) -> Self {
        Self { duration, usage }
    }

    /// Returns elapsed time from acquisition start until Hyper accepted the request.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Returns whether the selected connection had accepted an earlier request.
    pub fn usage(&self) -> ConnectionUsage {
        self.usage
    }
}

/// Facts recorded by a compatible HTTP client for one request attempt.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct HttpAttemptTelemetry {
    acquisition: Option<ConnectionAcquisitionTelemetry>,
    connection: Option<ConnectionMetadata>,
    dispatch_duration: Option<Duration>,
}

impl HttpAttemptTelemetry {
    /// Returns completed connection-acquisition telemetry, when supplied.
    pub fn acquisition(&self) -> Option<&ConnectionAcquisitionTelemetry> {
        self.acquisition.as_ref()
    }

    /// Returns metadata for the connection that accepted the request, when supplied.
    pub fn connection(&self) -> Option<&ConnectionMetadata> {
        self.connection.as_ref()
    }

    /// Returns the complete HTTP connector call duration, when supplied.
    ///
    /// This ends when the connector returns a response head or terminal error.
    /// It does not measure response-body transfer or time to first response byte.
    pub fn dispatch_duration(&self) -> Option<Duration> {
        self.dispatch_duration
    }
}

/// Shared request extension used to capture HTTP-attempt telemetry.
///
/// Recording is first-write-wins. This prevents retries inside an HTTP client
/// from combining acquisition timing from one selection with metadata from a
/// later selection.
#[derive(Clone, Default)]
pub struct CaptureHttpAttemptTelemetry {
    state: Arc<Mutex<HttpAttemptTelemetry>>,
}

impl CaptureHttpAttemptTelemetry {
    /// Creates an empty attempt capture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the observations recorded so far.
    pub fn get(&self) -> HttpAttemptTelemetry {
        self.lock().clone()
    }

    /// Records the complete HTTP connector call duration.
    ///
    /// Returns `true` when this call recorded the value.
    pub fn record_dispatch_duration(&self, duration: Duration) -> bool {
        let mut state = self.lock();
        if state.dispatch_duration.is_some() {
            return false;
        }
        state.dispatch_duration = Some(duration);
        true
    }

    /// Records the connection selection that Hyper accepted.
    ///
    /// Acquisition and connection metadata are committed together. Returns
    /// `true` when this call recorded the selection.
    pub fn record_connection_selection(
        &self,
        acquisition: ConnectionAcquisitionTelemetry,
        connection: ConnectionMetadata,
    ) -> bool {
        let mut state = self.lock();
        if state.acquisition.is_some() || state.connection.is_some() {
            return false;
        }
        state.acquisition = Some(acquisition);
        state.connection = Some(connection);
        true
    }

    fn lock(&self) -> MutexGuard<'_, HttpAttemptTelemetry> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl fmt::Debug for CaptureHttpAttemptTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureHttpAttemptTelemetry")
            .finish_non_exhaustive()
    }
}

impl Storable for CaptureHttpAttemptTelemetry {
    type Storer = StoreReplace<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> ConnectionMetadata {
        ConnectionMetadata::builder()
            .proxied(false)
            .poison_fn(|| {})
            .build()
    }

    #[test]
    fn records_dispatch_and_selection_independently() {
        let capture = CaptureHttpAttemptTelemetry::new();
        let acquisition =
            ConnectionAcquisitionTelemetry::new(Duration::from_millis(3), ConnectionUsage::Fresh);

        assert!(capture.record_connection_selection(acquisition, connection()));
        assert!(capture.record_dispatch_duration(Duration::from_millis(7)));

        let telemetry = capture.get();
        assert_eq!(telemetry.acquisition(), Some(&acquisition));
        assert_eq!(
            telemetry.dispatch_duration(),
            Some(Duration::from_millis(7))
        );
        assert!(telemetry.connection().is_some());
    }

    #[test]
    fn first_recorded_value_wins() {
        let capture = CaptureHttpAttemptTelemetry::new();
        let first =
            ConnectionAcquisitionTelemetry::new(Duration::from_millis(3), ConnectionUsage::Fresh);
        let second =
            ConnectionAcquisitionTelemetry::new(Duration::from_millis(9), ConnectionUsage::Reused);
        assert!(capture.record_connection_selection(first, connection()));
        assert!(!capture.record_connection_selection(second, connection()));
        assert!(capture.record_dispatch_duration(Duration::from_millis(4)));
        assert!(!capture.record_dispatch_duration(Duration::from_millis(8)));

        let telemetry = capture.get();
        assert_eq!(telemetry.acquisition(), Some(&first));
        assert_eq!(
            telemetry.dispatch_duration(),
            Some(Duration::from_millis(4))
        );
    }
}

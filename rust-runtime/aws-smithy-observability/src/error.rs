/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Observability Errors

use std::fmt;

use aws_smithy_runtime_api::box_error::BoxError;

/// An error in the SDKs Observability providers
#[non_exhaustive]
#[derive(Debug)]
pub struct ObservabilityError {
    kind: ErrorKind,
    source: BoxError,
}

/// The types of errors associated with [ObservabilityError]
#[non_exhaustive]
#[derive(Debug)]
pub enum ErrorKind {
    /// An error setting the `GlobalTelemetryProvider``
    SettingGlobalProvider,
    /// Error flushing metrics pipeline
    MetricsFlush,
    /// Error gracefully shutting down Metrics Provider
    MetricsShutdown,
    /// A custom error that does not fall under any other error kind
    Other,
}

impl ObservabilityError {
    /// Create a new [`ObservabilityError`] from an [ErrorKind] and a [BoxError]
    pub fn new<E>(kind: ErrorKind, err: E) -> Self
    where
        E: Into<BoxError>,
    {
        Self {
            kind,
            source: err.into(),
        }
    }

    /// Returns the corresponding [`ErrorKind`] for this error.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl fmt::Display for ObservabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Other => write!(f, "unclassified error"),
            ErrorKind::SettingGlobalProvider => {
                write!(f, "failed to set global telemetry provider")
            }
            ErrorKind::MetricsFlush => write!(f, "failed to flush metrics pipeline"),
            ErrorKind::MetricsShutdown => write!(f, "failed to shutdown metrics provider"),
        }
    }
}

impl std::error::Error for ObservabilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

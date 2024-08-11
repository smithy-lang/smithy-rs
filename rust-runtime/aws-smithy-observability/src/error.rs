/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Observability Errors

use std::fmt;

use aws_smithy_runtime_api::box_error::BoxError;

#[derive(Debug)]
pub(crate) struct ObservabilityError {
    kind: ErrorKind,
    source: BoxError,
}

//TODO(smithyObservability): refine the error kinds
#[non_exhaustive]
#[derive(Debug)]
pub(crate) enum ErrorKind {
    /// An error setting the `GlobalTelemetryProvider``
    SettingGlobalProvider,
    /// A custom error that does not fall under any other error kind
    Other,
}

impl ObservabilityError {
    pub(crate) fn new<E>(kind: ErrorKind, err: E) -> Self
    where
        E: Into<BoxError>,
    {
        Self {
            kind,
            source: err.into(),
        }
    }

    /// Returns the corresponding [`ErrorKind`] for this error.
    pub(crate) fn kind(&self) -> &ErrorKind {
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
        }
    }
}

impl std::error::Error for ObservabilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

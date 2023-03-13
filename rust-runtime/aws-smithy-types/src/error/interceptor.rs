/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors related to smithy interceptors

use std::fmt;

/// A generic error that behaves itself in async contexts
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// An error related to smithy interceptors.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    source: BoxError,
}

impl Error {
    /// Create a new error indicating a failure withing a read_before_execution interceptor
    pub fn read_before_execution(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeExecution,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_serialization interceptor
    pub fn modify_before_serialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeSerialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_before_serialization interceptor
    pub fn read_before_serialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeSerialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_serialization interceptor
    pub fn read_after_serialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterSerialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_retry_loop interceptor
    pub fn modify_before_retry_loop(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeRetryLoop,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_before_attempt interceptor
    pub fn read_before_attempt(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeAttempt,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_signing interceptor
    pub fn modify_before_signing(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeSigning,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_before_signing interceptor
    pub fn read_before_signing(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeSigning,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_signing interceptor
    pub fn read_after_signing(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterSigning,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_transmit interceptor
    pub fn modify_before_transmit(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeTransmit,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_before_transmit interceptor
    pub fn read_before_transmit(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeTransmit,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_transmit interceptor
    pub fn read_after_transmit(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterTransmit,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_deserialization interceptor
    pub fn modify_before_deserialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeDeserialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_before_deserialization interceptor
    pub fn read_before_deserialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadBeforeDeserialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_deserialization interceptor
    pub fn read_after_deserialization(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterDeserialization,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_attempt_completion interceptor
    pub fn modify_before_attempt_completion(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeAttemptCompletion,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_attempt interceptor
    pub fn read_after_attempt(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterAttempt,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a modify_before_completion interceptor
    pub fn modify_before_completion(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ModifyBeforeCompletion,
            source: source.into(),
        }
    }
    /// Create a new error indicating a failure withing a read_after_execution interceptor
    pub fn read_after_execution(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::ReadAfterExecution,
            source: source.into(),
        }
    }
    /// Create a new error indicating that an interceptor tried to access the tx_request out of turn
    pub fn invalid_tx_request_access(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::InvalidTxRequestAccess,
            source: source.into(),
        }
    }
    /// Create a new error indicating that an interceptor tried to access the tx_response out of turn
    pub fn invalid_tx_response_access(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::InvalidTxResponseAccess,
            source: source.into(),
        }
    }
    /// Create a new error indicating that an interceptor tried to access the modeled_response out of turn
    pub fn invalid_modeled_response_access(
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind: ErrorKind::InvalidModeledResponseAccess,
            source: source.into(),
        }
    }
}

#[derive(Debug)]
enum ErrorKind {
    /// An error occurred within the read_before_execution interceptor
    ReadBeforeExecution,
    /// An error occurred within the modify_before_serialization interceptor
    ModifyBeforeSerialization,
    /// An error occurred within the read_before_serialization interceptor
    ReadBeforeSerialization,
    /// An error occurred within the read_after_serialization interceptor
    ReadAfterSerialization,
    /// An error occurred within the modify_before_retry_loop interceptor
    ModifyBeforeRetryLoop,
    /// An error occurred within the read_before_attempt interceptor
    ReadBeforeAttempt,
    /// An error occurred within the modify_before_signing interceptor
    ModifyBeforeSigning,
    /// An error occurred within the read_before_signing interceptor
    ReadBeforeSigning,
    /// An error occurred within the read_after_signing interceptor
    ReadAfterSigning,
    /// An error occurred within the modify_before_transmit interceptor
    ModifyBeforeTransmit,
    /// An error occurred within the read_before_transmit interceptor
    ReadBeforeTransmit,
    /// An error occurred within the read_after_transmit interceptor
    ReadAfterTransmit,
    /// An error occurred within the modify_before_deserialization interceptor
    ModifyBeforeDeserialization,
    /// An error occurred within the read_before_deserialization interceptor
    ReadBeforeDeserialization,
    /// An error occurred within the read_after_deserialization interceptor
    ReadAfterDeserialization,
    /// An error occurred within the modify_before_attempt_completion interceptor
    ModifyBeforeAttemptCompletion,
    /// An error occurred within the read_after_attempt interceptor
    ReadAfterAttempt,
    /// An error occurred within the modify_before_completion interceptor
    ModifyBeforeCompletion,
    /// An error occurred within the read_after_execution interceptor
    ReadAfterExecution,
    // There is no InvalidModeledRequestAccess because it's always accessible
    /// An interceptor tried to access the tx_request out of turn
    InvalidTxRequestAccess,
    /// An interceptor tried to access the tx_response out of turn
    InvalidTxResponseAccess,
    /// An interceptor tried to access the modeled_response out of turn
    InvalidModeledResponseAccess,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ErrorKind::*;
        match &self.kind {
            ReadBeforeExecution => {
                write!(f, "read_before_execution interceptor encountered an error")
            }
            ModifyBeforeSerialization => write!(
                f,
                "modify_before_serialization interceptor encountered an error"
            ),
            ReadBeforeSerialization => write!(
                f,
                "read_before_serialization interceptor encountered an error"
            ),
            ReadAfterSerialization => write!(
                f,
                "read_after_serialization interceptor encountered an error"
            ),
            ModifyBeforeRetryLoop => write!(
                f,
                "modify_before_retry_loop interceptor encountered an error"
            ),
            ReadBeforeAttempt => write!(f, "read_before_attempt interceptor encountered an error"),
            ModifyBeforeSigning => {
                write!(f, "modify_before_signing interceptor encountered an error")
            }
            ReadBeforeSigning => write!(f, "read_before_signing interceptor encountered an error"),
            ReadAfterSigning => write!(f, "read_after_signing interceptor encountered an error"),
            ModifyBeforeTransmit => {
                write!(f, "modify_before_transmit interceptor encountered an error")
            }
            ReadBeforeTransmit => {
                write!(f, "read_before_transmit interceptor encountered an error")
            }
            ReadAfterTransmit => write!(f, "read_after_transmit interceptor encountered an error"),
            ModifyBeforeDeserialization => write!(
                f,
                "modify_before_deserialization interceptor encountered an error"
            ),
            ReadBeforeDeserialization => write!(
                f,
                "read_before_deserialization interceptor encountered an error"
            ),
            ReadAfterDeserialization => write!(
                f,
                "read_after_deserialization interceptor encountered an error"
            ),
            ModifyBeforeAttemptCompletion => write!(
                f,
                "modify_before_attempt_completion interceptor encountered an error"
            ),
            ReadAfterAttempt => write!(f, "read_after_attempt interceptor encountered an error"),
            ModifyBeforeCompletion => write!(
                f,
                "modify_before_completion interceptor encountered an error"
            ),
            ReadAfterExecution => {
                write!(f, "read_after_execution interceptor encountered an error")
            }
            InvalidTxRequestAccess => {
                write!(f, "tried to access tx_request before request serialization")
            }
            InvalidTxResponseAccess => write!(
                f,
                "tried to access tx_response before transmitting a request"
            ),
            InvalidModeledResponseAccess => write!(
                f,
                "tried to access modeled_response before response deserialization"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

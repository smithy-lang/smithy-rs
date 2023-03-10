/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors related to smithy interceptors

use std::fmt;

/// A generic error that behaves itself in async contexts
pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Errors related to smithy interceptors.
#[derive(Debug)]
pub enum InterceptorError {
    /// An error occurred within the read_before_execution interceptor
    ReadBeforeExecution(DynError),
    /// An error occurred within the modify_before_serialization interceptor
    ModifyBeforeSerialization(DynError),
    /// An error occurred within the read_before_serialization interceptor
    ReadBeforeSerialization(DynError),
    /// An error occurred within the read_after_serialization interceptor
    ReadAfterSerialization(DynError),
    /// An error occurred within the modify_before_retry_loop interceptor
    ModifyBeforeRetryLoop(DynError),
    /// An error occurred within the read_before_attempt interceptor
    ReadBeforeAttempt(DynError),
    /// An error occurred within the modify_before_signing interceptor
    ModifyBeforeSigning(DynError),
    /// An error occurred within the read_before_signing interceptor
    ReadBeforeSigning(DynError),
    /// An error occurred within the read_after_signing interceptor
    ReadAfterSigning(DynError),
    /// An error occurred within the modify_before_transmit interceptor
    ModifyBeforeTransmit(DynError),
    /// An error occurred within the read_before_transmit interceptor
    ReadBeforeTransmit(DynError),
    /// An error occurred within the read_after_transmit interceptor
    ReadAfterTransmit(DynError),
    /// An error occurred within the modify_before_deserialization interceptor
    ModifyBeforeDeserialization(DynError),
    /// An error occurred within the read_before_deserialization interceptor
    ReadBeforeDeserialization(DynError),
    /// An error occurred within the read_after_deserialization interceptor
    ReadAfterDeserialization(DynError),
    /// An error occurred within the modify_before_attempt_completion interceptor
    ModifyBeforeAttemptCompletion(DynError),
    /// An error occurred within the read_after_attempt interceptor
    ReadAfterAttempt(DynError),
    /// An error occurred within the modify_before_completion interceptor
    ModifyBeforeCompletion(DynError),
    /// An error occurred within the read_after_execution interceptor
    ReadAfterExecution(DynError),
    /// If you see this error, it means you found a bug. Please report it.
    AlreadyAtBottomOfStack,
    // There is no InvalidModeledRequestAccess because it's always accessible
    /// An interceptor tried to access the tx_request out of turn
    InvalidTxRequestAccess,
    /// An interceptor tried to access the tx_response out of turn
    InvalidTxResponseAccess,
    /// An interceptor tried to access the modeled_response out of turn
    InvalidModeledResponseAccess,
}

impl fmt::Display for InterceptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use InterceptorError::*;
        match self {
            ReadBeforeExecution(err) => write!(f, "read_before_execution interceptor encountered an error: {}", err),
            ModifyBeforeSerialization(err) => write!(f, "modify_before_serialization interceptor encountered an error: {}", err),
            ReadBeforeSerialization(err) => write!(f, "read_before_serialization interceptor encountered an error: {}", err),
            ReadAfterSerialization(err) => write!(f, "read_after_serialization interceptor encountered an error: {}", err),
            ModifyBeforeRetryLoop(err) => write!(f, "modify_before_retry_loop interceptor encountered an error: {}", err),
            ReadBeforeAttempt(err) => write!(f, "read_before_attempt interceptor encountered an error: {}", err),
            ModifyBeforeSigning(err) => write!(f, "modify_before_signing interceptor encountered an error: {}", err),
            ReadBeforeSigning(err) => write!(f, "read_before_signing interceptor encountered an error: {}", err),
            ReadAfterSigning(err) => write!(f, "read_after_signing interceptor encountered an error: {}", err),
            ModifyBeforeTransmit(err) => write!(f, "modify_before_transmit interceptor encountered an error: {}", err),
            ReadBeforeTransmit(err) => write!(f, "read_before_transmit interceptor encountered an error: {}", err),
            ReadAfterTransmit(err) => write!(f, "read_after_transmit interceptor encountered an error: {}", err),
            ModifyBeforeDeserialization(err) => write!(f, "modify_before_deserialization interceptor encountered an error: {}", err),
            ReadBeforeDeserialization(err) => write!(f, "read_before_deserialization interceptor encountered an error: {}", err),
            ReadAfterDeserialization(err) => write!(f, "read_after_deserialization interceptor encountered an error: {}", err),
            ModifyBeforeAttemptCompletion(err) => write!(f, "modify_before_attempt_completion interceptor encountered an error: {}", err),
            ReadAfterAttempt(err) => write!(f, "read_after_attempt interceptor encountered an error: {}", err),
            ModifyBeforeCompletion(err) => write!(f, "modify_before_completion interceptor encountered an error: {}", err),
            ReadAfterExecution(err) => write!(f, "read_after_execution interceptor encountered an error: {}", err),
            AlreadyAtBottomOfStack => write!(f, "can't pop interceptor context: already at bottom layer. This is a bug, please report it"),
            InvalidTxRequestAccess => write!(f, "tried to access tx_request before request serialization"),
            InvalidTxResponseAccess => write!(f, "tried to access tx_response before transmitting a request"),
            InvalidModeledResponseAccess => write!(f, "tried to access modeled_response before response deserialization"),
        }
    }
}

impl std::error::Error for InterceptorError {}

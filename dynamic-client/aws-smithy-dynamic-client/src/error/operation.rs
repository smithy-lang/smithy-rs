/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::error::metadata::{ErrorMetadata, ProvideErrorMetadata};
use std::error::Error;
use std::fmt;

/// An error response from the service.
///
/// The code, message, and request ID of the response are read with [`ProvideErrorMetadata`].
// TODO(dynamic-client): Add the modeled error shape, its members as a document, and its retry
// kind. Implement `CreateUnhandledError`, so `SdkError::into_service_error` works.
#[derive(Debug)]
pub struct DynamicOperationError {
    meta: ErrorMetadata,
}

impl DynamicOperationError {
    pub(crate) fn unhandled(meta: ErrorMetadata) -> Self {
        Self { meta }
    }
}

impl ProvideErrorMetadata for DynamicOperationError {
    fn meta(&self) -> &ErrorMetadata {
        &self.meta
    }
}

impl fmt::Display for DynamicOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.meta.code(), self.meta.message()) {
            (Some(code), Some(message)) => write!(f, "the service returned `{code}`: {message}"),
            (Some(code), None) => write!(f, "the service returned `{code}`"),
            (None, _) => write!(f, "the service returned an error"),
        }
    }
}

impl Error for DynamicOperationError {}

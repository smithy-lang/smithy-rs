/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_lang::ShapeId;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::result::CreateUnhandledError;
use aws_smithy_types::error::metadata::{ErrorMetadata, ProvideErrorMetadata};
use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind};
use aws_smithy_types::Document;
use std::error::Error;
use std::fmt;

/// An error response from the service.
///
/// A modeled error has the shape ID of its error structure, and its members as a document. An
/// error the model does not list is unhandled. An unhandled error has no shape ID and no members.
/// The code, message, and request ID of the response are read with [`ProvideErrorMetadata`].
#[derive(Debug)]
pub struct DynamicOperationError {
    modeled: Option<Modeled>,
    meta: ErrorMetadata,
    source: Option<BoxError>,
}

#[derive(Debug)]
struct Modeled {
    shape_id: ShapeId,
    members: Document,
    retryable: Option<ErrorKind>,
}

impl DynamicOperationError {
    pub(crate) fn modeled(
        shape_id: ShapeId,
        members: Document,
        retryable: Option<ErrorKind>,
        meta: ErrorMetadata,
    ) -> Self {
        Self {
            modeled: Some(Modeled {
                shape_id,
                members,
                retryable,
            }),
            meta,
            source: None,
        }
    }

    pub(crate) fn unhandled(meta: ErrorMetadata) -> Self {
        Self {
            modeled: None,
            meta,
            source: None,
        }
    }

    /// Returns the shape ID of the error structure, or `None` for an unhandled error.
    pub fn shape_id(&self) -> Option<&ShapeId> {
        self.modeled.as_ref().map(|modeled| &modeled.shape_id)
    }

    /// Returns the members of the error structure, as a document object keyed by Smithy member
    /// names, or `None` for an unhandled error.
    pub fn members(&self) -> Option<&Document> {
        self.modeled.as_ref().map(|modeled| &modeled.members)
    }
}

impl ProvideErrorMetadata for DynamicOperationError {
    fn meta(&self) -> &ErrorMetadata {
        &self.meta
    }
}

impl ProvideErrorKind for DynamicOperationError {
    fn retryable_error_kind(&self) -> Option<ErrorKind> {
        self.modeled.as_ref().and_then(|modeled| modeled.retryable)
    }

    fn code(&self) -> Option<&str> {
        self.meta.code()
    }
}

impl CreateUnhandledError for DynamicOperationError {
    fn create_unhandled_error(source: BoxError, meta: Option<ErrorMetadata>) -> Self {
        Self {
            modeled: None,
            meta: meta.unwrap_or_default(),
            source: Some(source),
        }
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

impl Error for DynamicOperationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|source| source.as_ref() as _)
    }
}

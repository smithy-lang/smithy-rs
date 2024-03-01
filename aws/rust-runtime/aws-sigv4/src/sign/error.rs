/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::http_request::SigningError;
use aws_smithy_runtime_api::box_error::BoxError;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
enum SignWithErrorKind {
    FailedToProduceSignature { source: SigningError },
    FailedToApplySignatureToRequest { source: BoxError },
}

/// Error that occurred while producing a signature and applying it to a request
#[derive(Debug)]
pub struct SignWithError {
    kind: SignWithErrorKind,
}

impl SignWithError {
    pub(crate) fn failed_to_produce_signature(source: SigningError) -> Self {
        Self {
            kind: SignWithErrorKind::FailedToProduceSignature { source },
        }
    }
    pub(crate) fn failed_to_apply_signature_to_request(source: BoxError) -> Self {
        Self {
            kind: SignWithErrorKind::FailedToApplySignatureToRequest { source },
        }
    }
}

impl fmt::Display for SignWithError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SignWithErrorKind::*;
        match self.kind {
            FailedToProduceSignature { .. } => {
                write!(f, "failed to produce signature")
            }
            FailedToApplySignatureToRequest { .. } => {
                write!(f, "failed to apply signature to request")
            }
        }
    }
}

impl Error for SignWithError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        use SignWithErrorKind::*;
        Some(match &self.kind {
            FailedToProduceSignature { source } => source,
            FailedToApplySignatureToRequest { source } => source.as_ref(),
        })
    }
}

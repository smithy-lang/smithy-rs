/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::diagnostic::{Diagnostic, DiagnosticSet};
use std::fmt;

/// Building runtime schemas from a model failed.
///
/// Assembly succeeded, but a schema-relevant trait value was malformed, the model contains a
/// cycle that schemas cannot represent, or the requested scope does not exist. Every
/// diagnostic is available through [`SchemaBuildError::diagnostics`]; the
/// [`DiagnosticCode`](crate::diagnostic::DiagnosticCode) of each identifies the problem:
///
/// - [`InvalidTrait`](crate::diagnostic::DiagnosticCode::InvalidTrait): a trait value has the
///   wrong type, an unknown enumerated value, or lacks a required property. The location is
///   that of the offending value.
/// - [`InvalidShape`](crate::diagnostic::DiagnosticCode::InvalidShape): list and map members
///   form a cycle that does not pass through a structure or union.
/// - [`UnresolvedReference`](crate::diagnostic::DiagnosticCode::UnresolvedReference) or
///   [`InvalidTarget`](crate::diagnostic::DiagnosticCode::InvalidTarget): the requested
///   service does not exist or is not a service, or an operation selected with
///   [`for_service_operations`](super::RuntimeSchemaSetBuilder::for_service_operations) does
///   not exist, is not an operation, or is not contained in the service.
///
/// Diagnostics are reported in model order and bounded by
/// [`RuntimeSchemaSetBuilder::max_diagnostics`](super::RuntimeSchemaSetBuilder::max_diagnostics).
#[derive(Debug)]
pub struct SchemaBuildError {
    diagnostics: DiagnosticSet,
}

impl SchemaBuildError {
    pub(super) fn new(diagnostics: DiagnosticSet) -> Self {
        debug_assert!(diagnostics.has_errors());
        Self { diagnostics }
    }

    /// Every diagnostic, in model order.
    pub fn diagnostics(&self) -> &DiagnosticSet {
        &self.diagnostics
    }

    /// Consumes the error, returning its diagnostics.
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics
    }
}

impl fmt::Display for SchemaBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to build runtime schemas")?;
        let errors = self.diagnostics.errors().count();
        if errors > 1 {
            write!(f, " ({errors} errors); first")?;
        }
        if let Some(first) = self.diagnostics.errors().next() {
            if let Some(location) = first.source_location() {
                write!(f, " at {}", location.short())?;
            }
            write!(f, ": {}", Diagnostic::message(first))?;
        }
        Ok(())
    }
}

impl std::error::Error for SchemaBuildError {}

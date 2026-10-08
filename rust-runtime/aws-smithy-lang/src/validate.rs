/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `StructuralV1` validation.

use crate::ast::Document;
use crate::diagnostic::DiagnosticSet;
use crate::loader::ModelLoader;

/// Validates `document` against `StructuralV1`. Implemented in a follow-up step.
pub(crate) fn validate(
    _document: &Document,
    _prelude: Option<&Document>,
    _options: &ModelLoader,
) -> DiagnosticSet {
    DiagnosticSet::default()
}

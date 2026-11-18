/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Model validation: the [`Validator`] extension point and the [`Validated`] result.
//!
//! [`ModelAssembler::assemble`](crate::ModelAssembler::assemble) validates in two phases, like
//! Java Smithy:
//!
//! 1. **Critical checks** run on the assembled shapes before any [`Model`] exists. They cover
//!    what every `Model` guarantees, so they cannot be disabled: every ordinary reference
//!    resolves to a root shape of an allowed type, no ordinary reference targets a trait
//!    definition, resource containment is acyclic, user shapes do not redefine the prelude, and
//!    unsupported features (such as `@mixin`) are rejected. If any fails, validation stops.
//! 2. **Validators** then run against the candidate [`Model`]: first the built-in
//!    `StructuralV1` validators, then custom validators in registration order. Because the
//!    critical checks passed, validators can use every view and [`knowledge`](crate::knowledge)
//!    index without defensive code. Any error discards the model.
//!
//! Built-in validators report their [`name`](Validator::name) through
//! [`Diagnostic::validator`]:
//!
//! | Validator | Checks |
//! | --------- | ------ |
//! | `CaseConflict` | Shape IDs and member names are case-insensitively unique. |
//! | `TraitDefinition` | Applied traits that resolve are trait definitions; with `require_trait_definitions`, every applied trait resolves. |
//! | `MapKey` | Map keys target `string` or `enum`. |
//! | `UnionMembers` | Unions have at least one member. |
//! | `EnumMembers` | Enums and intEnums have members that target `smithy.api#Unit` and have valid, unique values. |
//! | `UnitTarget` | Only operation input/output and union, enum, and intEnum members target `smithy.api#Unit`. |
//! | `ErrorTarget` | Service and operation `errors` target `@error` structures. |
//! | `ServiceRename` | Service `rename` keys resolve and values are valid, case-insensitively unique identifiers. |
//!
//! ```
//! use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
//! use aws_smithy_lang::{validation, Model};
//!
//! // Warn about operations whose names do not start with a verb from an allowed list.
//! let verbs = validation::from_fn("OperationVerb", |model, diagnostics| {
//!     for operation in model.operations().filter(|op| !op.is_prelude()) {
//!         let name = operation.id().name();
//!         if !["Get", "Put", "List", "Delete"].iter().any(|v| name.starts_with(v)) {
//!             diagnostics.push(
//!                 Diagnostic::warning(DiagnosticCode::Custom, format!("`{name}` should start with a verb"))
//!                     .with_shape_id(operation.id().clone())
//!                     .with_source_location(operation.source_location().clone()),
//!             );
//!         }
//!     }
//! });
//!
//! let validated = Model::assembler()
//!     .add_json_str("m.json", r#"{"smithy": "2.0", "shapes": {"ex#Weather": {"type": "operation"}}}"#)
//!     .add_validator(verbs)
//!     .assemble();
//! // Warnings do not prevent a model.
//! assert!(validated.result().is_some());
//! let warning = validated.diagnostics().iter().next().unwrap();
//! assert_eq!(warning.severity(), Severity::Warning);
//! assert_eq!(warning.validator(), Some("OperationVerb"));
//! ```

mod builtin;

pub(crate) use builtin::builtin_validators;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSet, LoadError, LoadErrorKind};
use crate::model::Model;
use crate::traits::TraitCodec;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// Checks a [`Model`] and reports problems as [`Diagnostic`]s.
///
/// Register validators with
/// [`ModelAssembler::add_validator`](crate::ModelAssembler::add_validator). A validator runs
/// after the critical checks, so the model's references all resolve. Reporting an
/// [`Error`](crate::diagnostic::Severity::Error) prevents the model from being returned.
///
/// Use [`from_fn`] for simple validators.
pub trait Validator: Send + Sync {
    /// A stable name identifying the validator, recorded on each diagnostic it reports.
    fn name(&self) -> &str;

    /// Validates `model`, reporting problems to `diagnostics`.
    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>);
}

/// Where a [`Validator`] reports diagnostics.
///
/// The sink records the validator's name on each diagnostic and enforces the assembler's
/// diagnostic limit.
pub struct DiagnosticSink<'a> {
    diagnostics: &'a mut DiagnosticSet,
    validator: Arc<str>,
}

impl<'a> DiagnosticSink<'a> {
    pub(crate) fn new(diagnostics: &'a mut DiagnosticSet, validator: &str) -> Self {
        Self {
            diagnostics,
            validator: Arc::from(validator),
        }
    }

    /// Reports a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics
            .push(diagnostic.with_validator(&self.validator));
    }
}

impl fmt::Debug for DiagnosticSink<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticSink")
            .field("validator", &self.validator)
            .finish_non_exhaustive()
    }
}

/// Creates a [`Validator`] from a name and a function.
pub fn from_fn<F>(name: impl Into<String>, validate: F) -> FnValidator<F>
where
    F: Fn(&Model, &mut DiagnosticSink<'_>) + Send + Sync,
{
    FnValidator {
        name: name.into(),
        validate,
    }
}

/// A [`Validator`] created by [`from_fn`].
pub struct FnValidator<F> {
    name: String,
    validate: F,
}

impl<F> Validator for FnValidator<F>
where
    F: Fn(&Model, &mut DiagnosticSink<'_>) + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        (self.validate)(model, diagnostics)
    }
}

impl<F> fmt::Debug for FnValidator<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FnValidator")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A [`Validator`] that decodes every application of a trait with its [`TraitCodec`],
/// reporting malformed values as [`InvalidTrait`](DiagnosticCode::InvalidTrait) errors at the
/// offending value.
///
/// The model keeps storing trait values as generic nodes; this only makes malformed values of
/// trait `T` fail assembly instead of failing later when they are decoded.
///
/// ```
/// use aws_smithy_lang::traits::Documentation;
/// use aws_smithy_lang::{validation, Model};
///
/// let json = r#"{"smithy": "2.0", "shapes": {
///     "ex#A": {"type": "string", "traits": {"smithy.api#documentation": 5}}
/// }}"#;
/// let validated = Model::assembler()
///     .add_json_str("m.json", json)
///     .add_validator(validation::trait_codec::<Documentation>())
///     .assemble();
/// let error = validated.diagnostics().errors().next().unwrap();
/// let location = error.source_location().unwrap();
/// assert_eq!(location.pointer(), Some("/shapes/ex#A/traits/smithy.api#documentation"));
/// ```
pub fn trait_codec<T: TraitCodec + 'static>() -> TraitCodecValidator<T> {
    TraitCodecValidator {
        name: format!("TraitCodec[{}]", T::ID),
        _codec: PhantomData,
    }
}

/// A [`Validator`] created by [`trait_codec`].
pub struct TraitCodecValidator<T> {
    name: String,
    _codec: PhantomData<fn() -> T>,
}

impl<T: TraitCodec> Validator for TraitCodecValidator<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for shape in model.shapes_with_trait(&T::ID) {
            if let Err(err) = shape.get_trait_as::<T>() {
                let message = err.to_string();
                diagnostics.push(
                    Diagnostic::error(DiagnosticCode::InvalidTrait, message)
                        .with_shape_id(shape.id().clone())
                        .with_source_location(err.source_location().clone()),
                );
            }
        }
    }
}

impl<T> fmt::Debug for TraitCodecValidator<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraitCodecValidator")
            .field("name", &self.name)
            .finish()
    }
}

/// A result together with every diagnostic produced while computing it.
///
/// [`result`](Self::result) is `Some` if and only if no diagnostic is an
/// [`Error`](crate::diagnostic::Severity::Error). Notes and warnings may accompany a result; use
/// [`into_parts`](Self::into_parts) to keep them, or [`into_result`](Self::into_result) when
/// only errors matter.
#[derive(Debug)]
pub struct Validated<T> {
    result: Option<T>,
    diagnostics: DiagnosticSet,
    failure: Option<LoadErrorKind>,
}

impl<T> Validated<T> {
    pub(crate) fn new(
        result: Option<T>,
        diagnostics: DiagnosticSet,
        failure: Option<LoadErrorKind>,
    ) -> Self {
        let result = result.filter(|_| !diagnostics.has_errors());
        Self {
            result,
            diagnostics,
            failure,
        }
    }

    /// The result, if there were no errors.
    pub fn result(&self) -> Option<&T> {
        self.result.as_ref()
    }

    /// Every diagnostic, including notes and warnings.
    pub fn diagnostics(&self) -> &DiagnosticSet {
        &self.diagnostics
    }

    /// Returns `true` if any diagnostic is an error, in which case there is no result.
    pub fn has_errors(&self) -> bool {
        self.result.is_none()
    }

    /// Returns the result, or a [`LoadError`] carrying every diagnostic. Notes and warnings
    /// that accompany a successful result are discarded.
    pub fn into_result(self) -> Result<T, LoadError> {
        match self.result {
            Some(result) => Ok(result),
            None => Err(LoadError::from_kind(
                self.failure.unwrap_or(LoadErrorKind::InvalidModel),
                self.diagnostics,
            )),
        }
    }

    /// Returns the result (if there were no errors) and every diagnostic.
    pub fn into_parts(self) -> (Option<T>, DiagnosticSet) {
        (self.result, self.diagnostics)
    }
}

#[cfg(test)]
mod tests;

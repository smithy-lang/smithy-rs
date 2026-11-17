/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The waiter validator: checks the rules of `smithy.waiters#waitable` that decoding does not.

use super::codec::{
    Context, ACCEPTORS, ACCEPTOR_PROPERTIES, INPUT_OUTPUT, MATCHER, OUTPUT,
    PATH_MATCHER_PROPERTIES, WAITER_PROPERTIES,
};
use super::expression::{self, Analysis, Expected, ParseError, Type};
use super::{AcceptorState, MatcherKind, PathComparator, PathMatcher, Waitable, Waiter, WAITABLE};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceLocation};
use crate::model::Model;
use crate::node::{Node, NodeObject};
use crate::shape::{OperationView, ShapeType};
use crate::shape_id::ShapeId;
use crate::traits::TraitCodec;
use crate::validation::{DiagnosticSink, Validator};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fmt::Write as _;

const STREAMING: ShapeId = ShapeId::from_static("smithy.api#streaming");

/// A [`Validator`] for `smithy.waiters#waitable` traits.
///
/// Errors:
///
/// - a malformed trait value, reported as decoding reports it;
/// - the trait applied to a shape that is not an operation, or to an operation whose input
///   or output has an event stream;
/// - a JMESPath `path` or `message` that does not parse, calls an unknown function, or calls a
///   function with the wrong number of arguments;
/// - an expression longer than 2048 bytes, or with more than 32 nesting tokens (`(`, `[`,
///   `{`, `!`, `&`, `*`), which the validator does not parse, so that model content cannot
///   exhaust the stack;
/// - a waiter whose `minDelay` is greater than its `maxDelay`, or with no `success`
///   acceptor;
/// - a `message` on an acceptor whose state is not `failure`;
/// - a `message` known not to evaluate to a string or an array of strings;
/// - a `booleanEquals` matcher that does not expect `"true"` or `"false"`; and
/// - two waiters whose names differ only by case in the closure of a service, including two
///   waiters of one operation.
///
/// Warnings:
///
/// - a waiter, acceptor, or path matcher property that decoding ignores;
/// - an `errorType` that is neither the shape ID nor the name of an error the operation
///   lists directly (it can still name an error the model does not define);
/// - a path known not to evaluate to the type its comparator compares; and
/// - other problems static analysis finds in an expression, such as selecting a member a
///   structure does not have, or passing a function an argument of the wrong type.
///
/// Expressions are evaluated against the operation's output, or for an `inputOutput`
/// matcher against an object with `input` and `output` members; a `message` is always
/// evaluated against the output. An omitted input or output is the empty `smithy.api#Unit`
/// structure. Fields are resolved by member name.
///
/// # Differences from the reference implementation
///
/// The reference implementation lints an expression by evaluating it against sample data
/// built from the model. This validator infers types instead and reports nothing that
/// depends on a type it cannot know, such as the value of a `document`. As a result:
///
/// - It reports no error that the reference implementation does not, except for expressions
///   beyond its size limits, expressions the two JMESPath parsers disagree about, and waiter
///   names that differ only by case on the same operation, which the specification forbids
///   but the reference implementation does not check.
/// - A function argument of the wrong type is a warning; the reference implementation reports
///   an error.
/// - A `message` whose type cannot be known is accepted; the reference implementation
///   requires its sample evaluation to produce a string or an array of strings.
/// - Warnings for the same expression can differ.
///
/// The validator is not run by default:
///
/// ```
/// use aws_smithy_lang::{waiters, Model};
///
/// let model = Model::assembler()
///     .add_validator(waiters::validator())
///     .assemble()
///     .into_result()?;
/// # Ok::<_, aws_smithy_lang::LoadError>(())
/// ```
pub fn validator() -> impl Validator {
    WaitersValidator
}

struct WaitersValidator;

impl Validator for WaitersValidator {
    fn name(&self) -> &str {
        "Waiters"
    }

    fn validate(&self, model: &Model, sink: &mut DiagnosticSink<'_>) {
        // The traits of operations, for the service-wide name check.
        let mut decoded: HashMap<&ShapeId, Waitable> = HashMap::new();
        let trait_id = WAITABLE;
        for shape in model.shapes_with_trait(&trait_id) {
            let id = shape.id();
            let value = shape.get_trait(&WAITABLE).expect("the shape has the trait");
            let mut report =
                |diagnostic: Diagnostic| sink.push(diagnostic.with_shape_id(id.clone()));

            let operation = shape.as_shape().and_then(|shape| shape.as_operation());
            if operation.is_none() {
                report(error(
                    value.source_location(),
                    format!(
                        "`{WAITABLE}` can only be applied to operations, but `{id}` is a {}",
                        shape.shape_type()
                    ),
                ));
            }
            let waitable = match Waitable::from_node(value) {
                Ok(waitable) => waitable,
                Err(err) => {
                    report(error(
                        err.source_location(),
                        format!("invalid `{WAITABLE}` trait value: {}", err.message()),
                    ));
                    continue;
                }
            };
            let Some(operation) = operation else {
                continue;
            };
            check_event_streams(operation, value, &mut report);
            unknown_properties(value, &mut report);
            for (name, waiter) in waitable.waiters() {
                check_waiter(operation, name, waiter, &mut report);
            }
            decoded.insert(id, waitable);
        }
        check_unique_names(model, &decoded, sink);
    }
}

fn error(location: &SourceLocation, message: String) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::InvalidTrait, message).with_source_location(location.clone())
}

fn warning(location: &SourceLocation, message: String) -> Diagnostic {
    Diagnostic::warning(DiagnosticCode::InvalidTrait, message)
        .with_source_location(location.clone())
}

/// Reports an error for each event stream in the operation's input or output, which the
/// trait's selector excludes.
fn check_event_streams(
    operation: OperationView<'_>,
    value: &Node,
    report: &mut impl FnMut(Diagnostic),
) {
    for (direction, structure) in [
        ("input", operation.input_shape()),
        ("output", operation.output_shape()),
    ] {
        for member in structure.members().iter() {
            let target = member.target_shape();
            if target.shape_type() == ShapeType::Union && target.has_trait(&STREAMING) {
                report(error(
                    value.source_location(),
                    format!(
                        "`{WAITABLE}` cannot be applied to an operation with an event stream, \
                         but {direction} member `{}` is an event stream",
                        member.id()
                    ),
                ));
            }
        }
    }
}

/// Warns about each structure property in the decoded trait `value` that decoding ignored.
fn unknown_properties(value: &Node, report: &mut impl FnMut(Diagnostic)) {
    let mut check = |object: &NodeObject, known: &[&str], what: &str, context: Context<'_>| {
        for (key, property) in object.iter() {
            if !known.contains(&key) {
                report(
                    Diagnostic::warning(
                        DiagnosticCode::UnknownProperty,
                        format!("{context}: unknown {what} property `{key}` is ignored"),
                    )
                    .with_source_location(property.source_location().clone()),
                );
            }
        }
    };
    let waiters = value.as_object().into_iter().flat_map(NodeObject::iter);
    for (name, waiter) in waiters {
        let Some(waiter) = waiter.as_object() else {
            continue;
        };
        let context = Context {
            waiter: name,
            acceptor: None,
        };
        check(waiter, &WAITER_PROPERTIES, "waiter", context);
        let acceptors = waiter.get(ACCEPTORS).and_then(Node::as_array);
        for (index, acceptor) in acceptors.into_iter().flatten().enumerate() {
            let Some(acceptor) = acceptor.as_object() else {
                continue;
            };
            let context = Context {
                acceptor: Some(index),
                ..context
            };
            check(acceptor, &ACCEPTOR_PROPERTIES, "acceptor", context);
            let matcher = acceptor.get(MATCHER).and_then(Node::as_object);
            for path_matcher in matcher
                .into_iter()
                .flat_map(|matcher| [matcher.get(OUTPUT), matcher.get(INPUT_OUTPUT)])
                .flatten()
                .filter_map(Node::as_object)
            {
                check(
                    path_matcher,
                    &PATH_MATCHER_PROPERTIES,
                    "path matcher",
                    context,
                );
            }
        }
    }
}

fn check_waiter(
    operation: OperationView<'_>,
    name: &str,
    waiter: &Waiter,
    report: &mut impl FnMut(Diagnostic),
) {
    let context = Context {
        waiter: name,
        acceptor: None,
    };
    if waiter.min_delay() > waiter.max_delay() {
        report(error(
            waiter.source_location(),
            format!(
                "{context}: `minDelay` of {} is greater than `maxDelay` of {}",
                waiter.min_delay(),
                waiter.max_delay()
            ),
        ));
    }
    let succeeds = waiter
        .acceptors()
        .iter()
        .any(|acceptor| acceptor.state() == AcceptorState::Success);
    if !succeeds {
        report(error(
            waiter.source_location(),
            format!("{context}: no acceptor has the `success` state"),
        ));
    }

    let output = Type::shape(operation.output_shape());
    for (index, acceptor) in waiter.acceptors().iter().enumerate() {
        let context = Context {
            acceptor: Some(index),
            ..context
        };
        match acceptor.matcher().kind() {
            MatcherKind::Output(matcher) => check_path(matcher, &output, context, report),
            MatcherKind::InputOutput(matcher) => {
                let root = Type::input_output(operation.input_shape(), operation.output_shape());
                check_path(matcher, &root, context, report);
            }
            MatcherKind::ErrorType(error_type) => {
                let mut errors = operation.direct_errors();
                if !errors.any(|id| id.as_str() == error_type || id.name() == error_type) {
                    let listed: Vec<_> = operation
                        .direct_errors()
                        .map(|id| format!("`{id}`"))
                        .collect();
                    let listed = match listed.is_empty() {
                        true => "lists no errors".to_owned(),
                        false => format!("lists {}", listed.join(", ")),
                    };
                    report(warning(
                        acceptor.matcher().source_location(),
                        format!(
                            "{context}: `errorType` `{error_type}` is not an error of the \
                             operation, which {listed}"
                        ),
                    ));
                }
            }
            MatcherKind::Success(_) => {}
        }

        let (Some(message), Some(location)) =
            (acceptor.message(), acceptor.message_source_location())
        else {
            continue;
        };
        if acceptor.state() != AcceptorState::Failure {
            report(error(
                location,
                format!(
                    "{context}: `message` is only allowed on acceptors with the `failure` \
                     state, but the state is `{}`",
                    acceptor.state().as_str()
                ),
            ));
            continue;
        }
        let Some(analysis) = analyze("message", message, &output, location, context, report) else {
            continue;
        };
        if let Some(actual) = analysis.result.mismatch(Expected::StringOrStringArray) {
            report(error(
                location,
                format!(
                    "{context}: `message` must evaluate to {}, but evaluates to {actual}",
                    Expected::StringOrStringArray
                ),
            ));
        }
    }
}

fn check_path(
    matcher: &PathMatcher,
    root: &Type<'_>,
    context: Context<'_>,
    report: &mut impl FnMut(Diagnostic),
) {
    let comparator = matcher.comparator();
    if comparator == PathComparator::BooleanEquals
        && !matches!(matcher.expected(), "true" | "false")
    {
        report(error(
            matcher.source_location(),
            format!(
                "{context}: a `booleanEquals` matcher must expect `true` or `false`, but \
                 expects `{}`",
                matcher.expected()
            ),
        ));
    }
    let location = matcher.path_source_location();
    let Some(analysis) = analyze("path", matcher.path(), root, location, context, report) else {
        return;
    };
    let expected = match comparator {
        PathComparator::StringEquals => Expected::String,
        PathComparator::BooleanEquals => Expected::Boolean,
        PathComparator::AllStringEquals | PathComparator::AnyStringEquals => Expected::StringArray,
    };
    if let Some(actual) = analysis.result.mismatch(expected) {
        let name = comparator.as_str();
        let article = if name.starts_with('a') { "an" } else { "a" };
        report(warning(
            location,
            format!(
                "{context}: {article} `{name}` matcher compares {expected}, but `path` \
                 evaluates to {actual}"
            ),
        ));
    }
}

/// Parses and checks `expression`, reporting its problems at `location`. Returns `None` when
/// it does not parse.
fn analyze<'m>(
    what: &str,
    expression: &str,
    root: &Type<'m>,
    location: &SourceLocation,
    context: Context<'_>,
    report: &mut impl FnMut(Diagnostic),
) -> Option<Analysis<'m>> {
    match expression::analyze(expression, root) {
        Ok(analysis) => {
            for problem in &analysis.problems {
                let mut message = format!("{context}: `{what}` {expression:?} ");
                let (line, column) = expression::position(expression, problem.offset);
                expression::write_position(&mut message, line, column)
                    .expect("writing to a string cannot fail");
                let _ = write!(message, ": {}", problem.message);
                report(
                    Diagnostic::new(problem.severity, DiagnosticCode::InvalidTrait, message)
                        .with_source_location(location.clone()),
                );
            }
            Some(analysis)
        }
        Err(err) => {
            let (code, message) = match &err {
                ParseError::TooLong(_) | ParseError::TooNested(_) => (
                    DiagnosticCode::ResourceLimit,
                    format!("{context}: `{what}` {err}"),
                ),
                ParseError::Syntax { .. } => (
                    DiagnosticCode::InvalidTrait,
                    format!("{context}: `{what}` {expression:?} {err}"),
                ),
            };
            report(
                Diagnostic::new(Severity::Error, code, message)
                    .with_source_location(location.clone()),
            );
            None
        }
    }
}

/// Reports waiters whose names differ only by case in the closure of a service.
fn check_unique_names(
    model: &Model,
    decoded: &HashMap<&ShapeId, Waitable>,
    sink: &mut DiagnosticSink<'_>,
) {
    for service in model.services() {
        // Waiters by lowercase name, in the order the service's operations are listed.
        let mut names: IndexMap<String, Vec<(&ShapeId, &str, &Waiter)>> = IndexMap::new();
        for operation in service.contained_operations() {
            let Some(waitable) = decoded.get(operation.id()) else {
                continue;
            };
            for (name, waiter) in waitable.waiters() {
                names.entry(name.to_ascii_lowercase()).or_default().push((
                    operation.id(),
                    name,
                    waiter,
                ));
            }
        }
        for conflicting in names.values().filter(|waiters| waiters.len() > 1) {
            for (index, (operation, name, waiter)) in conflicting.iter().enumerate() {
                let others = conflicting
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, (operation, name, _))| format!("`{name}` of `{operation}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut diagnostic = error(
                    waiter.source_location(),
                    format!(
                        "waiter `{name}` conflicts with {others}: waiter names must be \
                         case-insensitively unique in the closure of service `{}`",
                        service.id()
                    ),
                )
                .with_shape_id((*operation).clone());
                for (other, (_, _, waiter)) in conflicting.iter().enumerate() {
                    if other != index {
                        diagnostic = diagnostic.with_related(waiter.source_location().clone());
                    }
                }
                sink.push(diagnostic);
            }
        }
    }
}

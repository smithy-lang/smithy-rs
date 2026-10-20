/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static validation of endpoint rules and the traits that bind their parameters.
//!
//! Decoding ([`codec`](super::codec)) and the graph ([`bdd`](super::bdd)) already guarantee
//! structure. This module checks meaning that needs the whole rule set or model: name scoping,
//! URL schemes, and agreement between the rules and their context-binding traits. Nothing here
//! evaluates an expression.

use super::codec::RULE_SET_MEMBERS;
use super::context::{
    ClientContextParams, ContextParam, OperationContextParams, StaticContextParams, CONTEXT_PARAM,
};
use super::language::*;
use super::{EndpointBdd, EndpointRuleSet, ENDPOINT_BDD, ENDPOINT_RULE_SET};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceLocation};
use crate::model::Model;
use crate::node::Node;
use crate::shape::ServiceView;
use crate::shape::ShapeType;
use crate::traits::{TraitCodec, TraitDecodeError};
use crate::validation::{DiagnosticSink, Validator};
use std::collections::HashMap;

/// The root members of a `smithy.rules#endpointBdd` trait value.
const ROOT_MEMBERS: [&str; 7] = [
    "version",
    "parameters",
    "conditions",
    "results",
    "root",
    "nodeCount",
    "nodes",
];

impl EndpointBdd {
    /// Checks rules that decoding does not, returning any problems found.
    ///
    /// Errors:
    ///
    /// - two conditions assign the same name, or an assignment reuses a parameter name;
    /// - a condition references a name that is neither a parameter nor assigned by an
    ///   **earlier** condition (a BDD tests conditions in index order, so a later assignment
    ///   can never be bound when the reference is evaluated);
    /// - a result references a name that is neither a parameter nor an assignment; or
    /// - a literal endpoint URL does not start with `http://` or `https://`.
    ///
    /// Warnings:
    ///
    /// - a parameter has no documentation.
    ///
    /// Functions are not checked: whether a function exists, and its signature, is defined
    /// by the client that executes the rules.
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = undocumented_parameters(self.parameters());

        // The index of the condition assigning each name.
        let mut assigned: HashMap<&str, usize> = HashMap::new();
        for (index, condition) in self.conditions().iter().enumerate() {
            let Some(name) = condition.assign() else {
                continue;
            };
            if self.parameters().get(name).is_some() {
                diagnostics.push(error(
                    condition.source_location(),
                    format!("condition {index} assigns `{name}`, which is a parameter name"),
                ));
            } else if let Some(first) = assigned.get(name) {
                diagnostics.push(error(
                    condition.source_location(),
                    format!(
                        "condition {index} assigns `{name}`, already assigned by condition {first}"
                    ),
                ));
            } else {
                assigned.insert(name, index);
            }
        }

        for (index, condition) in self.conditions().iter().enumerate() {
            visit_expression(condition.function(), &mut |name, location| {
                if self.parameters().get(name).is_some() {
                    return;
                }
                let message = match assigned.get(name) {
                    Some(at) if *at < index => return,
                    Some(at) => format!(
                        "condition {index} references `{name}`, which is assigned by condition {at} and is never bound when condition {index} is tested"
                    ),
                    None => format!(
                        "condition {index} references `{name}`, which is not a parameter or an earlier assignment"
                    ),
                };
                diagnostics.push(error(location, message));
            });
        }

        for (index, result) in self.results().iter().enumerate() {
            let in_scope =
                |name: &str| self.parameters().get(name).is_some() || assigned.contains_key(name);
            let mut check = |name: &str, location: &SourceLocation| {
                if !in_scope(name) {
                    diagnostics.push(error(
                        location,
                        format!(
                            "result {index} references `{name}`, which is not a parameter or an assignment"
                        ),
                    ));
                }
            };
            match result.kind() {
                ResultKind::Endpoint(endpoint) => {
                    visit_endpoint(endpoint, &mut check);
                    diagnostics.extend(invalid_url_scheme(endpoint));
                }
                ResultKind::Error(message) => visit_expression(message, &mut check),
            }
        }
        diagnostics
    }
}

impl EndpointRuleSet {
    /// Checks rules that decoding does not, returning any problems found.
    ///
    /// Errors:
    ///
    /// - a condition assigns a parameter name or a name already assigned in its scope (an
    ///   earlier condition of the same rule or of an enclosing tree rule), as in the reference
    ///   implementation's type checker; sibling rules may reuse a name;
    /// - a condition, endpoint, or error references a name that is neither a parameter nor
    ///   assigned in its scope; or
    /// - a literal endpoint URL does not start with `http://` or `https://`.
    ///
    /// Warnings:
    ///
    /// - a parameter has no documentation.
    ///
    /// Functions are not checked. The rule tree is walked with an explicit stack.
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = undocumented_parameters(self.parameters());
        diagnostics.extend(self.check_rules(true));
        diagnostics
    }

    /// The errors for names assigned or referenced out of scope, as Java's type checker
    /// reports them.
    pub(crate) fn scope_errors(&self) -> Vec<Diagnostic> {
        self.check_rules(false)
    }

    fn check_rules(&self, check_urls: bool) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let parameters = self.parameters();
        // Assignments in scope, innermost last, with the conditions that made them.
        let mut scope: Vec<(&str, &Condition)> = Vec::new();
        // Each rule with the scope length at which its conditions start.
        let mut stack: Vec<(&Rule, usize)> = self.rules().iter().rev().map(|r| (r, 0)).collect();
        while let Some((rule, base)) = stack.pop() {
            scope.truncate(base);
            for condition in rule.conditions() {
                visit_expression(condition.function(), &mut |name, location| {
                    if parameters.get(name).is_none() && !scope.iter().any(|(n, _)| *n == name) {
                        diagnostics.push(error(
                            location,
                            format!(
                                "condition references `{name}`, which is not a parameter or an assignment in scope"
                            ),
                        ));
                    }
                });
                let Some(name) = condition.assign() else {
                    continue;
                };
                if let Some(parameter) = parameters.get(name) {
                    diagnostics.push(
                        error(
                            condition.source_location(),
                            format!("condition assigns `{name}`, which is a parameter name"),
                        )
                        .with_related(parameter.source_location().clone()),
                    );
                } else if let Some((_, first)) = scope.iter().find(|(n, _)| *n == name) {
                    diagnostics.push(
                        error(
                            condition.source_location(),
                            format!(
                                "condition assigns `{name}`, which is already assigned in scope"
                            ),
                        )
                        .with_related(first.source_location().clone()),
                    );
                } else {
                    scope.push((name, condition));
                }
            }
            let mut check = |name: &str, location: &SourceLocation| {
                if parameters.get(name).is_none() && !scope.iter().any(|(n, _)| *n == name) {
                    diagnostics.push(error(
                        location,
                        format!(
                            "rule references `{name}`, which is not a parameter or an assignment in scope"
                        ),
                    ));
                }
            };
            match rule.kind() {
                RuleKind::Endpoint(endpoint) => {
                    visit_endpoint(endpoint, &mut check);
                    if check_urls {
                        diagnostics.extend(invalid_url_scheme(endpoint));
                    }
                }
                RuleKind::Error(message) => visit_expression(message, &mut check),
                RuleKind::Tree(children) => {
                    let base = scope.len();
                    stack.extend(children.iter().rev().map(|child| (child, base)));
                }
            }
        }
        diagnostics
    }
}

fn error(location: &SourceLocation, message: String) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::InvalidTrait, message).with_source_location(location.clone())
}

/// A warning for each parameter without documentation.
fn undocumented_parameters(parameters: &Parameters) -> Vec<Diagnostic> {
    parameters
        .iter()
        .filter(|parameter| parameter.documentation().is_none())
        .map(|parameter| {
            Diagnostic::warning(
                DiagnosticCode::MissingProperty,
                format!(
                    "endpoint parameter `{}` has no documentation",
                    parameter.name()
                ),
            )
            .with_source_location(parameter.source_location().clone())
        })
        .collect()
}

/// Calls `visit` for every name referenced by an endpoint's URL, headers, and properties.
fn visit_endpoint(endpoint: &EndpointDefinition, visit: &mut impl FnMut(&str, &SourceLocation)) {
    visit_expression(endpoint.url(), visit);
    for (_, values) in endpoint.headers() {
        for value in values {
            visit_expression(value, visit);
        }
    }
    for (_, value) in endpoint.properties() {
        visit_literal(value, endpoint.source_location(), visit);
    }
}

/// Calls `visit` for every referenced name, including template references.
fn visit_expression(expression: &Expression, visit: &mut impl FnMut(&str, &SourceLocation)) {
    match expression.kind() {
        ExpressionKind::Reference(name) => visit(name, expression.source_location()),
        ExpressionKind::Literal(literal) => {
            visit_literal(literal, expression.source_location(), visit)
        }
        ExpressionKind::Function { arguments, .. } => {
            for argument in arguments.iter() {
                visit_expression(argument, visit);
            }
        }
    }
}

/// Literals record no locations of their own, so references within them are reported at
/// `location`, the enclosing expression or endpoint.
fn visit_literal(
    literal: &Literal,
    location: &SourceLocation,
    visit: &mut impl FnMut(&str, &SourceLocation),
) {
    match literal {
        Literal::String(template) => {
            for segment in template.segments() {
                if let TemplateSegment::Reference { name, .. } = segment {
                    visit(name, location);
                }
            }
        }
        Literal::Record(members) => {
            for (_, value) in members.iter() {
                visit_literal(value, location, visit);
            }
        }
        Literal::Tuple(items) => {
            for item in items.iter() {
                visit_literal(item, location, visit);
            }
        }
        Literal::Boolean(_) | Literal::Integer(_) => {}
    }
}

/// An error when a literal URL's leading text does not start with an HTTP scheme.
fn invalid_url_scheme(endpoint: &EndpointDefinition) -> Option<Diagnostic> {
    let url = endpoint.url();
    let ExpressionKind::Literal(Literal::String(template)) = url.kind() else {
        return None;
    };
    match template.segments().first() {
        Some(TemplateSegment::Literal(text))
            if !(text.starts_with("http://") || text.starts_with("https://")) =>
        {
            Some(error(
                url.source_location(),
                format!(
                    "endpoint URL must start with `http://` or `https://`, but starts with `{text}`"
                ),
            ))
        }
        _ => None,
    }
}

/// A [`Validator`] for services' endpoint rules and the traits that bind their parameters.
///
/// For every service with `smithy.rules#endpointBdd` or `smithy.rules#endpointRuleSet`, it
/// reports:
///
/// - a malformed `endpointBdd`, `endpointRuleSet`, `clientContextParams`,
///   `staticContextParams`, `operationContextParams`, or `contextParam` trait value;
/// - a warning for each unknown `endpointBdd` or `endpointRuleSet` root member, which
///   decoding ignores;
/// - every problem found by [`EndpointBdd::validate`] and [`EndpointRuleSet::validate`];
/// - a binding for a parameter that the rules do not define;
/// - a binding whose type differs from the parameter's type (`operationContextParams` paths
///   are not type-checked, because this crate does not interpret JMESPath);
/// - a `contextParam` on a member that does not target a string, enum, or boolean shape;
/// - a parameter that is neither built-in nor bound by any trait; and
/// - an operation that does not bind a parameter that is required, has no default, and is
///   not built-in.
///
/// Bindings are checked for the operations a service contains, against the parameters of
/// `endpointBdd` when it decodes and of `endpointRuleSet` otherwise, so a service with both
/// is checked once. The validator is not run by default:
///
/// ```
/// use aws_smithy_lang::{endpoint_rules, Model};
///
/// let model = Model::assembler()
///     .add_validator(endpoint_rules::validator())
///     .assemble()
///     .into_result()?;
/// # Ok::<_, aws_smithy_lang::LoadError>(())
/// ```
pub fn validator() -> impl Validator {
    EndpointRulesValidator
}

struct EndpointRulesValidator;

impl Validator for EndpointRulesValidator {
    fn name(&self) -> &str {
        "EndpointRules"
    }

    fn validate(&self, model: &Model, sink: &mut DiagnosticSink<'_>) {
        for service in model.services() {
            let shape = service.as_shape();
            let id = shape.id();
            let mut report =
                |diagnostic: Diagnostic| sink.push(diagnostic.with_shape_id(id.clone()));

            let bdd = shape.get_trait(&ENDPOINT_BDD).and_then(|value| {
                unknown_members(value, "endpointBdd", &ROOT_MEMBERS, &mut report);
                let rules = EndpointBdd::from_node(value)
                    .map_err(|err| report(decode_error::<EndpointBdd>(err)))
                    .ok()?;
                rules.validate().into_iter().for_each(&mut report);
                Some(rules)
            });
            let rule_set = shape.get_trait(&ENDPOINT_RULE_SET).and_then(|value| {
                unknown_members(value, "endpointRuleSet", &RULE_SET_MEMBERS, &mut report);
                let rules = EndpointRuleSet::from_node(value)
                    .map_err(|err| report(decode_error::<EndpointRuleSet>(err)))
                    .ok()?;
                rules.validate().into_iter().for_each(&mut report);
                Some(rules)
            });
            let parameters = match (&bdd, &rule_set) {
                (Some(bdd), _) => bdd.parameters(),
                (None, Some(rule_set)) => rule_set.parameters(),
                (None, None) => continue,
            };
            check_bindings(service, parameters, &mut report);
        }
    }
}

/// Warns about each member of the object `value` that is not in `known`.
fn unknown_members(value: &Node, what: &str, known: &[&str], report: &mut impl FnMut(Diagnostic)) {
    let Some(members) = value.as_object() else {
        return;
    };
    for (key, member) in members.iter() {
        if !known.contains(&key) {
            report(
                Diagnostic::warning(
                    DiagnosticCode::UnknownProperty,
                    format!("unknown {what} member `{key}` is ignored"),
                )
                .with_source_location(member.source_location().clone()),
            );
        }
    }
}

/// Checks the service's and its operations' context-binding traits against `parameters`.
fn check_bindings(
    service: ServiceView<'_>,
    parameters: &Parameters,
    report: &mut impl FnMut(Diagnostic),
) {
    let shape = service.as_shape();
    let mut bindings = Bindings::new(parameters);
    // Parameters bound for every operation.
    let mut service_bound = Vec::new();
    match shape.get_trait_as::<ClientContextParams>() {
        Ok(params) => {
            for param in params.iter().flat_map(|p| p.iter()) {
                bindings.bind(param.name(), Some(param.kind()), param.source_location());
                service_bound.push(param.name().to_owned());
            }
        }
        Err(err) => report(decode_error::<ClientContextParams>(err)),
    }

    for operation in service.contained_operations() {
        let operation_shape = operation.as_shape();
        let mut bound = service_bound.clone();
        match operation_shape.get_trait_as::<StaticContextParams>() {
            Ok(params) => {
                for param in params.iter().flat_map(|p| p.iter()) {
                    bindings.bind(
                        param.name(),
                        Some(param.value().kind()),
                        param.source_location(),
                    );
                    bound.push(param.name().to_owned());
                }
            }
            Err(err) => report(decode_error::<StaticContextParams>(err)),
        }
        match operation_shape.get_trait_as::<OperationContextParams>() {
            Ok(params) => {
                for param in params.iter().flat_map(|p| p.iter()) {
                    bindings.bind(param.name(), None, param.source_location());
                    bound.push(param.name().to_owned());
                }
            }
            Err(err) => report(decode_error::<OperationContextParams>(err)),
        }
        for member in operation.input_shape().members().iter() {
            let param = match member.get_trait_as::<ContextParam>() {
                Ok(Some(param)) => param,
                Ok(None) => continue,
                Err(err) => {
                    report(decode_error::<ContextParam>(err));
                    continue;
                }
            };
            let location = member
                .get_trait(&CONTEXT_PARAM)
                .map_or(member.source_location(), Node::source_location);
            let kind = match member.target_shape().shape_type() {
                ShapeType::String | ShapeType::Enum => ParameterType::String,
                ShapeType::Boolean => ParameterType::Boolean,
                other => {
                    bindings.errors.push(error(
                        location,
                        format!(
                            "`contextParam` `{}` is applied to `{}`, which targets a {other} shape; only string, enum, and boolean targets are supported",
                            param.name(),
                            member.id()
                        ),
                    ));
                    continue;
                }
            };
            bindings.bind(param.name(), Some(kind), location);
            bound.push(param.name().to_owned());
        }

        for parameter in parameters.iter() {
            let must_bind = parameter.required()
                && parameter.default().is_none()
                && parameter.built_in().is_none();
            if must_bind && !bound.iter().any(|name| name == parameter.name()) {
                bindings.errors.push(error(
                    operation_shape.source_location(),
                    format!(
                        "operation `{}` does not bind required endpoint parameter `{}`",
                        operation_shape.id(),
                        parameter.name()
                    ),
                ));
            }
        }
    }

    for parameter in parameters.iter() {
        if parameter.built_in().is_none() && !bindings.any.contains_key(parameter.name()) {
            bindings.errors.push(error(
                parameter.source_location(),
                format!(
                    "endpoint parameter `{}` is neither built-in nor bound by a context parameter trait",
                    parameter.name()
                ),
            ));
        }
    }
    bindings.errors.into_iter().for_each(report);
}

fn decode_error<T: TraitCodec>(err: TraitDecodeError) -> Diagnostic {
    let location = err.source_location().clone();
    let message = match err.trait_id() {
        Some(_) => err.to_string(),
        None => format!("invalid `{}` trait value: {}", T::ID, err.message()),
    };
    Diagnostic::error(DiagnosticCode::InvalidTrait, message).with_source_location(location)
}

/// Collects bindings and checks each against the rules' parameters.
struct Bindings<'a> {
    parameters: &'a Parameters,
    /// Every parameter bound by any trait, mapped to the first binding's location.
    any: HashMap<String, SourceLocation>,
    errors: Vec<Diagnostic>,
}

impl<'a> Bindings<'a> {
    fn new(parameters: &'a Parameters) -> Self {
        Self {
            parameters,
            any: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// Records a binding of `name` with type `kind`, when known.
    fn bind(&mut self, name: &str, kind: Option<ParameterType>, location: &SourceLocation) {
        let first = !self.any.contains_key(name);
        self.any
            .entry(name.to_owned())
            .or_insert_with(|| location.clone());
        let Some(parameter) = self.parameters.get(name) else {
            if first {
                self.errors.push(
                    Diagnostic::error(
                        DiagnosticCode::InvalidTrait,
                        format!("`{name}` is bound by a context parameter trait but is not an endpoint parameter"),
                    )
                    .with_source_location(location.clone()),
                );
            }
            return;
        };
        if let Some(kind) = kind {
            if kind != parameter.kind() {
                self.errors.push(
                    Diagnostic::error(
                        DiagnosticCode::InvalidTrait,
                        format!(
                            "`{name}` is bound as a {} value, but the endpoint parameter is {}",
                            kind.as_str(),
                            parameter.kind().as_str()
                        ),
                    )
                    .with_source_location(location.clone())
                    .with_related(parameter.source_location().clone()),
                );
            }
        }
    }
}

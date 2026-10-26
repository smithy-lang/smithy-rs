/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Expression helpers shared by the compiler stages.

use crate::endpoint_rules::codec::{encode_path, encode_template};
use crate::endpoint_rules::language::{
    Condition, EndpointDefinition, Expression, ExpressionKind, Literal, Location, PathSegment,
    RuleKind, Template, TemplateSegment,
};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;

pub(super) fn reference(name: &str, location: &Location) -> Expression {
    Expression {
        kind: ExpressionKind::Reference(name.to_owned()),
        location: location.clone(),
    }
}

pub(super) fn literal(literal: Literal, location: &Location) -> Expression {
    Expression {
        kind: ExpressionKind::Literal(literal),
        location: location.clone(),
    }
}

pub(super) fn call(name: &str, arguments: Vec<Expression>, location: &Location) -> Expression {
    Expression {
        kind: ExpressionKind::Function {
            name: name.to_owned(),
            arguments: arguments.into_boxed_slice(),
        },
        location: location.clone(),
    }
}

/// The function name and arguments of a call.
pub(super) fn as_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    match &expression.kind {
        ExpressionKind::Function { name, arguments } => Some((name, arguments)),
        _ => None,
    }
}

pub(super) fn as_reference(expression: &Expression) -> Option<&str> {
    match &expression.kind {
        ExpressionKind::Reference(name) => Some(name),
        _ => None,
    }
}

/// Smithy Java's `toString()` of an expression, such as `stringEquals(Region, "aws-global")`.
///
/// Java keys several maps by this text and orders some choices by it, so the compiler does
/// the same to make the same choices.
pub(super) fn key(expression: &Expression) -> String {
    let mut out = String::new();
    write_expression(&mut out, expression);
    out
}

fn write_expression(out: &mut String, expression: &Expression) {
    match &expression.kind {
        ExpressionKind::Literal(literal) => write_literal(out, literal),
        ExpressionKind::Reference(name) => out.push_str(name),
        ExpressionKind::Function { name, arguments } => {
            // Java's `GetAttr` prints as `target#path`.
            if let ("getAttr", [target, path]) = (name.as_str(), &**arguments) {
                if let ExpressionKind::Literal(Literal::String(path)) = &path.kind {
                    if let Some(path) = path.as_static() {
                        write_expression(out, target);
                        out.push('#');
                        out.push_str(path);
                        return;
                    }
                }
            }
            out.push_str(name);
            out.push('(');
            for (i, argument) in arguments.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expression(out, argument);
            }
            out.push(')');
        }
    }
}

fn write_literal(out: &mut String, literal: &Literal) {
    match literal {
        Literal::Boolean(b) => write!(out, "{b}").unwrap(),
        Literal::Integer(i) => write!(out, "{i}").unwrap(),
        Literal::String(template) => write!(out, "\"{}\"", encode_template(template)).unwrap(),
        Literal::Record(members) => {
            out.push('{');
            for (i, (name, value)) in members.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push('=');
                write_literal(out, value);
            }
            out.push('}');
        }
        Literal::Tuple(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_literal(out, item);
            }
            out.push(']');
        }
    }
}

/// Adds the names `expression` references to `out`.
pub(super) fn references<'a>(expression: &'a Expression, out: &mut BTreeSet<&'a str>) {
    match &expression.kind {
        ExpressionKind::Literal(literal) => literal_references(literal, out),
        ExpressionKind::Reference(name) => {
            out.insert(name);
        }
        ExpressionKind::Function { arguments, .. } => {
            for argument in arguments.iter() {
                references(argument, out);
            }
        }
    }
}

pub(super) fn literal_references<'a>(literal: &'a Literal, out: &mut BTreeSet<&'a str>) {
    match literal {
        Literal::String(template) => {
            for segment in template.segments.iter() {
                if let TemplateSegment::Reference { name, .. } = segment {
                    out.insert(name);
                }
            }
        }
        Literal::Record(members) => members
            .iter()
            .for_each(|(_, value)| literal_references(value, out)),
        Literal::Tuple(items) => items.iter().for_each(|item| literal_references(item, out)),
        Literal::Boolean(_) | Literal::Integer(_) => {}
    }
}

/// The set of names an expression references.
pub(super) fn reference_set(expression: &Expression) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    references(expression, &mut out);
    out
}

/// Counts every occurrence of every name `expression` references.
pub(super) fn count_references(expression: &Expression, counts: &mut HashMap<String, usize>) {
    match &expression.kind {
        ExpressionKind::Literal(literal) => count_literal_references(literal, counts),
        ExpressionKind::Reference(name) => *counts.entry(name.clone()).or_default() += 1,
        ExpressionKind::Function { arguments, .. } => {
            for argument in arguments.iter() {
                count_references(argument, counts);
            }
        }
    }
}

fn count_literal_references(literal: &Literal, counts: &mut HashMap<String, usize>) {
    match literal {
        Literal::String(template) => {
            for segment in template.segments.iter() {
                if let TemplateSegment::Reference { name, .. } = segment {
                    *counts.entry(name.clone()).or_default() += 1;
                }
            }
        }
        Literal::Record(members) => members
            .iter()
            .for_each(|(_, value)| count_literal_references(value, counts)),
        Literal::Tuple(items) => items
            .iter()
            .for_each(|item| count_literal_references(item, counts)),
        Literal::Boolean(_) | Literal::Integer(_) => {}
    }
}

/// The expressions and literals of a rule's endpoint or error, which conditions do not cover.
pub(super) enum Value<'a> {
    Expression(&'a Expression),
    Literal(&'a Literal),
}

/// The values of an endpoint or error result, in document order. Tree rules have none.
pub(super) fn result_values(kind: &RuleKind) -> Vec<Value<'_>> {
    match kind {
        RuleKind::Endpoint(endpoint) => {
            let mut values = vec![Value::Expression(&endpoint.url)];
            for (_, header) in endpoint.headers.iter() {
                values.extend(header.iter().map(Value::Expression));
            }
            values.extend(endpoint.properties.iter().map(|(_, v)| Value::Literal(v)));
            values
        }
        RuleKind::Error(error) => vec![Value::Expression(error)],
        RuleKind::Tree(_) => Vec::new(),
    }
}

impl<'a> Value<'a> {
    pub(super) fn references(&self, out: &mut BTreeSet<&'a str>) {
        match self {
            Value::Expression(expression) => references(expression, out),
            Value::Literal(literal) => literal_references(literal, out),
        }
    }

    pub(super) fn count_references(&self, counts: &mut HashMap<String, usize>) {
        match self {
            Value::Expression(expression) => count_references(expression, counts),
            Value::Literal(literal) => count_literal_references(literal, counts),
        }
    }
}

/// Renames the references for which `rename` returns a new name.
pub(super) fn rename(
    expression: &Expression,
    rename: &impl Fn(&str) -> Option<String>,
) -> Expression {
    let kind = match &expression.kind {
        ExpressionKind::Literal(literal) => {
            ExpressionKind::Literal(rename_literal(literal, rename))
        }
        ExpressionKind::Reference(name) => {
            ExpressionKind::Reference(rename(name).unwrap_or_else(|| name.clone()))
        }
        ExpressionKind::Function { name, arguments } => ExpressionKind::Function {
            name: name.clone(),
            arguments: arguments.iter().map(|a| self::rename(a, rename)).collect(),
        },
    };
    Expression {
        kind,
        location: expression.location.clone(),
    }
}

pub(super) fn rename_literal(
    literal: &Literal,
    rename: &impl Fn(&str) -> Option<String>,
) -> Literal {
    match literal {
        Literal::String(template) => Literal::String(Template {
            segments: template
                .segments
                .iter()
                .map(|segment| match segment {
                    TemplateSegment::Reference { name, path } => TemplateSegment::Reference {
                        name: rename(name).unwrap_or_else(|| name.clone()),
                        path: path.clone(),
                    },
                    literal => literal.clone(),
                })
                .collect(),
        }),
        Literal::Record(members) => Literal::Record(
            members
                .iter()
                .map(|(name, value)| (name.clone(), rename_literal(value, rename)))
                .collect(),
        ),
        Literal::Tuple(items) => {
            Literal::Tuple(items.iter().map(|i| rename_literal(i, rename)).collect())
        }
        Literal::Boolean(_) | Literal::Integer(_) => literal.clone(),
    }
}

/// Renames the references in an endpoint or error; tree rules are returned unchanged.
pub(super) fn rename_result(kind: &RuleKind, rename: &impl Fn(&str) -> Option<String>) -> RuleKind {
    match kind {
        RuleKind::Endpoint(endpoint) => RuleKind::Endpoint(EndpointDefinition {
            url: self::rename(&endpoint.url, rename),
            headers: endpoint
                .headers
                .iter()
                .map(|(name, values)| {
                    let values = values.iter().map(|v| self::rename(v, rename)).collect();
                    (name.clone(), values)
                })
                .collect(),
            properties: endpoint
                .properties
                .iter()
                .map(|(name, value)| (name.clone(), rename_literal(value, rename)))
                .collect(),
            location: endpoint.location.clone(),
        }),
        RuleKind::Error(error) => RuleKind::Error(self::rename(error, rename)),
        RuleKind::Tree(_) => kind.clone(),
    }
}

/// A condition with a new function and assignment, keeping its location.
pub(super) fn with_function(
    condition: &Condition,
    function: Expression,
    assign: Option<String>,
) -> Condition {
    Condition {
        function,
        assign,
        location: condition.location.clone(),
    }
}

/// Smithy Java's `LibraryFunction.canonicalize()`: orders the arguments of `booleanEquals`
/// and `stringEquals`, and unwraps `stringEquals` arguments that are a lone template
/// reference, such as `"{Region}"`.
pub(super) fn canonicalize(function: &Expression) -> Expression {
    let Some((name, arguments)) = as_call(function) else {
        return function.clone();
    };
    let (a, b) = match (name, arguments) {
        ("booleanEquals", [a, b]) => (a.clone(), b.clone()),
        ("stringEquals", [a, b]) => (strip_template(a), strip_template(b)),
        _ => return function.clone(),
    };
    let arguments = if should_swap(&a, &b) {
        vec![b, a]
    } else {
        vec![a, b]
    };
    call(name, arguments, &function.location)
}

/// `"{x}"` becomes `x`, and `"{x#a.b}"` becomes `getAttr(x, "a.b")`.
fn strip_template(expression: &Expression) -> Expression {
    if let ExpressionKind::Literal(Literal::String(template)) = &expression.kind {
        if let [TemplateSegment::Reference { name, path }] = &*template.segments {
            let location = &expression.location;
            let target = reference(name, location);
            if path.is_empty() {
                return target;
            }
            return get_attr(target, path, location);
        }
    }
    expression.clone()
}

pub(super) fn get_attr(
    target: Expression,
    path: &[PathSegment],
    location: &Location,
) -> Expression {
    let path = Template {
        segments: Box::new([TemplateSegment::Literal(encode_path(path))]),
    };
    call(
        "getAttr",
        vec![target, literal(Literal::String(path), location)],
        location,
    )
}

/// Whether to swap commutative arguments: expressions go before literals, and otherwise
/// arguments are in Java string order.
fn should_swap(a: &Expression, b: &Expression) -> bool {
    let (a_literal, b_literal) = (is_static_literal(a), is_static_literal(b));
    if a_literal != b_literal {
        return a_literal;
    }
    key(a) > key(b)
}

fn is_static_literal(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::Literal(Literal::String(template)) => template.as_static().is_some(),
        ExpressionKind::Literal(_) => true,
        _ => false,
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Parsing and static analysis of waiter JMESPath expressions.
//!
//! Expressions are parsed with the `jmespath` crate. That AST stays private to this module.
//! The checker infers a conservative JMESPath type for an expression evaluated against a
//! modeled shape. When the type of a value cannot be known (document, an object whose
//! fields are unknown, the result of joining different types), it is marked [`Type::Any`], and
//! nothing that depends on it is reported.
//!
//! Errors are problems that do not depend on the inferred type (an unknown function, a call
//! with the wrong number of arguments) and function arguments of the wrong type where the
//! reference implementation is certain to find the wrong type too. Everything else that
//! depends on the inferred type is a warning. The reference implementation instead lints by
//! evaluating the expression against sample data built from the model, and reports some of the
//! same problems as errors; this checker reports no error that the reference implementation's
//! linter does not.

use crate::diagnostic::Severity;
use crate::shape::{ShapeType, ShapeView};
use crate::shape_id::ShapeId;
use jmespath::ast::{Ast, Comparator};
use jmespath::{ErrorReason, Variable};
use std::fmt;

const LENGTH: ShapeId = ShapeId::from_static("smithy.api#length");

/// The longest expression the validator parses, in bytes. The longest in the checked-in AWS
/// models is about 200 bytes so this leaves an order of magnitude of wiggle room.
pub(super) const MAX_LENGTH: usize = 2048;

/// The deepest an expression may nest, as estimated by [`nesting_depth`].
///
/// The `jmespath` parser is recursive and does not limit its depth, so the validator bounds
/// the nesting before parsing. Between two levels the parser's recursion can only deepen by a
/// small constant (one call per binding power), and the checker's recursion follows the parse.
/// 32 leaves room on a 2 MiB thread for the worst case in an unoptimized build.
// TODO(jmespath#N): delete `nesting_depth` once a `jmespath` release limits its own recursion.
pub(super) const MAX_NESTING: usize = 32;

/// Why an expression could not be parsed.
#[derive(Debug)]
pub(super) enum ParseError {
    /// The expression is longer than [`MAX_LENGTH`].
    TooLong(usize),
    /// The expression nests deeper than [`MAX_NESTING`].
    TooNested(usize),
    /// The expression is not valid JMESPath.
    Syntax {
        line: usize,
        column: usize,
        reason: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::TooLong(length) => write!(
                f,
                "is {length} bytes long, longer than the {MAX_LENGTH} bytes this validator parses"
            ),
            ParseError::TooNested(depth) => write!(
                f,
                "nests {depth} levels deep (counting brackets, parentheses, braces, \
                 projections, `!`, and `&`), deeper than the {MAX_NESTING} this validator parses"
            ),
            ParseError::Syntax {
                line,
                column,
                reason,
            } => {
                write!(f, "is not valid JMESPath ")?;
                write_position(f, *line, *column)?;
                write!(f, ": {reason}")
            }
        }
    }
}

/// A problem found by the checker, at a byte offset in the expression.
#[derive(Debug)]
pub(super) struct Problem {
    pub(super) severity: Severity,
    pub(super) offset: usize,
    pub(super) message: String,
}

/// The result of checking an expression.
#[derive(Debug)]
pub(super) struct Analysis<'m> {
    /// The inferred type of the expression's result.
    pub(super) result: Type<'m>,
    pub(super) problems: Vec<Problem>,
}

/// Parses `expression` and checks it against `root`, the type of the value it is evaluated
/// against.
pub(super) fn analyze<'m>(expression: &str, root: &Type<'m>) -> Result<Analysis<'m>, ParseError> {
    let ast = parse(expression)?;
    let mut checker = Checker {
        problems: Vec::new(),
        at_root: true,
    };
    let result = checker.check(&ast, root);
    Ok(Analysis {
        result,
        problems: checker.problems,
    })
}

fn parse(expression: &str) -> Result<Ast, ParseError> {
    if expression.len() > MAX_LENGTH {
        return Err(ParseError::TooLong(expression.len()));
    }
    let depth = nesting_depth(expression);
    if depth > MAX_NESTING {
        return Err(ParseError::TooNested(depth));
    }
    jmespath::parse(expression).map_err(|error| ParseError::Syntax {
        // The parser counts lines and columns from 0.
        line: error.line + 1,
        column: error.column + 1,
        reason: match error.reason {
            ErrorReason::Parse(reason) => reason,
            ErrorReason::Runtime(reason) => reason.to_string(),
        },
    })
}

/// Estimates, from above, how deeply the `jmespath` parser nests when it parses `expression`.
///
/// Tokens are read as the `jmespath` lexer reads them, so the contents of `'...'`, `"..."`,
/// and `` `...` `` are skipped. A level is open:
///
/// - inside a parenthesis, a brace, a multi-select or function's brackets, or a filter's
///   condition, until the closing token;
/// - after `!`, or a projection (`*`, `[*]`, `[]`, a slice, or the part after a filter's
///   `]`), until the enclosing level closes, a `,`, or an operator that binds more loosely
///   than a projection (`|`, `||`, `&&`, or a comparison);
/// - after `&`, until the enclosing level closes or a `,`. The referenced expression
///   absorbs every operator, so levels open before the `&` stay open as long.
///
/// A plain index such as `[0]` does not nest. The estimate is the most levels open at once.
/// It can be larger than the parser's real nesting, but not smaller: where the parser returns
/// from a level at a different token, it is at a token where the estimate keeps the level
/// open, or at a parse error, after which the parser reads nothing more.
fn nesting_depth(expression: &str) -> usize {
    #[derive(Default)]
    struct Level {
        /// Whether this is a filter's condition, which a projection follows.
        filter: bool,
        /// Levels that close at a loosely binding operator.
        open: usize,
        /// Levels that close only at a `,` or the end of this level.
        pinned: usize,
    }

    let bytes = expression.as_bytes();
    // The expression itself is a level that the depth does not count.
    let mut levels = vec![Level::default()];
    let (mut depth, mut deepest) = (0, 0);
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        let next = bytes.get(index).copied();
        let nested = levels.len() > 1;
        let level = levels
            .last_mut()
            .expect("the expression level is never closed");
        match byte {
            b'\'' | b'"' | b'`' => {
                // The text ends at the first delimiter that a backslash does not escape.
                while let Some(&inner) = bytes.get(index) {
                    index += match inner {
                        b'\\' => 2,
                        _ => 1,
                    };
                    if inner == byte {
                        break;
                    }
                }
            }
            b'(' | b'{' => {
                levels.push(Level::default());
                depth += 1;
            }
            b'[' => match next {
                Some(b']') => {
                    index += 1;
                    level.open += 1;
                    depth += 1;
                }
                Some(b'?') => {
                    index += 1;
                    levels.push(Level {
                        filter: true,
                        ..Level::default()
                    });
                    depth += 1;
                }
                _ => match index_or_projection(&bytes[index..]) {
                    Some((length, projection)) => {
                        index += length;
                        if projection {
                            level.open += 1;
                            depth += 1;
                        }
                    }
                    None => {
                        levels.push(Level::default());
                        depth += 1;
                    }
                },
            },
            // An unmatched closing token is a parse error.
            b')' | b'}' | b']' if nested => {
                let closed = levels.pop().expect("the level is nested");
                depth -= 1 + closed.open + closed.pinned;
                if closed.filter {
                    levels.last_mut().expect("the level was nested").open += 1;
                    depth += 1;
                }
            }
            b'!' | b'&' | b'|' | b'=' | b'<' | b'>' => match (byte, next) {
                (b'!' | b'=' | b'<' | b'>', Some(b'='))
                | (b'&', Some(b'&'))
                | (b'|', Some(b'|')) => {
                    index += 1;
                    depth -= level.open;
                    level.open = 0;
                }
                (b'!', _) => {
                    level.open += 1;
                    depth += 1;
                }
                (b'&', _) => {
                    level.pinned += level.open + 1;
                    level.open = 0;
                    depth += 1;
                }
                // `|`, `<`, `>`, or a lone `=`, which the lexer rejects.
                _ => {
                    depth -= level.open;
                    level.open = 0;
                }
            },
            b'*' => {
                level.open += 1;
                depth += 1;
            }
            b',' => {
                depth -= level.open + level.pinned;
                level.open = 0;
                level.pinned = 0;
            }
            _ => {}
        }
        deepest = deepest.max(depth);
    }
    deepest
}

/// When `rest`, the text after a `[`, starts with an index (`0]`), a slice (`1:-1]`), or a
/// wildcard (`*]`), returns its length through the `]` and whether it is a projection: a
/// slice or a wildcard. Whitespace may separate the tokens, as the lexer allows.
fn index_or_projection(rest: &[u8]) -> Option<(usize, bool)> {
    let is_space = |byte: &u8| matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
    let end = rest
        .iter()
        .position(|byte| !(byte.is_ascii_digit() || b"-:*".contains(byte) || is_space(byte)))?;
    if rest[end] != b']' {
        return None;
    }
    let body = &rest[..end];
    let mut tokens = body.iter().filter(|byte| !is_space(byte));
    let projection = if body.contains(&b'*') {
        // Only a lone `*` is a wildcard index.
        if tokens.next() != Some(&b'*') || tokens.next().is_some() {
            return None;
        }
        true
    } else {
        tokens.next()?;
        body.contains(&b':')
    };
    Some((end + 1, projection))
}

/// The 1-based line and column, in characters, of byte `offset` in `expression`.
pub(super) fn position(expression: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut column) = (1, 1);
    for (index, c) in expression.char_indices() {
        if index >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Writes `at column C`, or `at line L, column C` for a position after the first line.
pub(super) fn write_position(f: &mut impl fmt::Write, line: usize, column: usize) -> fmt::Result {
    if line == 1 {
        write!(f, "at column {column}")
    } else {
        write!(f, "at line {line}, column {column}")
    }
}

/// A conservative JMESPath type.
#[derive(Clone, Debug)]
pub(super) enum Type<'m> {
    /// Unknown: any value may occur. Nothing that depends on the value is reported.
    Any,
    Null,
    Boolean,
    Number,
    String,
    /// An array whose elements have the given type.
    Array(Box<Type<'m>>),
    Object(Object<'m>),
    /// An expression reference, `&expression`.
    Expref,
    /// A value of a modeled shape, expanded one level at a time by [`resolve`](Self::resolve)
    /// so that recursive shapes are never expanded completely.
    Shape(ShapeView<'m>),
}

/// The fields of an object.
#[derive(Clone, Debug)]
pub(super) enum Object<'m> {
    /// A structure or union: the fields are its members, by member name.
    Members(ShapeView<'m>),
    /// A map: every field has the value type.
    Map(Box<Type<'m>>),
    /// Exactly these fields, as for a multi-select hash or the `inputOutput` root.
    Fields(Vec<(String, Type<'m>)>),
    /// Unknown fields.
    Unknown,
}

impl<'m> Type<'m> {
    /// The value of a structure, as matched by an `output` matcher.
    pub(super) fn shape(shape: ShapeView<'m>) -> Self {
        Type::Shape(shape)
    }

    /// The value matched by an `inputOutput` matcher: an object with `input` and `output`.
    pub(super) fn input_output(input: ShapeView<'m>, output: ShapeView<'m>) -> Self {
        Type::Object(Object::Fields(vec![
            ("input".to_owned(), Type::Shape(input)),
            ("output".to_owned(), Type::Shape(output)),
        ]))
    }

    /// Expands a modeled shape by one level. Other types are returned as they are.
    pub(super) fn resolve(&self) -> Type<'m> {
        let Type::Shape(shape) = self else {
            return self.clone();
        };
        match shape.shape_type() {
            ShapeType::Blob | ShapeType::String | ShapeType::Enum => Type::String,
            ShapeType::Boolean => Type::Boolean,
            ShapeType::Byte
            | ShapeType::Short
            | ShapeType::Integer
            | ShapeType::Long
            | ShapeType::Float
            | ShapeType::Double
            | ShapeType::BigInteger
            | ShapeType::BigDecimal
            | ShapeType::Timestamp
            | ShapeType::IntEnum => Type::Number,
            ShapeType::List => match shape.as_list() {
                Some(list) => Type::Array(Box::new(Type::Shape(list.member().target_shape()))),
                None => Type::Any,
            },
            ShapeType::Map => match shape.as_map() {
                Some(map) => Type::Object(Object::Map(Box::new(Type::Shape(
                    map.value().target_shape(),
                )))),
                None => Type::Any,
            },
            ShapeType::Structure | ShapeType::Union => Type::Object(Object::Members(*shape)),
            // Documents, and shapes that are never values.
            _ => Type::Any,
        }
    }

    /// The JMESPath name of this type, after resolving a modeled shape.
    pub(super) fn name(&self) -> &'static str {
        match self.resolve() {
            Type::Any => "any",
            Type::Null => "null",
            Type::Boolean => "boolean",
            Type::Number => "number",
            Type::String => "string",
            Type::Array(_) => "array",
            Type::Object(_) => "object",
            Type::Expref => "expref",
            Type::Shape(_) => unreachable!("`resolve` expands shapes"),
        }
    }

    /// The name of this type with an article: ``a `string` `` or ``an `array` ``.
    fn described(&self) -> String {
        let name = self.name();
        let article = match name.as_bytes()[0] {
            b'a' | b'e' | b'o' => "an",
            _ => "a",
        };
        format!("{article} `{name}`")
    }

    /// Describes this type when it is known not to be `expected`, or returns `None`.
    pub(super) fn mismatch(&self, expected: Expected) -> Option<String> {
        let resolved = self.resolve();
        let string_array = |resolved: &Type<'m>| match resolved {
            Type::Any => None,
            Type::Array(element) => match element.resolve() {
                Type::Any | Type::String => None,
                other => Some(format!("an `array` of `{}` values", other.name())),
            },
            other => Some(other.described()),
        };
        match (expected, &resolved) {
            (_, Type::Any) => None,
            (Expected::String, Type::String) | (Expected::Boolean, Type::Boolean) => None,
            (Expected::String | Expected::Boolean, other) => Some(other.described()),
            (Expected::StringArray, resolved) => string_array(resolved),
            (Expected::StringOrStringArray, Type::String) => None,
            (Expected::StringOrStringArray, resolved) => string_array(resolved),
        }
    }

    /// The type of the elements of this array, or `Any`.
    fn element(&self) -> Type<'m> {
        match self.resolve() {
            Type::Array(element) => *element,
            _ => Type::Any,
        }
    }
}

/// A result type required of an expression.
#[derive(Clone, Copy, Debug)]
pub(super) enum Expected {
    String,
    Boolean,
    StringArray,
    StringOrStringArray,
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Expected::String => "a `string`",
            Expected::Boolean => "a `boolean`",
            Expected::StringArray => "an `array` of `string` values",
            Expected::StringOrStringArray => "a `string` or an `array` of `string` values",
        })
    }
}

/// How many levels of nested arrays [`join`] compares element by element. Deeper elements
/// are `Any`. A modeled list is expanded one level per comparison, so without a limit the
/// recursion would be as deep as the lists nest, and endless for lists that contain each
/// other. The other type walkers here do not recurse over the model: [`Type::resolve`],
/// [`Type::element`], [`Type::mismatch`], and `Param::rejects` look one level down, and
/// [`literal`] walks a JSON literal, which is at most [`MAX_LENGTH`] bytes.
const MAX_JOIN_DEPTH: usize = 32;

/// The type of a value that is either `a` or `b`.
fn join<'m>(a: Type<'m>, b: Type<'m>) -> Type<'m> {
    join_at(a, b, 0)
}

/// [`join`] at `depth` levels of arrays.
fn join_at<'m>(a: Type<'m>, b: Type<'m>, depth: usize) -> Type<'m> {
    match (a, b) {
        (Type::Shape(a), Type::Shape(b)) if a.id() == b.id() => Type::Shape(a),
        (a, b) => match (a.resolve(), b.resolve()) {
            (Type::Null, Type::Null) => Type::Null,
            (Type::Boolean, Type::Boolean) => Type::Boolean,
            (Type::Number, Type::Number) => Type::Number,
            (Type::String, Type::String) => Type::String,
            (Type::Expref, Type::Expref) => Type::Expref,
            (Type::Array(a), Type::Array(b)) => Type::Array(Box::new(if depth < MAX_JOIN_DEPTH {
                join_at(*a, *b, depth + 1)
            } else {
                Type::Any
            })),
            (Type::Object(Object::Members(a)), Type::Object(Object::Members(b)))
                if a.id() == b.id() =>
            {
                Type::Object(Object::Members(a))
            }
            (Type::Object(_), Type::Object(_)) => Type::Object(Object::Unknown),
            _ => Type::Any,
        },
    }
}

/// The type of a value that is one of `types`, or `Any` when there are none.
fn join_all<'m>(types: impl IntoIterator<Item = Type<'m>>) -> Type<'m> {
    types.into_iter().reduce(join).unwrap_or(Type::Any)
}

/// Projections omit null results, so a projection whose expression is always null produces
/// an empty array, whose elements are unknown.
fn projected(result: Type<'_>) -> Type<'_> {
    match result {
        Type::Null => Type::Any,
        other => other,
    }
}

fn literal<'m>(value: &Variable) -> Type<'m> {
    match value {
        Variable::Null => Type::Null,
        Variable::String(_) => Type::String,
        Variable::Bool(_) => Type::Boolean,
        Variable::Number(_) => Type::Number,
        Variable::Array(items) => {
            Type::Array(Box::new(join_all(items.iter().map(|item| literal(item)))))
        }
        Variable::Object(fields) => Type::Object(Object::Fields(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), literal(value)))
                .collect(),
        )),
        Variable::Expref(_) => Type::Expref,
    }
}

/// A parameter type of a function, as checked by the reference implementation.
#[derive(Clone, Copy)]
enum Param {
    Any,
    Number,
    String,
    Array,
    Object,
    Expref,
    ArrayOrString,
    /// A string, array, or object.
    Sized,
    ArrayOfNumbers,
    ArrayOfStrings,
}

impl Param {
    /// Whether a value of type `value` is known to be invalid for this parameter.
    fn rejects(self, value: &Type<'_>) -> bool {
        let value = value.resolve();
        let element_is = |expected: fn(&Type<'_>) -> bool| match &value {
            Type::Array(element) => {
                let element = element.resolve();
                !(matches!(element, Type::Any) || expected(&element))
            }
            _ => true,
        };
        match (self, &value) {
            (_, Type::Any) | (Param::Any, _) => false,
            (Param::Number, Type::Number)
            | (Param::String, Type::String)
            | (Param::Array, Type::Array(_))
            | (Param::Object, Type::Object(_))
            | (Param::Expref, Type::Expref)
            | (Param::ArrayOrString, Type::Array(_) | Type::String)
            | (Param::Sized, Type::String | Type::Array(_) | Type::Object(_)) => false,
            (Param::ArrayOfNumbers, _) => element_is(|t| matches!(t, Type::Number)),
            (Param::ArrayOfStrings, _) => element_is(|t| matches!(t, Type::String)),
            _ => true,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Param::Any => "any value",
            Param::Number => "a `number`",
            Param::String => "a `string`",
            Param::Array => "an `array`",
            Param::Object => "an `object`",
            Param::Expref => "an expression reference (`&expression`)",
            Param::ArrayOrString => "an `array` or a `string`",
            Param::Sized => "a `string`, an `array`, or an `object`",
            Param::ArrayOfNumbers => "an `array` of `number` values",
            Param::ArrayOfStrings => "an `array` of `string` values",
        }
    }
}

/// The type of the reference implementation's sample value for `ast`, a function argument
/// evaluated against `root`, or `Any` where static analysis cannot be certain of it.
///
/// The reference implementation lints an expression by evaluating it against sample data
/// built from the model. A structure or union sample has every member, and a list sample has
/// `min(max(2, min), max)` elements for its `@length`. A map sample's keys are generated, so
/// a field name cannot be relied on to select one. A member's sample is a value of any type
/// when it is visited again, which can only happen once the shape it is selected from repeats
/// on the path from the root. Selecting a field or an index from a value that is not an
/// object or an array gives a value of any type. So the type is certain for:
///
/// - a literal and an expression reference;
/// - `!` and comparisons, which give a boolean or null, both of which every typed parameter
///   rejects; and
/// - a path of member names and indexes from the root that does not select from a map or
///   from the same shape twice. A member that does not exist is null in both.
fn sample_type<'m>(ast: &Ast, root: &Type<'m>) -> Type<'m> {
    match ast {
        Ast::Literal { value, .. } => literal(value),
        Ast::Expref { .. } => Type::Expref,
        Ast::Not { .. } | Ast::Comparison { .. } => Type::Boolean,
        _ => sample_path(ast, root).unwrap_or(Type::Any),
    }
}

/// The type of the reference implementation's sample value for a path of member names and
/// indexes from `root`, or `None` when `ast` is not such a path or its type is not certain.
/// The elements of a list are `Any` when its sample is empty.
fn sample_path<'m>(ast: &Ast, root: &Type<'m>) -> Option<Type<'m>> {
    let mut steps = Vec::new();
    let mut node = ast;
    while let Ast::Subexpr { lhs, rhs, .. } = node {
        steps.push(&**rhs);
        node = lhs;
    }
    steps.push(node);

    // The shapes selected from so far, which must not repeat.
    let mut selected = Vec::new();
    let mut select = |value: &Type<'m>| match value {
        Type::Shape(shape) if selected.contains(&shape.id()) => false,
        Type::Shape(shape) => {
            selected.push(shape.id());
            true
        }
        _ => true,
    };
    let mut current = root.clone();
    for step in steps.into_iter().rev() {
        if matches!(step, Ast::Identity { .. }) {
            continue;
        }
        if !select(&current) {
            return None;
        }
        current = match (step, current.resolve()) {
            (Ast::Field { name, .. }, Type::Object(Object::Members(shape))) => shape
                .member(name)
                .map_or(Type::Null, |member| Type::Shape(member.target_shape())),
            (Ast::Field { name, .. }, Type::Object(Object::Fields(fields))) => fields
                .into_iter()
                .find(|(field, _)| field == name)
                .map_or(Type::Null, |(_, value)| value),
            (Ast::Index { idx, .. }, Type::Array(element)) => {
                let Type::Shape(list) = &current else {
                    return None;
                };
                let length = sample_length(list);
                let index = match i64::from(*idx) {
                    from_end if from_end < 0 => length + from_end,
                    index => index,
                };
                if (0..length).contains(&index) {
                    *element
                } else {
                    Type::Null
                }
            }
            _ => return None,
        };
    }
    Some(match (&current, current.resolve()) {
        // Where the reference implementation requires an array of numbers or strings, it
        // rejects elements of any type too, so only an empty sample makes them uncertain.
        (Type::Shape(list), Type::Array(_)) if sample_length(list) == 0 => {
            Type::Array(Box::new(Type::Any))
        }
        (_, resolved) => resolved,
    })
}

/// The number of elements in the reference implementation's sample of a list.
fn sample_length(list: &ShapeView<'_>) -> i64 {
    let bound = |key: &str| {
        let bound = list
            .get_trait(&LENGTH)?
            .as_object()?
            .get(key)?
            .as_number()?;
        // Truncated, as the reference implementation's `intValue` does.
        Some(bound.as_f64() as i64)
    };
    let mut length = 2;
    if let Some(min) = bound("min") {
        length = length.max(min);
    }
    if let Some(max) = bound("max") {
        length = length.min(max);
    }
    length
}

/// The parameters of a standard JMESPath function: the positional parameters and, for a
/// variadic function, the type of the remaining arguments. The arities are the reference
/// implementation's.
fn signature(name: &str) -> Option<(&'static [Param], Option<Param>)> {
    use Param::*;
    Some(match name {
        "abs" | "ceil" | "floor" => (&[Number], None),
        "avg" | "sum" => (&[ArrayOfNumbers], None),
        "contains" => (&[ArrayOrString, Any], None),
        "ends_with" | "starts_with" => (&[String, String], None),
        "join" => (&[String, ArrayOfStrings], None),
        "keys" | "values" => (&[Object], None),
        "length" => (&[Sized], None),
        "map" => (&[Expref, Array], None),
        "max" | "min" | "sort" => (&[Array], None),
        "max_by" | "min_by" | "sort_by" => (&[Array, Expref], None),
        "merge" => (&[], Some(Object)),
        "not_null" => (&[Any], Some(Any)),
        "reverse" => (&[ArrayOrString], None),
        "to_array" | "to_number" | "to_string" | "type" => (&[Any], None),
        _ => return None,
    })
}

struct Checker {
    problems: Vec<Problem>,
    /// Whether the value being checked against is the root, the value the whole expression is
    /// evaluated against.
    at_root: bool,
}

impl Checker {
    fn report(&mut self, severity: Severity, offset: usize, message: String) {
        self.problems.push(Problem {
            severity,
            offset,
            message,
        });
    }

    fn warn(&mut self, offset: usize, message: String) {
        self.report(Severity::Warning, offset, message);
    }

    /// Checks `ast` evaluated against `current`, a value derived from the root.
    fn check_derived<'m>(&mut self, ast: &Ast, current: &Type<'m>) -> Type<'m> {
        let at_root = std::mem::replace(&mut self.at_root, false);
        let result = self.check(ast, current);
        self.at_root = at_root;
        result
    }

    /// Checks `ast` evaluated against a value of type `current`, returning the type of the
    /// result.
    ///
    /// Chains of `.`, `|`, `||`, `&&`, and comparisons nest to the left without deepening the
    /// parser's recursion, so they can be as long as the expression. They are walked
    /// iteratively; every other node recurses.
    fn check<'m>(&mut self, ast: &Ast, current: &Type<'m>) -> Type<'m> {
        let mut spine = Vec::new();
        let mut node = ast;
        while let Ast::Subexpr { lhs, .. }
        | Ast::Or { lhs, .. }
        | Ast::And { lhs, .. }
        | Ast::Comparison { lhs, .. } = node
        {
            spine.push(node);
            node = lhs;
        }
        // Every node of the spine is evaluated against `current`, like its leftmost operand.
        let mut result = self.check_node(node, current);
        for parent in spine.into_iter().rev() {
            result = match parent {
                Ast::Subexpr { rhs, .. } => self.check_derived(rhs, &result),
                Ast::Or { rhs, .. } | Ast::And { rhs, .. } => {
                    let right = self.check(rhs, current);
                    join(result, right)
                }
                Ast::Comparison {
                    comparator,
                    rhs,
                    offset,
                    ..
                } => {
                    let right = self.check(rhs, current);
                    self.compare(comparator, &result, &right, *offset)
                }
                _ => unreachable!("only binary operators are pushed"),
            };
        }
        result
    }

    fn check_node<'m>(&mut self, ast: &Ast, current: &Type<'m>) -> Type<'m> {
        match ast {
            Ast::Identity { .. } => current.clone(),
            Ast::Literal { value, .. } => literal(value),
            Ast::Field { name, offset } => self.field(current, name, *offset),
            Ast::Index { idx, offset } => match current.resolve() {
                Type::Array(element) => *element,
                Type::Any | Type::Null => current.resolve(),
                other => {
                    self.warn(
                        *offset,
                        format!(
                            "index `[{idx}]` is applied to {}, so it is always null",
                            other.described()
                        ),
                    );
                    Type::Null
                }
            },
            Ast::Slice { offset, .. } => match current.resolve() {
                array @ (Type::Array(_) | Type::Any | Type::Null) => array,
                other => {
                    self.warn(
                        *offset,
                        format!(
                            "a slice is applied to {}, so it is always null",
                            other.described()
                        ),
                    );
                    Type::Null
                }
            },
            Ast::Projection { lhs, rhs, offset } => {
                let left = self.check(lhs, current).resolve();
                match left {
                    Type::Array(element) => {
                        Type::Array(Box::new(projected(self.check_derived(rhs, &element))))
                    }
                    other => {
                        // Flatten, object values, and slices report their own problems and
                        // produce null; a wildcard index `[*]` is applied to `lhs` directly.
                        if !matches!(other, Type::Any | Type::Null) {
                            self.warn(
                                *offset,
                                format!(
                                    "`[*]` projects {}, which is not an array, so it is \
                                     always null",
                                    other.described()
                                ),
                            );
                        }
                        self.check_derived(rhs, &Type::Any);
                        match other {
                            Type::Any => Type::Any,
                            _ => Type::Null,
                        }
                    }
                }
            }
            Ast::Flatten { node, offset } => match self.check(node, current).resolve() {
                Type::Array(element) => match element.resolve() {
                    Type::Array(inner) => Type::Array(inner),
                    _ => Type::Array(element),
                },
                any_or_null @ (Type::Any | Type::Null) => any_or_null,
                other => {
                    self.warn(
                        *offset,
                        format!(
                            "`[]` flattens {}, which is not an array, so it is always null",
                            other.described()
                        ),
                    );
                    Type::Null
                }
            },
            Ast::ObjectValues { node, offset } => match self.check(node, current).resolve() {
                Type::Object(object) => Type::Array(Box::new(values(&object))),
                any_or_null @ (Type::Any | Type::Null) => any_or_null,
                other => {
                    self.warn(
                        *offset,
                        format!(
                            "`*` selects the values of {}, which is not an object, so it \
                             is always null",
                            other.described()
                        ),
                    );
                    Type::Null
                }
            },
            Ast::Condition {
                predicate, then, ..
            } => {
                self.check(predicate, current);
                self.check(then, current)
            }
            Ast::Not { node, .. } => {
                self.check(node, current);
                Type::Boolean
            }
            Ast::MultiList { elements, .. } => {
                // A multi-select of null is null.
                let null = matches!(current.resolve(), Type::Null);
                let input = if null { Type::Any } else { current.clone() };
                let elements: Vec<_> = elements.iter().map(|e| self.check(e, &input)).collect();
                if null {
                    Type::Null
                } else {
                    Type::Array(Box::new(join_all(elements)))
                }
            }
            Ast::MultiHash { elements, .. } => {
                let null = matches!(current.resolve(), Type::Null);
                let input = if null { Type::Any } else { current.clone() };
                let fields = elements
                    .iter()
                    .map(|pair| (pair.key.clone(), self.check(&pair.value, &input)))
                    .collect();
                if null {
                    Type::Null
                } else {
                    Type::Object(Object::Fields(fields))
                }
            }
            Ast::Expref { ast, .. } => {
                // Outside a function argument, the referenced expression is never evaluated.
                self.check_derived(ast, &Type::Any);
                Type::Expref
            }
            Ast::Function { name, args, offset } => self.function(name, args, current, *offset),
            Ast::Subexpr { .. } | Ast::Or { .. } | Ast::And { .. } | Ast::Comparison { .. } => {
                self.check(ast, current)
            }
        }
    }

    fn field<'m>(&mut self, current: &Type<'m>, name: &str, offset: usize) -> Type<'m> {
        match current.resolve() {
            Type::Object(Object::Members(shape)) => match shape.member(name) {
                Some(member) => Type::Shape(member.target_shape()),
                None => {
                    self.warn(
                        offset,
                        format!(
                            "`{name}` is not a member of `{}`, so it is always null",
                            shape.id()
                        ),
                    );
                    Type::Null
                }
            },
            Type::Object(Object::Map(value)) => *value,
            Type::Object(Object::Fields(fields)) => {
                match fields.iter().find(|(field, _)| field == name) {
                    Some((_, value)) => value.clone(),
                    None => {
                        let names: Vec<_> = fields.iter().map(|(n, _)| format!("`{n}`")).collect();
                        self.warn(
                            offset,
                            format!(
                                "`{name}` is not one of the fields {}, so it is always null",
                                names.join(", ")
                            ),
                        );
                        Type::Null
                    }
                }
            }
            Type::Object(Object::Unknown) | Type::Any => Type::Any,
            Type::Null => Type::Null,
            other => {
                self.warn(
                    offset,
                    format!(
                        "field `{name}` is selected from {}, so it is always null",
                        other.described()
                    ),
                );
                Type::Null
            }
        }
    }

    fn compare<'m>(
        &mut self,
        comparator: &Comparator,
        left: &Type<'m>,
        right: &Type<'m>,
        offset: usize,
    ) -> Type<'m> {
        let symbol = match comparator {
            Comparator::Equal | Comparator::NotEqual => return Type::Boolean,
            Comparator::LessThan => "<",
            Comparator::LessThanEqual => "<=",
            Comparator::GreaterThan => ">",
            Comparator::GreaterThanEqual => ">=",
        };
        match (left.resolve(), right.resolve()) {
            (Type::Number, Type::Number) => Type::Boolean,
            (Type::Any, Type::Any | Type::Number) | (Type::Number, Type::Any) => Type::Any,
            (left, right) => {
                let non_number = if matches!(left, Type::Number | Type::Any) {
                    right
                } else {
                    left
                };
                self.warn(
                    offset,
                    format!(
                        "`{symbol}` compares {}, but ordering comparisons need numbers, so \
                         the result is always null",
                        non_number.described()
                    ),
                );
                Type::Null
            }
        }
    }

    fn function<'m>(
        &mut self,
        name: &str,
        args: &[Ast],
        current: &Type<'m>,
        offset: usize,
    ) -> Type<'m> {
        let Some((params, variadic)) = signature(name) else {
            for arg in args {
                self.check(arg, current);
            }
            self.report(
                Severity::Error,
                offset,
                format!("`{name}` is not a JMESPath function"),
            );
            return Type::Any;
        };

        // Expression references are evaluated against the elements of the function's array
        // argument; every other argument is evaluated against the current value.
        let types: Vec<Type<'m>> = args
            .iter()
            .map(|arg| match arg {
                Ast::Expref { .. } => Type::Expref,
                _ => self.check(arg, current),
            })
            .collect();
        let array = match name {
            "map" => types.get(1),
            _ => types.first(),
        };
        let element = array.map_or(Type::Any, Type::element);
        let mut expref_result = Type::Any;
        for arg in args {
            if let Ast::Expref { ast, .. } = arg {
                expref_result = self.check_derived(ast, &element);
            }
        }

        let arity_ok = match variadic {
            None => args.len() == params.len(),
            Some(_) => args.len() >= params.len(),
        };
        if !arity_ok {
            let expected = match (variadic, params.len()) {
                (Some(_), n) => format!("at least {n} argument{}", plural(n)),
                (None, n) => format!("{n} argument{}", plural(n)),
            };
            let given = match args.len() {
                1 => "1 was given".to_owned(),
                n => format!("{n} were given"),
            };
            self.report(
                Severity::Error,
                offset,
                format!("`{name}` takes {expected}, but {given}"),
            );
        } else {
            for (index, (arg, value)) in args.iter().zip(&types).enumerate() {
                let param = params
                    .get(index)
                    .copied()
                    .or(variadic)
                    .unwrap_or(Param::Any);
                if param.rejects(value) {
                    // The reference implementation reports an error when its sample of the
                    // argument has the wrong type. Report one too when that is certain.
                    let severity = if self.at_root && param.rejects(&sample_type(arg, current)) {
                        Severity::Error
                    } else {
                        Severity::Warning
                    };
                    self.report(
                        severity,
                        offset_of(arg).unwrap_or(offset),
                        format!(
                            "argument {index} of `{name}` must be {}, but is {}",
                            param.describe(),
                            value.described()
                        ),
                    );
                }
            }
        }

        let first = types.first().cloned().unwrap_or(Type::Any);
        match name {
            "abs" | "avg" | "ceil" | "floor" | "length" | "sum" | "to_number" => Type::Number,
            "contains" | "ends_with" | "starts_with" => Type::Boolean,
            "join" | "to_string" | "type" => Type::String,
            "keys" => Type::Array(Box::new(Type::String)),
            "values" => match first.resolve() {
                Type::Object(object) => Type::Array(Box::new(values(&object))),
                _ => Type::Any,
            },
            "map" => Type::Array(Box::new(projected(expref_result))),
            "max" | "min" | "max_by" | "min_by" => first.element(),
            "sort" | "sort_by" | "reverse" => first.resolve(),
            "merge" => Type::Object(Object::Unknown),
            "not_null" => join_all(
                types
                    .into_iter()
                    .filter(|t| !matches!(t.resolve(), Type::Null)),
            ),
            "to_array" => match first.resolve() {
                array @ (Type::Array(_) | Type::Any) => array,
                _ => Type::Array(Box::new(first)),
            },
            _ => Type::Any,
        }
    }
}

/// The type of the values of an object.
fn values<'m>(object: &Object<'m>) -> Type<'m> {
    match object {
        Object::Members(shape) => join_all(
            shape
                .members()
                .iter()
                .map(|member| Type::Shape(member.target_shape())),
        ),
        Object::Map(value) => (**value).clone(),
        Object::Fields(fields) => join_all(fields.iter().map(|(_, value)| value.clone())),
        Object::Unknown => Type::Any,
    }
}

/// The offset of an argument node, to locate problems with it.
fn offset_of(ast: &Ast) -> Option<usize> {
    Some(match ast {
        Ast::Field { offset, .. }
        | Ast::Literal { offset, .. }
        | Ast::Identity { offset }
        | Ast::Expref { offset, .. }
        | Ast::Index { offset, .. }
        | Ast::Not { offset, .. }
        | Ast::MultiList { offset, .. }
        | Ast::MultiHash { offset, .. } => *offset,
        _ => return None,
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Model, ShapeId};

    /// A xorshift generator, so that the random test below is the same on every run: a stack
    /// overflow aborts the test binary, which leaves no failing case to shrink or persist, so
    /// a failure must be reproducible by running the test again.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    /// A random expression with at most `depth` nested constructs. Most parse.
    fn random_expression(rng: &mut Rng, depth: usize) -> String {
        const LEAVES: &[&str] = &["a", "@", "'x'", "`1`", "\"q\"", "a[0]", "a.b"];
        let leaf = LEAVES[rng.below(LEAVES.len())];
        if depth == 0 || rng.below(50) == 0 {
            return leaf.to_owned();
        }
        let e = random_expression(rng, depth - 1);
        // The right-hand side of `.` must start with a name, `*`, `{`, `[`, or `&`.
        let dot = |e: &str| match e.as_bytes() {
            [b'a'..=b'z' | b'"' | b'*' | b'{' | b'&', ..] => e.to_owned(),
            [b'[', next, ..] if !matches!(next, b'?' | b']') => e.to_owned(),
            _ => format!("[{e}]"),
        };
        match rng.below(30) {
            0 => format!("({e})"),
            1 => format!("f({e})"),
            2 => format!("f({leaf}, {e})"),
            3 => format!("[{e}]"),
            4 => format!("[{leaf}, {e}]"),
            5 => format!("{{k: {e}}}"),
            6 => format!("{leaf}[?{e}]"),
            7 => format!("{e}[?{leaf}]"),
            8 => format!("!{e}"),
            // `&&` would be the and operator.
            9 => format!("& {e}"),
            10 => format!("{leaf}[*].{}", dot(&e)),
            11 => format!("{e}[*]"),
            12 => format!("*.{}", dot(&e)),
            13 => format!("{e}.*"),
            14 => format!("{e}[]"),
            15 => format!("{leaf}[].{}", dot(&e)),
            16 => format!("{e}[1:]"),
            17 => format!("{leaf}[::-1].{}", dot(&e)),
            18 => format!("{leaf} | {e}"),
            19 => format!("{e} | {leaf}"),
            20 => format!("{leaf} || {e}"),
            21 => format!("{e} || {leaf}"),
            22 => format!("{leaf} && {e}"),
            23 => format!("{e} && {leaf}"),
            24 => format!("{leaf} == {e}"),
            25 => format!("{e} < {leaf}"),
            26 => format!("{leaf}.{}", dot(&e)),
            27 => format!("{e}.{}", dot(leaf)),
            28 => format!("{leaf}[?{leaf}].{}", dot(&e)),
            _ => format!("{leaf} | {leaf} || {leaf} && {leaf} == {leaf}.{}", dot(&e)),
        }
    }

    /// The most levels the parser has open at once while it parses `ast`, as [`nesting_depth`]
    /// counts them: a function's or multi-select's elements, a filter's condition, a
    /// projection's right-hand side, and the operands of `!` and `&`. A left-hand operand is
    /// parsed before the construct it belongs to starts, so it adds nothing. (Grouping
    /// parentheses and the binding-power ladder leave no level here.)
    fn ast_levels(ast: &Ast) -> usize {
        match ast {
            Ast::Function { args, .. } | Ast::MultiList { elements: args, .. } => {
                1 + args.iter().map(ast_levels).max().unwrap_or(0)
            }
            Ast::MultiHash { elements, .. } => {
                1 + elements
                    .iter()
                    .map(|pair| ast_levels(&pair.value))
                    .max()
                    .unwrap_or(0)
            }
            Ast::Not { node, .. } | Ast::Expref { ast: node, .. } => 1 + ast_levels(node),
            // A filter's condition and the projection after it are each one level.
            Ast::Projection { lhs, rhs, .. } => ast_levels(lhs).max(1 + ast_levels(rhs)),
            Ast::Condition {
                predicate, then, ..
            } => ast_levels(predicate).max(ast_levels(then)),
            Ast::Flatten { node, .. } | Ast::ObjectValues { node, .. } => ast_levels(node),
            Ast::Subexpr { lhs, rhs, .. }
            | Ast::Or { lhs, rhs, .. }
            | Ast::And { lhs, rhs, .. }
            | Ast::Comparison { lhs, rhs, .. } => ast_levels(lhs).max(ast_levels(rhs)),
            Ast::Identity { .. }
            | Ast::Field { .. }
            | Ast::Index { .. }
            | Ast::Literal { .. }
            | Ast::Slice { .. } => 0,
        }
    }

    #[test]
    fn estimates_nesting_depth() {
        for (expression, depth) in [
            ("a.b", 0),
            // String and literal contents and plain indexes do not nest.
            ("name == '*********************************'", 0),
            (r#""(" == `"[{"` && '\'[' && `"\`("`"#, 0),
            ("a[0].b[-1].c[ 2 ]", 0),
            // Levels close at their closing token.
            ("f(a) || [b] || {k: c} || (d)", 1),
            ("f(g(h(a)))", 3),
            // `!` and projections close at a loosely binding operator, and stack until then.
            ("!!a || !b", 2),
            ("a[*].b[*].c[].d.*.e[1:].f", 5),
            ("a[*].b || c[].d | e.* == f[::-1]", 1),
            // A filter's condition is a level, and so is the projection after it.
            ("a[?b[?c]]", 2),
            ("a[?b].c[?d].e", 2),
            // `&` closes only at a `,` or its level's closing token, and so does every level
            // open when it starts.
            ("& a || & b", 2),
            ("!& a || !& b", 4),
            ("f(& a, & b, !c)", 2),
            // The deepest expression in the checked-in AWS models.
            (
                "length(services[?!(length(deployments) == `1` && runningCount == \
                 desiredCount)]) == `0`",
                5,
            ),
        ] {
            assert_eq!(nesting_depth(expression), depth, "{expression}");
        }
    }

    #[test]
    fn rejects_expressions_beyond_the_limits_before_parsing() {
        let long = "a".repeat(MAX_LENGTH + 1);
        assert!(matches!(parse(&long), Err(ParseError::TooLong(_))));
        assert!(parse(&"a".repeat(MAX_LENGTH)).is_ok());

        let nested = format!(
            "{}a{}",
            "(".repeat(MAX_NESTING + 1),
            ")".repeat(MAX_NESTING + 1)
        );
        assert!(matches!(parse(&nested), Err(ParseError::TooNested(33))));
        let nested = format!("{}a{}", "(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
        assert!(parse(&nested).is_ok());

        // Long expressions that do not nest are accepted.
        let indexes: String = (0..40).map(|i| format!("a{i}[{i}].")).collect();
        assert!(parse(&format!("{indexes}a")).is_ok());
        let calls = format!("length(a){}", " || length(a)".repeat(40));
        assert!(parse(&calls).is_ok());
    }

    #[test]
    fn parses_the_worst_case_of_each_level_at_the_nesting_limit() {
        // The ladder climbs every binding power, which is as deep as the parser recurses
        // between two levels. These run on the test harness's 2 MiB thread, so they also check
        // that the limit leaves room in unoptimized builds.
        const LADDER: &str = "a | a || a && a == a";
        for (repeated, closing) in [
            (format!("f({LADDER}."), ")"),
            ("(a | a || a && a == ".to_owned(), ")"),
            (format!("{LADDER}.["), "]"),
            (format!("{LADDER}.{{k: "), "}"),
            (format!("{LADDER}[?"), "]"),
            (format!("{LADDER}.&"), ""),
            ("!".to_owned(), ""),
            ("a | a || a && a == !f(".to_owned(), ")"),
            ("a[*].".to_owned(), ""),
            ("*.".to_owned(), ""),
            ("a[0:1].".to_owned(), ""),
            ("a[].".to_owned(), ""),
            ("a[?a].".to_owned(), ""),
            // A projection of a call: the checker recurses through both.
            (format!("f({LADDER}."), ")[*][]"),
            (format!("({LADDER}.f("), ")[])[]"),
        ] {
            let expression = |n: usize| format!("{}a{}", repeated.repeat(n), closing.repeat(n));
            let n = (1..=MAX_NESTING)
                .take_while(|&n| nesting_depth(&expression(n)) <= MAX_NESTING)
                .last()
                .unwrap();
            let deepest = expression(n);
            assert_eq!(nesting_depth(&deepest), MAX_NESTING, "{deepest}");
            if let Err(err) = analyze(&deepest, &Type::Any) {
                panic!("{deepest}: {err}");
            }
            assert!(
                matches!(
                    analyze(&expression(n + 1), &Type::Any),
                    Err(ParseError::TooNested(_))
                ),
                "{repeated}"
            );
        }
    }

    #[test]
    fn parses_random_expressions_within_the_limit() {
        // An expression the estimate accepts must parse and check without exhausting the
        // stack, which would abort the test binary, and must not open more levels than
        // estimated.
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut near_the_limit = 0;
        for _ in 0..20_000 {
            let nesting = 1 + rng.below(60);
            let expression = random_expression(&mut rng, nesting);
            let depth = nesting_depth(&expression);
            if expression.len() > MAX_LENGTH || depth > MAX_NESTING {
                continue;
            }
            let ast = parse(&expression).unwrap_or_else(|err| panic!("{expression}: {err}"));
            let levels = ast_levels(&ast);
            assert!(
                levels <= depth,
                "{expression}: estimated {depth} levels, but the parser opens {levels}"
            );
            analyze(&expression, &Type::Any).unwrap();
            if depth > MAX_NESTING - 4 {
                near_the_limit += 1;
            }
        }
        assert!(near_the_limit > 300, "{near_the_limit} near the limit");
    }

    #[test]
    fn walks_long_left_nested_chains_iteratively() {
        let chain = format!("a{}", ".a".repeat(MAX_LENGTH / 2 - 1));
        assert!(chain.len() <= MAX_LENGTH);
        let analysis = analyze(&chain, &Type::Any).unwrap();
        assert!(matches!(analysis.result, Type::Any));
        let or = format!("a{}", " || a".repeat((MAX_LENGTH - 1) / 5));
        assert!(matches!(
            analyze(&or, &Type::Any).unwrap().result,
            Type::Any
        ));
    }

    #[test]
    fn reports_syntax_errors_with_one_based_positions() {
        let Err(ParseError::Syntax { line, column, .. }) = parse("a ||") else {
            panic!("expected a syntax error");
        };
        assert_eq!((line, column), (1, 5));
        assert_eq!(position("ab\ncd", 4), (2, 2));
        assert_eq!(position("ab", 0), (1, 1));
    }

    fn model() -> Model {
        Model::from_json_str(
            "m.json",
            r#"{"smithy": "2.0", "shapes": {
                "ex#Output": {"type": "structure", "members": {
                    "name": {"target": "smithy.api#String"},
                    "count": {"target": "smithy.api#Integer"},
                    "ready": {"target": "smithy.api#Boolean"},
                    "created": {"target": "smithy.api#Timestamp"},
                    "data": {"target": "smithy.api#Blob"},
                    "doc": {"target": "smithy.api#Document"},
                    "items": {"target": "ex#Items"},
                    "ids": {"target": "ex#Ids"},
                    "none": {"target": "ex#None"},
                    "tags": {"target": "ex#Tags"},
                    "next": {"target": "ex#Output"}
                }},
                "ex#Items": {"type": "list", "member": {"target": "ex#Item"}},
                "ex#Ids": {"type": "list", "member": {"target": "smithy.api#String"}},
                "ex#None": {"type": "list", "member": {"target": "smithy.api#String"},
                    "traits": {"smithy.api#length": {"max": 0}}},
                "ex#Item": {"type": "structure", "members": {
                    "id": {"target": "smithy.api#String"},
                    "size": {"target": "smithy.api#Long"},
                    "children": {"target": "ex#Items"}
                }},
                "ex#Tags": {"type": "map", "key": {"target": "smithy.api#String"},
                    "value": {"target": "smithy.api#String"}}
            }}"#,
        )
        .unwrap()
    }

    /// The JMESPath name of the result of `expression` against `ex#Output`, and the messages
    /// of its problems.
    fn check(model: &Model, expression: &str) -> (String, Vec<String>) {
        let output = model
            .expect_shape(&ShapeId::from_static("ex#Output"))
            .unwrap()
            .expect_shape()
            .unwrap();
        let analysis = analyze(expression, &Type::shape(output)).unwrap();
        let result = match analysis.result.resolve() {
            Type::Array(element) => format!("array<{}>", element.name()),
            other => other.name().to_owned(),
        };
        let problems = analysis
            .problems
            .into_iter()
            .map(|p| format!("{:?} {}: {}", p.severity, p.offset, p.message))
            .collect();
        (result, problems)
    }

    #[test]
    fn infers_types_from_the_model() {
        let model = model();
        for (expression, expected) in [
            ("name", "string"),
            ("data", "string"),
            ("count", "number"),
            ("created", "number"),
            ("ready", "boolean"),
            ("doc", "any"),
            ("doc.anything[0]", "any"),
            ("items", "array<object>"),
            ("items[].id", "array<string>"),
            ("items[*].size", "array<number>"),
            ("items[?size > `1`].id", "array<string>"),
            ("items[].children[].id", "array<string>"),
            ("next.next.next.items[0].id", "string"),
            ("tags", "object"),
            ("tags.anything", "string"),
            ("tags.*", "array<string>"),
            ("*", "array<any>"),
            ("length(items) > `0`", "boolean"),
            ("count == `1` && ready", "boolean"),
            ("name || data", "string"),
            ("name || count", "any"),
            ("!ready", "boolean"),
            ("[name, data]", "array<string>"),
            ("{a: name}", "object"),
            ("keys(tags)", "array<string>"),
            ("values(tags)", "array<string>"),
            ("contains(items[].id, 'x')", "boolean"),
            ("join(', ', items[].id)", "string"),
            ("max_by(items, &size).id", "string"),
            ("sort_by(items, &id)[0].size", "number"),
            ("map(&id, items)", "array<string>"),
            ("not_null(name, data)", "string"),
            ("to_array(name)", "array<string>"),
            ("items[0:2]", "array<object>"),
            ("items | [0] | id", "string"),
            ("`\"literal\"`", "string"),
            ("`[1, 2]`", "array<number>"),
            ("@", "object"),
        ] {
            let (result, problems) = check(&model, expression);
            assert_eq!(result, expected, "{expression}: {problems:?}");
            assert_eq!(problems, Vec::<String>::new(), "{expression}");
        }
    }

    #[test]
    fn reports_problems_at_their_offsets() {
        let model = model();
        let mut failures = Vec::new();
        for (expression, result, problems) in [
            (
                "nmae",
                "null",
                vec!["Warning 0: `nmae` is not a member of `ex#Output`, so it is always null"],
            ),
            (
                "items[].nmae",
                "array<any>",
                vec!["Warning 8: `nmae` is not a member of `ex#Item`, so it is always null"],
            ),
            (
                "name.first",
                "null",
                vec!["Warning 5: field `first` is selected from a `string`, so it is always null"],
            ),
            (
                "count[0]",
                "null",
                vec!["Warning 7: index `[0]` is applied to a `number`, so it is always null"],
            ),
            (
                "name[]",
                "null",
                vec![
                    "Warning 4: `[]` flattens a `string`, which is not an array, so it is always \
                     null",
                ],
            ),
            (
                "name[*]",
                "null",
                vec![
                    "Warning 6: `[*]` projects a `string`, which is not an array, so it is always \
                     null",
                ],
            ),
            (
                "name < `1`",
                "null",
                vec![
                    "Warning 7: `<` compares a `string`, but ordering comparisons need numbers, \
                     so the result is always null",
                ],
            ),
            (
                "lenght(items)",
                "any",
                vec!["Error 6: `lenght` is not a JMESPath function"],
            ),
            (
                "length(items, name)",
                "number",
                vec!["Error 6: `length` takes 1 argument, but 2 were given"],
            ),
            (
                "not_null()",
                "any",
                vec!["Error 8: `not_null` takes at least 1 argument, but 0 were given"],
            ),
            (
                "length(count)",
                "number",
                vec![
                    "Error 7: argument 0 of `length` must be a `string`, an `array`, or an \
                     `object`, but is a `number`",
                ],
            ),
            (
                "sum(items[].id)",
                "number",
                vec![
                    "Warning 3: argument 0 of `sum` must be an `array` of `number` values, but is \
                     an `array`",
                ],
            ),
            // Problems in every subexpression are found, whatever the type flowing into it.
            (
                "doc[?lenght(@)].x",
                "any",
                vec!["Error 11: `lenght` is not a JMESPath function"],
            ),
        ] {
            let (actual, actual_problems) = check(&model, expression);
            if actual != result || actual_problems != problems {
                failures.push(format!("{expression}: {actual} {actual_problems:#?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn joins_nested_lists_to_a_bounded_depth() {
        // Two lists that contain each other, and two valid chains of lists far deeper than any
        // real model. `||` joins the types of its operands element by element.
        const DEPTH: usize = 10_000;
        let mut shapes = vec![
            r#""ex#Output": {"type": "structure", "members": {
                "cyclicA": {"target": "ex#CyclicA"},
                "cyclicB": {"target": "ex#CyclicB"},
                "deepA": {"target": "ex#DeepA0"},
                "deepB": {"target": "ex#DeepB0"}
            }}"#
            .to_owned(),
            r#""ex#CyclicA": {"type": "list", "member": {"target": "ex#CyclicB"}}"#.to_owned(),
            r#""ex#CyclicB": {"type": "list", "member": {"target": "ex#CyclicA"}}"#.to_owned(),
        ];
        for chain in ["A", "B"] {
            for depth in 0..DEPTH {
                let member = match depth + 1 {
                    DEPTH => "smithy.api#String".to_owned(),
                    next => format!("ex#Deep{chain}{next}"),
                };
                shapes.push(format!(
                    r#""ex#Deep{chain}{depth}": {{"type": "list", "member": {{"target": "{member}"}}}}"#
                ));
            }
        }
        let model = Model::from_json_str(
            "m.json",
            &format!(r#"{{"smithy": "2.0", "shapes": {{{}}}}}"#, shapes.join(",")),
        )
        .unwrap();
        for expression in ["cyclicA || cyclicB", "deepA || deepB"] {
            let (result, problems) = check(&model, expression);
            assert_eq!(result, "array<array>", "{expression}");
            assert_eq!(problems, Vec::<String>::new(), "{expression}");
        }
    }

    #[test]
    fn argument_types_are_errors_where_the_reference_sample_is_certain() {
        use Severity::{Error, Warning};
        let model = model();
        let mut failures = Vec::new();
        for (expression, severity) in [
            ("length(count)", Error),
            ("abs(@)", Error),
            ("abs(items[0].id)", Error),
            ("abs(items[-1].id)", Error),
            // Out of range of the two-element sample, so null.
            ("abs(items[2])", Error),
            ("length(nmae)", Error),
            ("abs('x')", Error),
            ("abs(`[1]`)", Error),
            ("abs(&count)", Error),
            ("abs(!ready)", Error),
            ("abs(name == 'x')", Error),
            ("sum(ids)", Error),
            ("count == `1` || length(count) > `0`", Error),
            ("[length(count)]", Error),
            // The reference implementation's sample is a value of any type, an empty array,
            // or not certain to have the inferred type.
            ("length(next.count)", Warning),
            ("abs(items[0].children[0].id)", Warning),
            // A field of null is a value of any type.
            ("abs(items[2].id)", Warning),
            ("sum(none)", Warning),
            ("abs(tags.anything)", Warning),
            ("abs(max_by(items, &size))", Warning),
            ("abs(items[].id)", Warning),
            ("abs(name || name)", Warning),
            ("next.length(count)", Warning),
            ("items[?abs(id) > `0`]", Warning),
            ("map(&abs(id), items)", Warning),
        ] {
            let (_, problems) = check(&model, expression);
            let arguments: Vec<_> = problems
                .iter()
                .filter(|problem| problem.contains(": argument "))
                .collect();
            let expected = format!("{severity:?} ");
            if arguments.len() != 1 || !arguments[0].starts_with(&expected) {
                failures.push(format!("{expression}: {problems:#?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");

        let shape = |id| {
            model
                .expect_shape(&ShapeId::from_static(id))
                .unwrap()
                .expect_shape()
                .unwrap()
        };
        let root = Type::input_output(shape("ex#Item"), shape("ex#Output"));
        for (expression, severity) in [
            ("length(output.count)", Error),
            ("abs(input.id)", Error),
            ("length(output.next.count)", Warning),
        ] {
            let analysis = analyze(expression, &root).unwrap();
            let severities: Vec<_> = analysis.problems.iter().map(|p| p.severity).collect();
            assert_eq!(severities, [severity], "{expression}");
        }
    }

    #[test]
    fn mismatches_describe_known_types_only() {
        let model = model();
        let output = model
            .expect_shape(&ShapeId::from_static("ex#Output"))
            .unwrap()
            .expect_shape()
            .unwrap();
        let result = |expression: &str| analyze(expression, &Type::shape(output)).unwrap().result;
        assert_eq!(result("name").mismatch(Expected::String), None);
        assert_eq!(
            result("count").mismatch(Expected::String).as_deref(),
            Some("a `number`")
        );
        assert_eq!(result("doc").mismatch(Expected::Boolean), None);
        assert_eq!(result("items[].id").mismatch(Expected::StringArray), None);
        assert_eq!(
            result("items[].size")
                .mismatch(Expected::StringOrStringArray)
                .as_deref(),
            Some("an `array` of `number` values")
        );
        assert_eq!(result("items[].nope").mismatch(Expected::StringArray), None);
        assert_eq!(
            result("ready").mismatch(Expected::StringArray).as_deref(),
            Some("a `boolean`")
        );
    }
}

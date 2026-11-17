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
//! Only problems that do not depend on the inferred type are errors: an unknown function or a
//! call with the wrong number of arguments. Everything that depends on the inferred type is a
//! warning. The reference implementation instead lints by evaluating the expression against
//! sample data built from the model, and reports some of the same problems as errors; this
//! checker reports no error that the reference implementation's linter does not.

use crate::diagnostic::Severity;
use crate::shape::{ShapeType, ShapeView};
use jmespath::ast::{Ast, Comparator};
use jmespath::{ErrorReason, Variable};
use std::fmt;

/// The longest expression the validator parses, in bytes. The longest in the checked-in AWS
/// models is about 200 bytes so this leaves an order of magnitude of wiggle room.
pub(super) const MAX_LENGTH: usize = 2048;

/// The most nesting tokens (`(`, `[`, `{`, `!`, `&`, and `*`) an expression may contain.
///
/// The `jmespath` parser is recursive. Its recursion depth can only grow past a small
/// constant (the number of binding powers) at one of these tokens, so limiting them bounds
/// the depth. 32 leaves room on a 2 MiB thread for the worst case in an unoptimized build;
/// the checked-in AWS models use at most 7.
pub(super) const MAX_NESTING: usize = 32;

/// Why an expression could not be parsed.
#[derive(Debug)]
pub(super) enum ParseError {
    /// The expression is longer than [`MAX_LENGTH`].
    TooLong(usize),
    /// The expression has more nesting tokens than [`MAX_NESTING`].
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
            ParseError::TooNested(count) => write!(
                f,
                "has {count} nesting tokens (`(`, `[`, `{{`, `!`, `&`, or `*`), more than the \
                 {MAX_NESTING} this validator parses"
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
    let nesting = nesting_tokens(expression);
    if nesting > MAX_NESTING {
        return Err(ParseError::TooNested(nesting));
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

/// Counts the tokens at which the parser's recursion can deepen. The count over-approximates:
/// characters inside string literals are counted too. `&&` (and) and `!=` (not equal) are not
/// nesting tokens.
fn nesting_tokens(expression: &str) -> usize {
    let mut count = 0;
    let mut chars = expression.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
            }
            '!' if chars.peek() == Some(&'=') => {
                chars.next();
            }
            '(' | '[' | '{' | '!' | '&' | '*' => count += 1,
            _ => {}
        }
    }
    count
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

/// The type of a value that is either `a` or `b`.
fn join<'m>(a: Type<'m>, b: Type<'m>) -> Type<'m> {
    match (a, b) {
        (Type::Shape(a), Type::Shape(b)) if a.id() == b.id() => Type::Shape(a),
        (a, b) => match (a.resolve(), b.resolve()) {
            (Type::Null, Type::Null) => Type::Null,
            (Type::Boolean, Type::Boolean) => Type::Boolean,
            (Type::Number, Type::Number) => Type::Number,
            (Type::String, Type::String) => Type::String,
            (Type::Expref, Type::Expref) => Type::Expref,
            (Type::Array(a), Type::Array(b)) => Type::Array(Box::new(join(*a, *b))),
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
                Ast::Subexpr { rhs, .. } => self.check(rhs, &result),
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
                        Type::Array(Box::new(projected(self.check(rhs, &element))))
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
                        self.check(rhs, &Type::Any);
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
                self.check(ast, &Type::Any);
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
                expref_result = self.check(ast, &element);
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
            self.report(
                Severity::Error,
                offset,
                format!("`{name}` takes {expected}, but {} were given", args.len()),
            );
        } else {
            for (index, (arg, value)) in args.iter().zip(&types).enumerate() {
                let param = params
                    .get(index)
                    .copied()
                    .or(variadic)
                    .unwrap_or(Param::Any);
                if param.rejects(value) {
                    self.warn(
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

    #[test]
    fn counts_nesting_tokens() {
        assert_eq!(nesting_tokens("a.b"), 0);
        assert_eq!(nesting_tokens("length(a[?b == 'x' && c != d]) > `0`"), 2);
        assert_eq!(nesting_tokens("!a && &b && {c: d[*]}"), 5);
        // A real expression from the AWS models.
        assert_eq!(
            nesting_tokens(
                "length(services[?!(length(deployments) == `1` && runningCount == \
                 desiredCount)]) == `0`"
            ),
            5
        );
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
    }

    #[test]
    fn parses_the_worst_case_at_the_nesting_limit() {
        // Each repetition climbs every binding power before the next nesting token, which is
        // the deepest the parser recurses per token. This runs on the test harness's 2 MiB
        // thread, so it also checks the limit leaves room in unoptimized builds.
        let worst = format!(
            "{}a{}",
            "f(a | a || a && a == a.".repeat(MAX_NESTING),
            ")".repeat(MAX_NESTING)
        );
        assert_eq!(nesting_tokens(&worst), MAX_NESTING);
        let analysis = analyze(&worst, &Type::Any).unwrap();
        assert!(!analysis.problems.is_empty(), "`f` is not a function");
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
                    "tags": {"target": "ex#Tags"},
                    "next": {"target": "ex#Output"}
                }},
                "ex#Items": {"type": "list", "member": {"target": "ex#Item"}},
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
                    "Warning 7: argument 0 of `length` must be a `string`, an `array`, or an \
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

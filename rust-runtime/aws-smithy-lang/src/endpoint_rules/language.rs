/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The endpoint rules language: parameters, expressions, templates, conditions, and results.
//!
//! These types record syntax only. Nothing evaluates an expression or calls a function.
//! Equality ignores source locations, like [`Node`](crate::Node) equality.

use crate::diagnostic::SourceLocation;
use std::fmt;

/// A source location that equality ignores.
#[derive(Clone, Default)]
pub(crate) struct Location(pub(crate) SourceLocation);

impl PartialEq for Location {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Location {}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A rules-engine version, such as `1.1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RulesVersion {
    major: u32,
    minor: u32,
}

impl RulesVersion {
    /// Version 1.0.
    pub const V1_0: Self = Self::new(1, 0);
    /// Version 1.1, the minimum version for `smithy.rules#endpointBdd`.
    pub const V1_1: Self = Self::new(1, 1);

    /// Creates a version.
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// The major component.
    pub fn major(self) -> u32 {
        self.major
    }

    /// The minor component.
    pub fn minor(self) -> u32 {
        self.minor
    }

    /// Parses a version the way the reference implementation does: at least two
    /// dot-separated non-negative integer components that fit in `i32`, of which only the
    /// first two are used.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut component = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<i32>().ok().map(|value| value as u32)
        };
        let major = component()?;
        let minor = component()?;
        Some(Self { major, minor })
    }
}

impl fmt::Display for RulesVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Endpoint parameter definitions, in declaration order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parameters {
    // Names are unique: they come from a node object's keys.
    parameters: Box<[Parameter]>,
}

impl Parameters {
    pub(crate) fn new(parameters: Vec<Parameter>) -> Self {
        Self {
            parameters: parameters.into_boxed_slice(),
        }
    }

    /// The parameter named `name`.
    pub fn get(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// The parameters in declaration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Parameter> + '_ {
        self.parameters.iter()
    }

    /// The number of parameters.
    pub fn len(&self) -> usize {
        self.parameters.len()
    }

    /// Whether there are no parameters.
    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty()
    }
}

/// An endpoint parameter definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub(crate) name: String,
    pub(crate) kind: ParameterType,
    pub(crate) required: bool,
    pub(crate) default: Option<ParameterValue>,
    pub(crate) built_in: Option<String>,
    pub(crate) documentation: Option<String>,
    pub(crate) deprecated: Option<Deprecated>,
    pub(crate) location: Location,
}

impl Parameter {
    /// The parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The parameter type.
    pub fn kind(&self) -> ParameterType {
        self.kind
    }

    /// Whether a value is required. A required parameter may still be satisfied by its
    /// [`default`](Self::default).
    pub fn required(&self) -> bool {
        self.required
    }

    /// The modeled default value.
    pub fn default(&self) -> Option<&ParameterValue> {
        self.default.as_ref()
    }

    /// The name of the built-in value that supplies this parameter, such as `AWS::Region`.
    pub fn built_in(&self) -> Option<&str> {
        self.built_in.as_deref()
    }

    /// The documentation.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// The deprecation metadata.
    pub fn deprecated(&self) -> Option<&Deprecated> {
        self.deprecated.as_ref()
    }

    /// Where the parameter definition was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// Deprecation metadata for a parameter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deprecated {
    pub(crate) message: Option<String>,
    pub(crate) since: Option<String>,
}

impl Deprecated {
    /// The deprecation message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// When the parameter was deprecated.
    pub fn since(&self) -> Option<&str> {
        self.since.as_deref()
    }
}

/// The type of an endpoint parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterType {
    /// `string`
    String,
    /// `boolean`
    Boolean,
    /// `stringArray`
    StringArray,
}

impl ParameterType {
    /// The name used in the trait value.
    pub fn as_str(self) -> &'static str {
        match self {
            ParameterType::String => "string",
            ParameterType::Boolean => "boolean",
            ParameterType::StringArray => "stringArray",
        }
    }
}

/// A modeled parameter default value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterValue {
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// An array of strings.
    StringArray(Vec<String>),
}

impl ParameterValue {
    /// The type of this value.
    pub fn kind(&self) -> ParameterType {
        match self {
            ParameterValue::String(_) => ParameterType::String,
            ParameterValue::Boolean(_) => ParameterType::Boolean,
            ParameterValue::StringArray(_) => ParameterType::StringArray,
        }
    }
}

/// An expression: a literal, a reference to a parameter or assignment, or a function call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) location: Location,
}

impl Expression {
    /// What kind of expression this is.
    pub fn kind(&self) -> &ExpressionKind {
        &self.kind
    }

    /// Where the expression was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// The kinds of [`Expression`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExpressionKind {
    /// A literal value.
    Literal(Literal),
    /// A reference (`{"ref": name}`) to a parameter or condition assignment.
    Reference(String),
    /// A function call (`{"fn": name, "argv": [...]}`).
    ///
    /// The name is recorded as written; unknown functions are preserved.
    Function {
        /// The function ID, such as `isSet` or `aws.partition`.
        name: String,
        /// The arguments, in order.
        arguments: Box<[Expression]>,
    },
}

/// A literal value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Literal {
    /// A boolean.
    Boolean(bool),
    /// An integer.
    Integer(i32),
    /// A string template, which may reference parameters and assignments.
    String(Template),
    /// A record, in declaration order.
    Record(Box<[(String, Literal)]>),
    /// A tuple.
    Tuple(Box<[Literal]>),
}

/// A string template such as `https://{Region}.{PartitionResult#dnsSuffix}`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Template {
    pub(crate) segments: Box<[TemplateSegment]>,
}

impl Template {
    /// The segments, in order. A plain string has at most one literal segment.
    pub fn segments(&self) -> &[TemplateSegment] {
        &self.segments
    }

    /// The template's text when it contains no references.
    pub fn as_static(&self) -> Option<&str> {
        match &*self.segments {
            [] => Some(""),
            [TemplateSegment::Literal(text)] => Some(text),
            _ => None,
        }
    }
}

/// A segment of a [`Template`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TemplateSegment {
    /// Literal text, with `{{` and `}}` escapes already removed.
    Literal(String),
    /// A reference such as `{Region}` or `{PartitionResult#dnsSuffix}`.
    Reference {
        /// The referenced parameter or assignment.
        name: String,
        /// The attribute path after `#`; empty for a plain reference.
        path: Box<[PathSegment]>,
    },
}

/// A segment of an attribute path such as `resourceId[1]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PathSegment {
    /// A record key.
    Key(String),
    /// An array index. Negative indexes count from the end and require version 1.1.
    Index(i32),
}

/// A condition: a function call whose result may be assigned to a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    pub(crate) function: Expression,
    pub(crate) assign: Option<String>,
    pub(crate) location: Location,
}

impl Condition {
    /// The function call. Its [`kind`](Expression::kind) is always
    /// [`ExpressionKind::Function`].
    pub fn function(&self) -> &Expression {
        &self.function
    }

    /// The function name.
    pub fn function_name(&self) -> &str {
        match &self.function.kind {
            ExpressionKind::Function { name, .. } => name,
            _ => unreachable!("conditions are always function calls"),
        }
    }

    /// The function arguments.
    pub fn arguments(&self) -> &[Expression] {
        match &self.function.kind {
            ExpressionKind::Function { arguments, .. } => arguments,
            _ => unreachable!("conditions are always function calls"),
        }
    }

    /// The name the function's result is assigned to.
    pub fn assign(&self) -> Option<&str> {
        self.assign.as_deref()
    }

    /// Where the condition was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// An endpoint definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointDefinition {
    pub(crate) url: Expression,
    pub(crate) headers: Box<[(String, Box<[Expression]>)]>,
    pub(crate) properties: Box<[(String, Literal)]>,
    pub(crate) location: Location,
}

impl EndpointDefinition {
    /// The URL expression.
    pub fn url(&self) -> &Expression {
        &self.url
    }

    /// The headers and their value expressions, in order.
    pub fn headers(&self) -> impl ExactSizeIterator<Item = (&str, &[Expression])> + '_ {
        self.headers
            .iter()
            .map(|(name, values)| (name.as_str(), &**values))
    }

    /// The endpoint properties, such as `authSchemes`, in order.
    pub fn properties(&self) -> impl ExactSizeIterator<Item = (&str, &Literal)> + '_ {
        self.properties
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// Where the endpoint was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// A conditionless endpoint or error rule, as used for BDD results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultRule {
    pub(crate) documentation: Option<String>,
    pub(crate) kind: ResultKind,
    pub(crate) location: Location,
}

impl ResultRule {
    /// The documentation.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// The endpoint or error.
    pub fn kind(&self) -> &ResultKind {
        &self.kind
    }

    /// Where the rule was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// The outcome of a [`ResultRule`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResultKind {
    /// An endpoint.
    Endpoint(EndpointDefinition),
    /// An error message expression.
    Error(Expression),
}

/// A rule in an endpoint rule set: conditions that must all hold, and an outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub(crate) conditions: Box<[Condition]>,
    pub(crate) documentation: Option<String>,
    pub(crate) kind: RuleKind,
    pub(crate) location: Location,
}

impl Rule {
    /// The conditions, in the order they are tested. Each condition sees the assignments of
    /// the conditions before it and of its ancestors' conditions.
    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }

    /// The documentation.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }

    /// The endpoint, error, or nested rules.
    pub fn kind(&self) -> &RuleKind {
        &self.kind
    }

    /// Where the rule was loaded from.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location.0
    }
}

/// The outcome of a [`Rule`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleKind {
    /// An endpoint.
    Endpoint(EndpointDefinition),
    /// An error message expression.
    Error(Expression),
    /// Nested rules, tried in order.
    Tree(Box<[Rule]>),
}

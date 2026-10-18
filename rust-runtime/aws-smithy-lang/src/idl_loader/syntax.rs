/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::diagnostic::Position;
use crate::node::Number;
use std::sync::Arc;

/// A half-open UTF-8 byte range in an IDL input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl Span {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Spanned<T> {
    pub(crate) value: T,
    pub(crate) span: Span,
}

impl<T> Spanned<T> {
    pub(crate) fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// Maps UTF-8 byte offsets to Smithy's one-based character columns.
#[derive(Debug, Clone)]
pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(input: &str) -> Self {
        let mut starts = vec![0];
        let bytes = input.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            match bytes[offset] {
                b'\n' => starts.push(offset + 1),
                b'\r' => {
                    if bytes.get(offset + 1) == Some(&b'\n') {
                        offset += 1;
                    }
                    starts.push(offset + 1);
                }
                _ => {}
            }
            offset += 1;
        }
        Self { starts }
    }

    pub(crate) fn position(&self, input: &str, offset: usize) -> Position {
        let mut offset = offset.min(input.len());
        while offset > 0 && !input.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let line_start = self.starts[line];
        let column = input[line_start..offset].chars().count() + 1;
        Position {
            line: u32::try_from(line + 1).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseErrorKind {
    Syntax,
    DuplicateKey,
    ResourceLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdlParseError {
    pub(crate) offset: usize,
    pub(crate) kind: ParseErrorKind,
    pub(crate) message: String,
}

impl IdlParseError {
    pub(crate) fn syntax(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            kind: ParseErrorKind::Syntax,
            message: message.into(),
        }
    }

    pub(crate) fn duplicate(offset: usize, key: &str) -> Self {
        Self {
            offset,
            kind: ParseErrorKind::DuplicateKey,
            message: format!("duplicate key `{key}`"),
        }
    }

    pub(crate) fn resource(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            kind: ParseErrorKind::ResourceLimit,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ShapeIdSyntax {
    pub(crate) value: String,
    pub(crate) absolute: bool,
    pub(crate) member: Option<String>,
    pub(crate) span: Span,
}

impl ShapeIdSyntax {
    pub(crate) fn root_name(&self) -> &str {
        let root = self
            .value
            .rsplit_once('#')
            .map_or(self.value.as_str(), |(_, name)| name);
        root.split_once('$').map_or(root, |(name, _)| name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeSyntax {
    Null(Span),
    Bool(bool, Span),
    Number(Spanned<Number>),
    String(Spanned<String>),
    SyntacticShapeId(ShapeIdSyntax),
    Array(Vec<NodeSyntax>, Span),
    Object(Vec<(Spanned<String>, NodeSyntax)>, Span),
}

impl NodeSyntax {
    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Null(span)
            | Self::Bool(_, span)
            | Self::Array(_, span)
            | Self::Object(_, span) => *span,
            Self::Number(value) => value.span,
            Self::String(value) => value.span,
            Self::SyntacticShapeId(value) => value.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TraitValueSyntax {
    Omitted { at: Span },
    Explicit(NodeSyntax),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TraitSyntax {
    pub(crate) id: ShapeIdSyntax,
    pub(crate) value: TraitValueSyntax,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Annotations {
    pub(crate) documentation: Option<Spanned<String>>,
    pub(crate) traits: Vec<TraitSyntax>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MemberNameSyntax {
    Named(Spanned<String>),
    Elided(Spanned<String>),
}

impl MemberNameSyntax {
    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Named(value) | Self::Elided(value) => value.span,
        }
    }

    pub(crate) fn value(&self) -> &str {
        match self {
            Self::Named(value) | Self::Elided(value) => &value.value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MemberSyntax {
    pub(crate) annotations: Annotations,
    pub(crate) name: MemberNameSyntax,
    pub(crate) target: Option<ShapeIdSyntax>,
    pub(crate) value: Option<NodeSyntax>,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyValueSyntax {
    Node(NodeSyntax),
    InlineStructure {
        annotations: Annotations,
        members: Vec<MemberSyntax>,
        resource: Option<ShapeIdSyntax>,
        mixins: Vec<ShapeIdSyntax>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertySyntax {
    pub(crate) name: Spanned<String>,
    pub(crate) value: PropertyValueSyntax,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeKindSyntax {
    Blob,
    Boolean,
    Document,
    String,
    Byte,
    Short,
    Integer,
    Long,
    Float,
    Double,
    BigInteger,
    BigDecimal,
    Timestamp,
    List,
    Map,
    Structure,
    Union,
    Enum,
    IntEnum,
    Service,
    Resource,
    Operation,
    /// Smithy 1.0 `set`, retained only for a precise unsupported diagnostic.
    Set,
}

impl ShapeKindSyntax {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Boolean => "boolean",
            Self::Document => "document",
            Self::String => "string",
            Self::Byte => "byte",
            Self::Short => "short",
            Self::Integer => "integer",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
            Self::BigInteger => "bigInteger",
            Self::BigDecimal => "bigDecimal",
            Self::Timestamp => "timestamp",
            Self::List => "list",
            Self::Map => "map",
            Self::Structure => "structure",
            Self::Union => "union",
            Self::Enum => "enum",
            Self::IntEnum => "intEnum",
            Self::Service => "service",
            Self::Resource => "resource",
            Self::Operation => "operation",
            Self::Set => "set",
        }
    }

    pub(crate) fn is_simple(self) -> bool {
        matches!(
            self,
            Self::Blob
                | Self::Boolean
                | Self::Document
                | Self::String
                | Self::Byte
                | Self::Short
                | Self::Integer
                | Self::Long
                | Self::Float
                | Self::Double
                | Self::BigInteger
                | Self::BigDecimal
                | Self::Timestamp
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapeBodySyntax {
    None,
    Members(Vec<MemberSyntax>),
    Properties(Vec<PropertySyntax>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeSyntax {
    pub(crate) annotations: Annotations,
    pub(crate) kind: ShapeKindSyntax,
    pub(crate) name: Spanned<String>,
    pub(crate) resource: Option<ShapeIdSyntax>,
    pub(crate) mixins: Vec<ShapeIdSyntax>,
    pub(crate) body: ShapeBodySyntax,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ApplySyntax {
    pub(crate) annotations: Annotations,
    pub(crate) target: ShapeIdSyntax,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StatementSyntax {
    Shape(ShapeSyntax),
    Apply(ApplySyntax),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetadataSyntax {
    pub(crate) key: Spanned<String>,
    pub(crate) value: NodeSyntax,
    pub(crate) span: Span,
}

#[derive(Debug, Clone)]
pub(crate) struct IdlDocument {
    pub(crate) source: Arc<str>,
    pub(crate) text: Arc<str>,
    pub(crate) lines: LineIndex,
    pub(crate) controls: Vec<(Spanned<String>, NodeSyntax)>,
    pub(crate) metadata: Vec<MetadataSyntax>,
    pub(crate) namespace: Option<Spanned<String>>,
    pub(crate) uses: Vec<ShapeIdSyntax>,
    pub(crate) statements: Vec<StatementSyntax>,
}

impl IdlDocument {
    pub(crate) fn position(&self, span: Span) -> Position {
        self.lines.position(&self.text, span.start)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shape types and borrowed shape views.

use std::fmt;

/// The type of a shape, used for filtering and refinement.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShapeType {
    /// `blob`
    Blob,
    /// `boolean`
    Boolean,
    /// `document`
    Document,
    /// `string`
    String,
    /// `byte`
    Byte,
    /// `short`
    Short,
    /// `integer`
    Integer,
    /// `long`
    Long,
    /// `float`
    Float,
    /// `double`
    Double,
    /// `bigInteger`
    BigInteger,
    /// `bigDecimal`
    BigDecimal,
    /// `timestamp`
    Timestamp,
    /// `list`
    List,
    /// `map`
    Map,
    /// `structure`
    Structure,
    /// `union`
    Union,
    /// `enum`
    Enum,
    /// `intEnum`
    IntEnum,
    /// `service`
    Service,
    /// `resource`
    Resource,
    /// `operation`
    Operation,
    /// `member`
    Member,
}

impl ShapeType {
    /// Every shape type, in declaration order.
    pub const ALL: [ShapeType; 23] = [
        ShapeType::Blob,
        ShapeType::Boolean,
        ShapeType::Document,
        ShapeType::String,
        ShapeType::Byte,
        ShapeType::Short,
        ShapeType::Integer,
        ShapeType::Long,
        ShapeType::Float,
        ShapeType::Double,
        ShapeType::BigInteger,
        ShapeType::BigDecimal,
        ShapeType::Timestamp,
        ShapeType::List,
        ShapeType::Map,
        ShapeType::Structure,
        ShapeType::Union,
        ShapeType::Enum,
        ShapeType::IntEnum,
        ShapeType::Service,
        ShapeType::Resource,
        ShapeType::Operation,
        ShapeType::Member,
    ];

    /// The JSON AST `type` name, such as `bigInteger`.
    pub fn as_str(self) -> &'static str {
        match self {
            ShapeType::Blob => "blob",
            ShapeType::Boolean => "boolean",
            ShapeType::Document => "document",
            ShapeType::String => "string",
            ShapeType::Byte => "byte",
            ShapeType::Short => "short",
            ShapeType::Integer => "integer",
            ShapeType::Long => "long",
            ShapeType::Float => "float",
            ShapeType::Double => "double",
            ShapeType::BigInteger => "bigInteger",
            ShapeType::BigDecimal => "bigDecimal",
            ShapeType::Timestamp => "timestamp",
            ShapeType::List => "list",
            ShapeType::Map => "map",
            ShapeType::Structure => "structure",
            ShapeType::Union => "union",
            ShapeType::Enum => "enum",
            ShapeType::IntEnum => "intEnum",
            ShapeType::Service => "service",
            ShapeType::Resource => "resource",
            ShapeType::Operation => "operation",
            ShapeType::Member => "member",
        }
    }

    /// Parses a root-shape JSON AST `type` name. `member` is not a root shape type.
    pub(crate) fn from_root_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|t| *t != ShapeType::Member && t.as_str() == name)
    }

    /// Returns `true` for the simple (non-aggregate, non-service) shape types.
    pub fn is_simple(self) -> bool {
        matches!(
            self,
            ShapeType::Blob
                | ShapeType::Boolean
                | ShapeType::Document
                | ShapeType::String
                | ShapeType::Byte
                | ShapeType::Short
                | ShapeType::Integer
                | ShapeType::Long
                | ShapeType::Float
                | ShapeType::Double
                | ShapeType::BigInteger
                | ShapeType::BigDecimal
                | ShapeType::Timestamp
        )
    }

    /// Returns `true` for service, resource, and operation.
    pub fn is_service_shape(self) -> bool {
        matches!(
            self,
            ShapeType::Service | ShapeType::Resource | ShapeType::Operation
        )
    }
}

impl fmt::Display for ShapeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

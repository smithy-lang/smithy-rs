/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shapes: owned semantic shape types, their builders, and borrowed views into a model.
//!
//! There are two ways to look at a shape:
//!
//! - **Owned shapes** ([`Shape`], [`StructureShape`], [`MemberShape`], ...) are plain values.
//!   Build them with builders such as [`StructureShape::builder`], or copy one out of a model
//!   with [`ShapeView::shape`]. Owned shapes do not need to be part of a model, and their
//!   references do not need to resolve until they are assembled with
//!   [`ModelAssembler::add_shape`](crate::ModelAssembler::add_shape).
//! - **Views** ([`ShapeView`], [`ServiceView`], [`MemberView`], ...) are cheap `Copy` borrows
//!   of a shape inside a [`Model`](crate::Model). Because every reference in a model resolves,
//!   views can follow references, such as [`MemberView::target_shape`], and answer
//!   model-wide queries, such as [`ServiceView::contained_operations`].
//!
//! ```
//! use aws_smithy_lang::shape::{OperationShape, StructureShape};
//! use aws_smithy_lang::{traits, Model, Node, NodeObject, ShapeId};
//!
//! let input = StructureShape::builder("ex#GetThingInput")
//!     .member("id", "smithy.api#String")
//!     .add_trait(traits::INPUT, NodeObject::new())
//!     .build()?;
//! let operation = OperationShape::builder("ex#GetThing").input("ex#GetThingInput").build()?;
//!
//! let model = Model::assembler()
//!     .add_shape(input)
//!     .add_shape(operation)
//!     .assemble()
//!     .into_result()?;
//! let operation = model.expect_shape(&ShapeId::from_static("ex#GetThing"))?.expect_operation()?;
//! assert_eq!(operation.input_shape().members().len(), 1);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```

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
    ///
    /// Note: This is a slice rather than an array so that adding a variant
    /// to this `#[non_exhaustive]` enum does not change the constant's type.
    pub const ALL: &'static [ShapeType] = &[
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

pub(crate) mod owned;
mod view;

pub use owned::{
    EnumShape, EnumShapeBuilder, IntEnumShape, IntEnumShapeBuilder, ListShape, ListShapeBuilder,
    MapShape, MapShapeBuilder, MemberIter, MemberShape, MemberShapeBuilder, OperationShape,
    OperationShapeBuilder, ResourceShape, ResourceShapeBuilder, ServiceShape, ServiceShapeBuilder,
    Shape, ShapeBuildError, SimpleShape, SimpleShapeBuilder, StructureShape, StructureShapeBuilder,
    UnionShape, UnionShapeBuilder,
};
pub(crate) use owned::{Members, TraitMap};
pub use view::{
    AppliedTrait, EnumValue, EnumView, IntEnumView, ListView, MapView, MemberView, MembersView,
    OperationView, ResourceView, ServiceView, ShapeExpectationError, ShapeRef, ShapeView,
    StructureView, Traits, UnionView,
};

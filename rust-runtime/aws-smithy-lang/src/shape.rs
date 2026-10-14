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

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

use crate::ast::{
    DeclKind, MemberDecl, OperationDecl, ResourceDecl, ServiceDecl, ShapeDecl, TraitMap,
};
use crate::diagnostic::{escape_pointer_token, SourceLocation};
use crate::loader::PRELUDE_NAMESPACE;
use crate::model::{unit_id, ModelData};
use crate::node::Node;
use crate::shape_id::ShapeId;
use crate::traits::{self, TraitCodec, TraitDecodeError};

/// Generates `is_*`, `as_*`, and `expect_*` refinement methods.
macro_rules! shape_refinements {
    ($($ty:ident => $view:ident: $is:ident, $as:ident, $expect:ident;)*) => {
        $(
            #[doc = concat!("Returns `true` if this is a `", stringify!($ty), "` shape.")]
            pub fn $is(&self) -> bool {
                self.shape_type() == ShapeType::$ty
            }

            #[doc = concat!("Returns a [`", stringify!($view), "`] if this is a `", stringify!($ty), "` shape.")]
            pub fn $as(&self) -> Option<$view<'a>> {
                self.$is().then(|| $view { shape: self.shape() })
            }

            #[doc = concat!("Returns a [`", stringify!($view), "`], or an error if this is not a `", stringify!($ty), "` shape.")]
            pub fn $expect(&self) -> Result<$view<'a>, ShapeExpectationError> {
                self.$as().ok_or_else(|| {
                    ShapeExpectationError::wrong_type(self.id(), ShapeType::$ty, self.shape_type())
                })
            }
        )*
    };
}

macro_rules! all_refinements {
    ($macro:ident) => {
        $macro! {
            List => ListView: is_list, as_list, expect_list;
            Map => MapView: is_map, as_map, expect_map;
            Structure => StructureView: is_structure, as_structure, expect_structure;
            Union => UnionView: is_union, as_union, expect_union;
            Enum => EnumView: is_enum, as_enum, expect_enum;
            IntEnum => IntEnumView: is_int_enum, as_int_enum, expect_int_enum;
            Service => ServiceView: is_service, as_service, expect_service;
            Operation => OperationView: is_operation, as_operation, expect_operation;
            Resource => ResourceView: is_resource, as_resource, expect_resource;
        }
    };
}

/// The applied traits of a shape or member, in source order.
#[derive(Clone, Copy)]
pub struct Traits<'a> {
    map: &'a TraitMap,
    owner: &'a SourceLocation,
}

impl<'a> Traits<'a> {
    /// The number of applied traits.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if no traits are applied.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The value of a trait, by absolute shape ID (a [`ShapeId`] or `&str`).
    pub fn get(&self, id: impl AsRef<str>) -> Option<&'a Node> {
        self.map.get(id.as_ref())
    }

    /// Returns `true` if the trait is applied.
    pub fn contains(&self, id: impl AsRef<str>) -> bool {
        self.map.contains_key(id.as_ref())
    }

    /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is not applied.
    pub fn get_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
        traits::decode(self.get(T::ID))
    }

    /// Iterates applied traits.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = AppliedTrait<'a>> + 'a {
        let owner = self.owner;
        self.map
            .iter()
            .map(move |(id, value)| AppliedTrait { id, value, owner })
    }

    /// Iterates applied trait IDs.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.map.keys()
    }
}

impl fmt::Debug for Traits<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.map.iter()).finish()
    }
}

/// One applied trait.
#[derive(Clone, Copy, Debug)]
pub struct AppliedTrait<'a> {
    id: &'a ShapeId,
    value: &'a Node,
    owner: &'a SourceLocation,
}

impl<'a> AppliedTrait<'a> {
    /// The trait's shape ID.
    pub fn id(&self) -> &'a ShapeId {
        self.id
    }

    /// The trait value.
    pub fn value(&self) -> &'a Node {
        self.value
    }

    /// Where the trait was applied.
    pub fn source(&self) -> SourceLocation {
        self.owner.child(&format!(
            "/traits/{}",
            escape_pointer_token(self.id.as_str())
        ))
    }
}

/// A root shape in a [`Model`](crate::Model).
#[derive(Clone, Copy)]
pub struct ShapeView<'a> {
    pub(crate) data: &'a ModelData,
    pub(crate) decl: &'a ShapeDecl,
}

impl<'a> ShapeView<'a> {
    pub(crate) fn new(data: &'a ModelData, decl: &'a ShapeDecl) -> Self {
        Self { data, decl }
    }

    fn shape(&self) -> ShapeView<'a> {
        *self
    }

    /// The shape ID.
    pub fn id(&self) -> &'a ShapeId {
        &self.decl.id
    }

    /// The shape type.
    pub fn shape_type(&self) -> ShapeType {
        self.decl.shape_type()
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'a> {
        Traits {
            map: &self.decl.traits,
            owner: &self.decl.source,
        }
    }

    /// The value of a trait, by absolute shape ID (a [`ShapeId`] or `&str`).
    pub fn get_trait(&self, id: impl AsRef<str>) -> Option<&'a Node> {
        self.decl.traits.get(id.as_ref())
    }

    /// Returns `true` if the trait is applied.
    pub fn has_trait(&self, id: impl AsRef<str>) -> bool {
        self.decl.traits.contains_key(id.as_ref())
    }

    /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is not applied.
    pub fn get_trait_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
        self.traits().get_as()
    }

    /// Where the shape was declared.
    pub fn source(&self) -> &'a SourceLocation {
        &self.decl.source
    }

    /// Returns `true` if this shape comes from the injected Smithy prelude.
    pub fn is_prelude(&self) -> bool {
        self.data.prelude.is_some() && self.decl.id.namespace() == PRELUDE_NAMESPACE
    }

    /// The members: a list's `member`, a map's `key` and `value`, or named members in
    /// declaration order. Other shapes have no members.
    pub fn members(&self) -> MembersView<'a> {
        MembersView {
            data: self.data,
            decl: Some(self.decl),
        }
    }

    /// A member by name.
    pub fn member(&self, name: &str) -> Option<MemberView<'a>> {
        self.members().get(name)
    }

    all_refinements!(shape_refinements);
}

impl fmt::Debug for ShapeView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShapeView")
            .field("id", self.id())
            .field("type", &self.shape_type())
            .finish()
    }
}

/// A uniform view of a shape's members.
#[derive(Clone, Copy)]
pub struct MembersView<'a> {
    data: &'a ModelData,
    decl: Option<&'a ShapeDecl>,
}

impl<'a> MembersView<'a> {
    /// The number of members.
    pub fn len(&self) -> usize {
        self.decl.map_or(0, ShapeDecl::member_count)
    }

    /// Returns `true` if there are no members.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A member by name.
    pub fn get(&self, name: &str) -> Option<MemberView<'a>> {
        let container = self.decl?;
        let decl = container.member(name)?;
        Some(MemberView {
            data: self.data,
            container,
            decl,
        })
    }

    /// Iterates members in order.
    pub fn iter(&self) -> impl Iterator<Item = MemberView<'a>> + 'a {
        let data = self.data;
        self.decl.into_iter().flat_map(move |container| {
            container.members().map(move |decl| MemberView {
                data,
                container,
                decl,
            })
        })
    }

    /// Iterates member names in order.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.decl
            .into_iter()
            .flat_map(|container| container.members().map(MemberDecl::name))
    }

    /// Iterates `(name, member)` pairs in order.
    pub fn iter_named(&self) -> impl Iterator<Item = (&'a str, MemberView<'a>)> + 'a {
        self.iter().map(|member| (member.name(), member))
    }
}

impl fmt::Debug for MembersView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

/// The effective value of an `enum` or `intEnum` member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumValue<'a> {
    /// An `enum` member's explicit `@enumValue`, or its name when omitted.
    String(&'a str),
    /// An `intEnum` member's `@enumValue`.
    Int(i64),
}

/// A member of a list, map, structure, union, enum, or intEnum.
#[derive(Clone, Copy)]
pub struct MemberView<'a> {
    data: &'a ModelData,
    container: &'a ShapeDecl,
    decl: &'a MemberDecl,
}

impl<'a> MemberView<'a> {
    pub(crate) fn new(data: &'a ModelData, container: &'a ShapeDecl, decl: &'a MemberDecl) -> Self {
        Self {
            data,
            container,
            decl,
        }
    }

    /// The member ID, such as `com.example#Shape$member`.
    pub fn id(&self) -> &'a ShapeId {
        &self.decl.id
    }

    /// The member name.
    pub fn name(&self) -> &'a str {
        self.decl.name()
    }

    /// The target shape ID.
    pub fn target(&self) -> &'a ShapeId {
        &self.decl.target
    }

    /// The target shape. Every member of a loaded model has a resolved target.
    pub fn target_shape(&self) -> ShapeView<'a> {
        let decl = self
            .data
            .root(self.decl.target.as_str())
            .expect("validated models only contain resolved member targets");
        ShapeView::new(self.data, decl)
    }

    /// The shape containing this member.
    pub fn container(&self) -> ShapeView<'a> {
        ShapeView::new(self.data, self.container)
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'a> {
        Traits {
            map: &self.decl.traits,
            owner: &self.decl.source,
        }
    }

    /// The value of a trait, by absolute shape ID (a [`ShapeId`] or `&str`).
    pub fn get_trait(&self, id: impl AsRef<str>) -> Option<&'a Node> {
        self.decl.traits.get(id.as_ref())
    }

    /// Returns `true` if the trait is applied.
    pub fn has_trait(&self, id: impl AsRef<str>) -> bool {
        self.decl.traits.contains_key(id.as_ref())
    }

    /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is not applied.
    pub fn get_trait_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
        self.traits().get_as()
    }

    /// Where the member was declared.
    pub fn source(&self) -> &'a SourceLocation {
        &self.decl.source
    }

    /// The effective enum value for `enum` and `intEnum` members; `None` for other members.
    ///
    /// An `enum` member that omits `@enumValue` has its member name as its effective value. Use
    /// [`declared_enum_value`](Self::declared_enum_value) to see what was actually written.
    pub fn enum_value(&self) -> Option<EnumValue<'a>> {
        let explicit = self.get_trait(traits::ENUM_VALUE);
        match self.container.kind {
            DeclKind::Enum(_) => match explicit {
                Some(node) => node.as_str().map(EnumValue::String),
                None => Some(EnumValue::String(self.name())),
            },
            DeclKind::IntEnum(_) => explicit?.as_number()?.as_i64().map(EnumValue::Int),
            _ => None,
        }
    }

    /// The explicitly declared `@enumValue` of an `enum` or `intEnum` member, or `None` if it
    /// was omitted or this is not an enum member. Unlike [`enum_value`](Self::enum_value), an
    /// omitted `enum` value is not synthesized from the member name.
    pub fn declared_enum_value(&self) -> Option<EnumValue<'a>> {
        match self.container.kind {
            DeclKind::Enum(_) | DeclKind::IntEnum(_) => {
                self.get_trait(traits::ENUM_VALUE)?;
                self.enum_value()
            }
            _ => None,
        }
    }
}

impl fmt::Debug for MemberView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemberView")
            .field("id", self.id())
            .field("target", self.target())
            .finish()
    }
}

/// Either a root shape or a member.
#[derive(Clone, Copy, Debug)]
pub enum ShapeRef<'a> {
    /// A root shape.
    Shape(ShapeView<'a>),
    /// A member.
    Member(MemberView<'a>),
}

macro_rules! ref_refinements {
    ($($ty:ident => $view:ident: $is:ident, $as:ident, $expect:ident;)*) => {
        $(
            #[doc = concat!("Returns `true` if this is a `", stringify!($ty), "` shape.")]
            pub fn $is(&self) -> bool {
                self.shape_type() == ShapeType::$ty
            }

            #[doc = concat!("Returns a [`", stringify!($view), "`] if this is a `", stringify!($ty), "` shape.")]
            pub fn $as(&self) -> Option<$view<'a>> {
                self.as_shape()?.$as()
            }

            #[doc = concat!("Returns a [`", stringify!($view), "`], or an error if this is not a `", stringify!($ty), "` shape.")]
            pub fn $expect(&self) -> Result<$view<'a>, ShapeExpectationError> {
                match self {
                    ShapeRef::Shape(shape) => shape.$expect(),
                    ShapeRef::Member(member) => Err(ShapeExpectationError::wrong_type(
                        member.id(),
                        ShapeType::$ty,
                        ShapeType::Member,
                    )),
                }
            }
        )*
    };
}

impl<'a> ShapeRef<'a> {
    pub(crate) fn data(&self) -> &'a ModelData {
        match self {
            ShapeRef::Shape(s) => s.data,
            ShapeRef::Member(m) => m.data,
        }
    }

    /// The shape or member ID.
    pub fn id(&self) -> &'a ShapeId {
        match self {
            ShapeRef::Shape(s) => s.id(),
            ShapeRef::Member(m) => m.id(),
        }
    }

    /// The shape type; [`ShapeType::Member`] for members.
    pub fn shape_type(&self) -> ShapeType {
        match self {
            ShapeRef::Shape(s) => s.shape_type(),
            ShapeRef::Member(_) => ShapeType::Member,
        }
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'a> {
        match self {
            ShapeRef::Shape(s) => s.traits(),
            ShapeRef::Member(m) => m.traits(),
        }
    }

    /// The value of a trait, by absolute shape ID (a [`ShapeId`] or `&str`).
    pub fn get_trait(&self, id: impl AsRef<str>) -> Option<&'a Node> {
        self.traits().get(id)
    }

    /// Returns `true` if the trait is applied.
    pub fn has_trait(&self, id: impl AsRef<str>) -> bool {
        self.traits().contains(id)
    }

    /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is not applied.
    pub fn get_trait_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
        self.traits().get_as()
    }

    /// Where the shape or member was declared.
    pub fn source(&self) -> &'a SourceLocation {
        match self {
            ShapeRef::Shape(s) => s.source(),
            ShapeRef::Member(m) => m.source(),
        }
    }

    /// The members of a root shape; empty for members.
    pub fn members(&self) -> MembersView<'a> {
        match self {
            ShapeRef::Shape(s) => s.members(),
            ShapeRef::Member(m) => MembersView {
                data: m.data,
                decl: None,
            },
        }
    }

    /// Returns `true` for a member.
    pub fn is_member(&self) -> bool {
        matches!(self, ShapeRef::Member(_))
    }

    /// The root shape, if this is not a member.
    pub fn as_shape(&self) -> Option<ShapeView<'a>> {
        match self {
            ShapeRef::Shape(s) => Some(*s),
            ShapeRef::Member(_) => None,
        }
    }

    /// The member, if this is a member.
    pub fn as_member(&self) -> Option<MemberView<'a>> {
        match self {
            ShapeRef::Member(m) => Some(*m),
            ShapeRef::Shape(_) => None,
        }
    }

    /// The root shape, or an error if this is a member.
    pub fn expect_shape(&self) -> Result<ShapeView<'a>, ShapeExpectationError> {
        match self {
            ShapeRef::Shape(s) => Ok(*s),
            ShapeRef::Member(m) => Err(ShapeExpectationError::not_root(m.id())),
        }
    }

    /// The member, or an error if this is a root shape.
    pub fn expect_member(&self) -> Result<MemberView<'a>, ShapeExpectationError> {
        match self {
            ShapeRef::Member(m) => Ok(*m),
            ShapeRef::Shape(s) => Err(ShapeExpectationError::wrong_type(
                s.id(),
                ShapeType::Member,
                s.shape_type(),
            )),
        }
    }

    all_refinements!(ref_refinements);
}

impl<'a> From<ShapeView<'a>> for ShapeRef<'a> {
    fn from(value: ShapeView<'a>) -> Self {
        ShapeRef::Shape(value)
    }
}

impl<'a> From<MemberView<'a>> for ShapeRef<'a> {
    fn from(value: MemberView<'a>) -> Self {
        ShapeRef::Member(value)
    }
}

macro_rules! refined_view {
    ($($(#[$doc:meta])* $name:ident;)*) => {
        $(
            $(#[$doc])*
            ///
            /// Common shape accessors are available directly; [`as_shape`](Self::as_shape)
            /// returns the underlying [`ShapeView`] for everything else.
            #[derive(Clone, Copy, Debug)]
            pub struct $name<'a> {
                shape: ShapeView<'a>,
            }

            impl<'a> $name<'a> {
                /// The underlying untyped shape view.
                pub fn as_shape(&self) -> ShapeView<'a> {
                    self.shape
                }

                /// The shape ID.
                pub fn id(&self) -> &'a ShapeId {
                    self.shape.id()
                }

                /// The shape type.
                pub fn shape_type(&self) -> ShapeType {
                    self.shape.shape_type()
                }

                /// The applied traits.
                pub fn traits(&self) -> Traits<'a> {
                    self.shape.traits()
                }

                /// The value of a trait, by absolute shape ID (a [`ShapeId`] or `&str`).
                pub fn get_trait(&self, id: impl AsRef<str>) -> Option<&'a Node> {
                    self.shape.get_trait(id)
                }

                /// Returns `true` if the trait is applied.
                pub fn has_trait(&self, id: impl AsRef<str>) -> bool {
                    self.shape.has_trait(id)
                }

                /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is
                /// not applied.
                pub fn get_trait_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
                    self.shape.get_trait_as()
                }

                /// Where the shape was declared.
                pub fn source(&self) -> &'a SourceLocation {
                    self.shape.source()
                }

                /// Returns `true` if this is a prelude shape.
                pub fn is_prelude(&self) -> bool {
                    self.shape.is_prelude()
                }

                /// The shape's members, uniformly (empty for services, resources, and
                /// operations).
                pub fn members(&self) -> MembersView<'a> {
                    self.shape.members()
                }
            }

            impl<'a> From<$name<'a>> for ShapeView<'a> {
                fn from(value: $name<'a>) -> Self {
                    value.shape
                }
            }

            impl<'a> From<$name<'a>> for ShapeRef<'a> {
                fn from(value: $name<'a>) -> Self {
                    ShapeRef::Shape(value.shape)
                }
            }
        )*
    };
}

refined_view! {
    /// A `list` shape.
    ListView;
    /// A `map` shape.
    MapView;
    /// A `structure` shape.
    StructureView;
    /// A `union` shape.
    UnionView;
    /// An `enum` shape.
    EnumView;
    /// An `intEnum` shape.
    IntEnumView;
    /// A `service` shape.
    ServiceView;
    /// An `operation` shape.
    OperationView;
    /// A `resource` shape.
    ResourceView;
}

macro_rules! named_member_lookup {
    ($($name:ident),*) => {
        $(
            impl<'a> $name<'a> {
                /// A member by name.
                pub fn member(&self, name: &str) -> Option<MemberView<'a>> {
                    self.shape.member(name)
                }
            }
        )*
    };
}

named_member_lookup!(StructureView, UnionView, EnumView, IntEnumView);

impl<'a> ListView<'a> {
    /// The `member` member.
    pub fn member(&self) -> MemberView<'a> {
        let DeclKind::List { member } = &self.shape.decl.kind else {
            unreachable!("ListView always wraps a list")
        };
        MemberView::new(self.shape.data, self.shape.decl, member)
    }
}

impl<'a> MapView<'a> {
    /// The `key` member.
    pub fn key(&self) -> MemberView<'a> {
        let DeclKind::Map { key, .. } = &self.shape.decl.kind else {
            unreachable!("MapView always wraps a map")
        };
        MemberView::new(self.shape.data, self.shape.decl, key)
    }

    /// The `value` member.
    pub fn value(&self) -> MemberView<'a> {
        let DeclKind::Map { value, .. } = &self.shape.decl.kind else {
            unreachable!("MapView always wraps a map")
        };
        MemberView::new(self.shape.data, self.shape.decl, value)
    }
}

impl<'a> ServiceView<'a> {
    fn decl(&self) -> &'a ServiceDecl {
        let DeclKind::Service(service) = &self.shape.decl.kind else {
            unreachable!("ServiceView always wraps a service")
        };
        service
    }

    /// The service version, if declared.
    pub fn version(&self) -> Option<&'a str> {
        self.decl().version.as_deref()
    }

    /// **Declaration-only:** operations listed directly in the service's `operations`.
    ///
    /// This omits operations bound through resources, including resource lifecycle and
    /// collection operations. Use [`contained_operations`](Self::contained_operations) for every
    /// operation in the service.
    pub fn direct_operations(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().operations.iter()
    }

    /// **Declaration-only:** resources listed directly in the service's `resources`.
    ///
    /// This omits nested child resources. Use
    /// [`contained_resources`](Self::contained_resources) for every resource in the service.
    pub fn direct_resources(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().resources.iter()
    }

    /// **Declaration-only:** the service's common `errors`, which every contained operation can
    /// also return.
    ///
    /// Use [`OperationKnowledge::errors_in`](crate::knowledge::OperationKnowledge::errors_in)
    /// for the complete errors of an operation within this service.
    pub fn common_errors(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().errors.iter()
    }

    /// The `rename` map: shape ID to its name within this service.
    pub fn rename(&self) -> impl ExactSizeIterator<Item = (&'a ShapeId, &'a str)> + 'a {
        self.decl()
            .rename
            .iter()
            .map(|(id, name)| (id, name.as_str()))
    }

    /// The renamed name of `id` within this service, if any.
    pub fn renamed(&self, id: &str) -> Option<&'a str> {
        self.decl().rename.get(id).map(String::as_str)
    }
}

impl<'a> OperationView<'a> {
    fn decl(&self) -> &'a OperationDecl {
        let DeclKind::Operation(operation) = &self.shape.decl.kind else {
            unreachable!("OperationView always wraps an operation")
        };
        operation
    }

    /// The input structure; `smithy.api#Unit` when omitted.
    pub fn input(&self) -> &'a ShapeId {
        self.decl().input.as_ref().unwrap_or_else(|| unit_id())
    }

    /// The output structure; `smithy.api#Unit` when omitted.
    pub fn output(&self) -> &'a ShapeId {
        self.decl().output.as_ref().unwrap_or_else(|| unit_id())
    }

    /// The input as declared, or `None` if the document omitted it.
    pub fn declared_input(&self) -> Option<&'a ShapeId> {
        self.decl().input.as_ref()
    }

    /// The output as declared, or `None` if the document omitted it.
    pub fn declared_output(&self) -> Option<&'a ShapeId> {
        self.decl().output.as_ref()
    }

    /// The input shape.
    pub fn input_shape(&self) -> ShapeView<'a> {
        self.resolve(self.input())
    }

    /// The output shape.
    pub fn output_shape(&self) -> ShapeView<'a> {
        self.resolve(self.output())
    }

    /// **Declaration-only:** errors listed directly in the operation's `errors`.
    ///
    /// This omits the common errors of the service the operation is used in. Use
    /// [`OperationKnowledge::errors_in`](crate::knowledge::OperationKnowledge::errors_in) for
    /// the complete errors of the operation within a service.
    pub fn direct_errors(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().errors.iter()
    }

    fn resolve(&self, id: &ShapeId) -> ShapeView<'a> {
        let decl = self
            .shape
            .data
            .root(id.as_str())
            .expect("validated models only contain resolved operation references");
        ShapeView::new(self.shape.data, decl)
    }
}

impl<'a> ResourceView<'a> {
    fn decl(&self) -> &'a ResourceDecl {
        let DeclKind::Resource(resource) = &self.shape.decl.kind else {
            unreachable!("ResourceView always wraps a resource")
        };
        resource
    }

    /// Identifier names and their target shapes.
    pub fn identifiers(&self) -> impl ExactSizeIterator<Item = (&'a str, &'a ShapeId)> + 'a {
        self.decl().identifiers.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Property names and their target shapes.
    pub fn properties(&self) -> impl ExactSizeIterator<Item = (&'a str, &'a ShapeId)> + 'a {
        self.decl().properties.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The `create` lifecycle operation.
    pub fn create(&self) -> Option<&'a ShapeId> {
        self.decl().create.as_ref()
    }

    /// The `put` lifecycle operation.
    pub fn put(&self) -> Option<&'a ShapeId> {
        self.decl().put.as_ref()
    }

    /// The `read` lifecycle operation.
    pub fn read(&self) -> Option<&'a ShapeId> {
        self.decl().read.as_ref()
    }

    /// The `update` lifecycle operation.
    pub fn update(&self) -> Option<&'a ShapeId> {
        self.decl().update.as_ref()
    }

    /// The `delete` lifecycle operation.
    pub fn delete(&self) -> Option<&'a ShapeId> {
        self.decl().delete.as_ref()
    }

    /// The `list` lifecycle operation.
    pub fn list(&self) -> Option<&'a ShapeId> {
        self.decl().list.as_ref()
    }

    /// **Declaration-only:** non-lifecycle instance operations listed in the resource's
    /// `operations`.
    ///
    /// This omits lifecycle operations, collection operations, and operations of child
    /// resources. Use [`contained_operations`](Self::contained_operations) for every operation in
    /// the resource.
    pub fn direct_operations(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().operations.iter()
    }

    /// **Declaration-only:** non-lifecycle collection operations listed in the resource's
    /// `collectionOperations`.
    ///
    /// Use [`contained_operations`](Self::contained_operations) for every operation in the
    /// resource.
    pub fn direct_collection_operations(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().collection_operations.iter()
    }

    /// **Declaration-only:** child resources listed directly in the resource's `resources`.
    ///
    /// This omits grandchildren and deeper descendants. Use
    /// [`contained_resources`](Self::contained_resources) for every nested resource.
    pub fn child_resources(&self) -> impl ExactSizeIterator<Item = &'a ShapeId> + 'a {
        self.decl().resources.iter()
    }
}

/// A shape was missing or was not of the expected type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeExpectationError {
    id: String,
    kind: ExpectationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectationKind {
    NotFound,
    NotRoot,
    WrongType {
        expected: ShapeType,
        actual: ShapeType,
    },
}

impl ShapeExpectationError {
    pub(crate) fn not_found(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            kind: ExpectationKind::NotFound,
        }
    }

    fn not_root(id: &ShapeId) -> Self {
        Self {
            id: id.to_string(),
            kind: ExpectationKind::NotRoot,
        }
    }

    fn wrong_type(id: &ShapeId, expected: ShapeType, actual: ShapeType) -> Self {
        Self {
            id: id.to_string(),
            kind: ExpectationKind::WrongType { expected, actual },
        }
    }

    /// The shape ID that was looked up.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns `true` if no shape with this ID exists.
    pub fn is_not_found(&self) -> bool {
        self.kind == ExpectationKind::NotFound
    }

    /// The expected shape type, for a type mismatch.
    pub fn expected(&self) -> Option<ShapeType> {
        match self.kind {
            ExpectationKind::WrongType { expected, .. } => Some(expected),
            _ => None,
        }
    }

    /// The actual shape type, for a type mismatch or a member that was expected to be a root.
    pub fn actual(&self) -> Option<ShapeType> {
        match self.kind {
            ExpectationKind::WrongType { actual, .. } => Some(actual),
            ExpectationKind::NotRoot => Some(ShapeType::Member),
            ExpectationKind::NotFound => None,
        }
    }
}

impl fmt::Display for ShapeExpectationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExpectationKind::NotFound => write!(f, "shape `{}` not found", self.id),
            ExpectationKind::NotRoot => write!(f, "`{}` is a member, not a root shape", self.id),
            ExpectationKind::WrongType { expected, actual } => {
                write!(
                    f,
                    "expected `{}` to be a {expected}, but it is a {actual}",
                    self.id
                )
            }
        }
    }
}

impl std::error::Error for ShapeExpectationError {}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Borrowed views of shapes in a [`Model`](crate::Model).

use super::ShapeType;
use std::fmt;

use super::owned::{
    EnumShape, IntEnumShape, ListShape, MapShape, MemberShape, OperationShape, ResourceShape,
    ServiceShape, Shape, StructureShape, TraitMap, UnionShape,
};
use crate::diagnostic::{fmt_located, SourceLocation};
use crate::model::ModelData;
use crate::node::Node;
use crate::prelude;
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
                self.$is().then_some($view { shape: *self })
            }

            #[doc = concat!("Returns a [`", stringify!($view), "`], or an error if this is not a `", stringify!($ty), "` shape.")]
            pub fn $expect(&self) -> Result<$view<'a>, ShapeExpectationError> {
                self.$as().ok_or_else(|| {
                    ShapeExpectationError::wrong_type(
                        self.id(),
                        ShapeType::$ty,
                        self.shape_type(),
                        self.source_location(),
                    )
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

/// The applied traits of a shape or member, in insertion order.
///
/// Lookups take an absolute trait ID (a [`ShapeId`] or `&str`). A relative name without a
/// namespace, such as `"required"`, resolves to the prelude (`smithy.api#required`), matching
/// Java Smithy.
#[derive(Clone, Copy)]
pub struct Traits<'a> {
    map: &'a TraitMap,
}

impl<'a> Traits<'a> {
    pub(crate) fn new(map: &'a TraitMap) -> Self {
        Self { map }
    }

    /// The number of applied traits.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if no traits are applied.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The value of a trait.
    pub fn get(&self, id: impl AsRef<str>) -> Option<&'a Node> {
        traits::find(self.map, id.as_ref()).map(|(_, value)| value)
    }

    /// Returns `true` if the trait is applied.
    pub fn contains(&self, id: impl AsRef<str>) -> bool {
        traits::find(self.map, id.as_ref()).is_some()
    }

    /// Decodes a trait with a [`TraitCodec`]. Returns `Ok(None)` if the trait is not applied.
    pub fn get_as<T: TraitCodec>(&self) -> Result<Option<T>, TraitDecodeError> {
        traits::decode(self.map.get(&T::ID))
    }

    /// Iterates applied traits.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = AppliedTrait<'a>> + 'a {
        self.map
            .iter()
            .map(|(id, value)| AppliedTrait { id, value })
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

    /// Where the trait was applied: the location of its value.
    pub fn source_location(&self) -> &'a SourceLocation {
        self.value.source_location()
    }
}

/// A root shape in a [`Model`](crate::Model).
#[derive(Clone, Copy)]
pub struct ShapeView<'a> {
    pub(crate) data: &'a ModelData,
    pub(crate) shape: &'a Shape,
}

impl<'a> ShapeView<'a> {
    pub(crate) fn new(data: &'a ModelData, shape: &'a Shape) -> Self {
        Self { data, shape }
    }

    /// The owned shape this view borrows. Clone it to build a modified copy with
    /// `to_builder`.
    pub fn shape(&self) -> &'a Shape {
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
        Traits::new(&self.shape.common().traits)
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

    /// Where the shape was declared, or [`SourceLocation::none`] for shapes built in code.
    pub fn source_location(&self) -> &'a SourceLocation {
        self.shape.source_location()
    }

    /// Returns `true` if this shape comes from the injected Smithy prelude.
    ///
    /// Without the prelude (see
    /// [`ModelAssembler::disable_prelude`](crate::ModelAssembler::disable_prelude)), a model may
    /// define its own `smithy.api` shapes; those are not prelude shapes. Use
    /// [`prelude::is_prelude_shape`] to check only the namespace.
    pub fn is_prelude(&self) -> bool {
        self.data.prelude.is_some() && prelude::is_prelude_shape(self.id())
    }

    /// The members: a list's `member`, a map's `key` and `value`, or named members in
    /// declaration order. Other shapes have no members.
    pub fn members(&self) -> MembersView<'a> {
        MembersView {
            data: self.data,
            shape: Some(self.shape),
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
    shape: Option<&'a Shape>,
}

impl<'a> MembersView<'a> {
    /// The number of members.
    pub fn len(&self) -> usize {
        self.shape.map_or(0, Shape::member_count)
    }

    /// Returns `true` if there are no members.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A member by name.
    pub fn get(&self, name: &str) -> Option<MemberView<'a>> {
        let container = self.shape?;
        let member = container.member(name)?;
        Some(MemberView::new(self.data, container, member))
    }

    /// Iterates members in order.
    pub fn iter(&self) -> impl Iterator<Item = MemberView<'a>> + 'a {
        let data = self.data;
        self.shape.into_iter().flat_map(move |container| {
            container
                .members()
                .map(move |member| MemberView::new(data, container, member))
        })
    }

    /// Iterates member names in order.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.shape
            .into_iter()
            .flat_map(|container| container.members().map(MemberShape::name))
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

/// A member of a list, map, structure, union, enum, or intEnum, within a [`Model`](crate::Model).
///
/// A `MemberView` is a read-only cursor: a `Copy` value holding three borrows (the model, the
/// containing shape, and the member). It can move to related shapes, such as
/// [`container`](Self::container) and [`target_shape`](Self::target_shape), because the model
/// is immutable and every reference resolves. It is not a zipper: it cannot replace the member
/// or rebuild the surrounding shape. To change a member, build a new one from
/// [`shape`](Self::shape) with [`MemberShape::to_builder`] and assemble a new model.
#[derive(Clone, Copy)]
pub struct MemberView<'a> {
    data: &'a ModelData,
    container: &'a Shape,
    member: &'a MemberShape,
}

impl<'a> MemberView<'a> {
    pub(crate) fn new(data: &'a ModelData, container: &'a Shape, member: &'a MemberShape) -> Self {
        Self {
            data,
            container,
            member,
        }
    }

    /// The owned member this view borrows.
    pub fn shape(&self) -> &'a MemberShape {
        self.member
    }

    /// The member ID, such as `com.example#Shape$member`.
    pub fn id(&self) -> &'a ShapeId {
        self.member.id()
    }

    /// The member name.
    pub fn name(&self) -> &'a str {
        self.member.name()
    }

    /// The target shape ID.
    pub fn target(&self) -> &'a ShapeId {
        self.member.target()
    }

    /// The target shape. Every member of a model has a resolved target.
    pub fn target_shape(&self) -> ShapeView<'a> {
        let shape = self
            .data
            .root(self.member.target.as_str())
            .expect("models only contain resolved member targets");
        ShapeView::new(self.data, shape)
    }

    /// The shape containing this member.
    pub fn container(&self) -> ShapeView<'a> {
        ShapeView::new(self.data, self.container)
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'a> {
        Traits::new(&self.member.traits)
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

    /// Where the member was declared, or [`SourceLocation::none`] for members built in code.
    pub fn source_location(&self) -> &'a SourceLocation {
        self.member.source_location()
    }

    /// The effective enum value for `enum` and `intEnum` members; `None` for other members.
    ///
    /// An `enum` member that omits `@enumValue` has its member name as its effective value. Use
    /// [`declared_enum_value`](Self::declared_enum_value) to see what was actually written.
    pub fn enum_value(&self) -> Option<EnumValue<'a>> {
        let explicit = self.member.traits.get(&traits::ENUM_VALUE);
        match self.container {
            Shape::Enum(_) => match explicit {
                Some(node) => node.as_str().map(EnumValue::String),
                None => Some(EnumValue::String(self.name())),
            },
            Shape::IntEnum(_) => explicit?.as_number()?.as_i64().map(EnumValue::Int),
            _ => None,
        }
    }

    /// The explicitly declared `@enumValue` of an `enum` or `intEnum` member, or `None` if it
    /// was omitted or this is not an enum member. Unlike [`enum_value`](Self::enum_value), an
    /// omitted `enum` value is not synthesized from the member name.
    pub fn declared_enum_value(&self) -> Option<EnumValue<'a>> {
        match self.container {
            Shape::Enum(_) | Shape::IntEnum(_) => {
                self.member.traits.get(&traits::ENUM_VALUE)?;
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
                        member.source_location(),
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

    /// Where the shape or member was declared, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &'a SourceLocation {
        match self {
            ShapeRef::Shape(s) => s.source_location(),
            ShapeRef::Member(m) => m.source_location(),
        }
    }

    /// The members of a root shape; empty for members.
    pub fn members(&self) -> MembersView<'a> {
        match self {
            ShapeRef::Shape(s) => s.members(),
            ShapeRef::Member(m) => MembersView {
                data: m.data,
                shape: None,
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
            ShapeRef::Member(m) => {
                Err(ShapeExpectationError::not_root(m.id(), m.source_location()))
            }
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
                s.source_location(),
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

                /// Where the shape was declared, or [`SourceLocation::none`].
                pub fn source_location(&self) -> &'a SourceLocation {
                    self.shape.source_location()
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

macro_rules! typed_shape {
    ($($view:ident => $variant:ident($ty:ident);)*) => {
        $(
            impl<'a> $view<'a> {
                #[doc = concat!("The owned [`", stringify!($ty), "`] this view borrows.")]
                pub fn shape(&self) -> &'a $ty {
                    match self.shape.shape {
                        Shape::$variant(shape) => shape,
                        _ => unreachable!(concat!(stringify!($view), " always wraps a ", stringify!($variant))),
                    }
                }
            }
        )*
    };
}

typed_shape! {
    ListView => List(ListShape);
    MapView => Map(MapShape);
    StructureView => Structure(StructureShape);
    UnionView => Union(UnionShape);
    EnumView => Enum(EnumShape);
    IntEnumView => IntEnum(IntEnumShape);
    ServiceView => Service(ServiceShape);
    OperationView => Operation(OperationShape);
    ResourceView => Resource(ResourceShape);
}

impl<'a> ListView<'a> {
    /// The `member` member.
    pub fn member(&self) -> MemberView<'a> {
        MemberView::new(self.shape.data, self.shape.shape, &self.shape().member)
    }
}

impl<'a> MapView<'a> {
    /// The `key` member.
    pub fn key(&self) -> MemberView<'a> {
        MemberView::new(self.shape.data, self.shape.shape, &self.shape().key)
    }

    /// The `value` member.
    pub fn value(&self) -> MemberView<'a> {
        MemberView::new(self.shape.data, self.shape.shape, &self.shape().value)
    }
}

impl<'a> ServiceView<'a> {
    fn decl(&self) -> &'a ServiceShape {
        self.shape()
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
    fn decl(&self) -> &'a OperationShape {
        self.shape()
    }

    /// The input structure; `smithy.api#Unit` when omitted.
    pub fn input(&self) -> &'a ShapeId {
        self.decl().input()
    }

    /// The output structure; `smithy.api#Unit` when omitted.
    pub fn output(&self) -> &'a ShapeId {
        self.decl().output()
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
        let shape = self
            .shape
            .data
            .root(id.as_str())
            .expect("models only contain resolved operation references");
        ShapeView::new(self.shape.data, shape)
    }
}

impl<'a> ResourceView<'a> {
    fn decl(&self) -> &'a ResourceShape {
        self.shape()
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
    location: SourceLocation,
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
            location: SourceLocation::none(),
        }
    }

    fn not_root(id: &ShapeId, location: &SourceLocation) -> Self {
        Self {
            id: id.to_string(),
            kind: ExpectationKind::NotRoot,
            location: location.clone(),
        }
    }

    fn wrong_type(
        id: &ShapeId,
        expected: ShapeType,
        actual: ShapeType,
        location: &SourceLocation,
    ) -> Self {
        Self {
            id: id.to_string(),
            kind: ExpectationKind::WrongType { expected, actual },
            location: location.clone(),
        }
    }

    /// Where the shape that was found is defined, or [`SourceLocation::none`] if no shape was
    /// found or it was built in code.
    pub fn source_location(&self) -> &SourceLocation {
        &self.location
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
        let id = &self.id;
        match &self.kind {
            ExpectationKind::NotFound => write!(f, "shape `{id}` not found"),
            ExpectationKind::NotRoot => fmt_located(
                f,
                &self.location,
                format_args!("`{id}` is a member, not a root shape"),
            ),
            ExpectationKind::WrongType { expected, actual } => fmt_located(
                f,
                &self.location,
                format_args!("expected `{id}` to be a {expected}, but it is a {actual}"),
            ),
        }
    }
}

impl std::error::Error for ShapeExpectationError {}

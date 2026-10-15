/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Owned semantic shapes and their builders.
//!
//! These types are the model's storage: the JSON AST loader, [`ModelAssembler::add_shape`],
//! and [`Model`] views all use them. Shapes are plain values; references to other shapes are
//! [`ShapeId`]s that resolve only once the shapes are assembled into a [`Model`].
//!
//! Equality ignores source locations. Member order is significant, while unordered
//! collections, such as a service's operations and a resource's identifiers, compare as sets
//! and maps. An omitted operation input or output equals an explicit `smithy.api#Unit`, and an
//! `enum` member without `@enumValue` equals one whose value is its member name.
//!
//! [`ModelAssembler::add_shape`]: crate::ModelAssembler::add_shape
//! [`Model`]: crate::Model

use super::view::Traits;
use super::ShapeType;
use crate::diagnostic::{fmt_located, SourceLocation};
use crate::node::Node;
use crate::prelude;
use crate::shape_id::{IntoShapeId, InvalidShapeIdError, ShapeId};
use crate::traits;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::fmt;

/// Applied traits, keyed by absolute trait shape ID, in insertion order.
pub(crate) type TraitMap = IndexMap<ShapeId, Node>;

/// Named members keyed by member name, in declaration order.
pub(crate) type Members = IndexMap<String, MemberShape>;

/// Data common to every root shape.
#[derive(Debug, Clone)]
pub(crate) struct Common {
    pub(crate) id: ShapeId,
    pub(crate) traits: TraitMap,
    pub(crate) source: SourceLocation,
}

impl PartialEq for Common {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.traits == other.traits
    }
}

/// Compares unordered reference sets.
fn same_set(a: &[ShapeId], b: &[ShapeId]) -> bool {
    a.len() == b.len() && {
        let a: HashSet<&ShapeId> = a.iter().collect();
        b.iter().all(|id| a.contains(id))
    }
}

/// Compares named members in order.
fn same_members(a: &Members, b: &Members) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a == b)
}

/// Compares enum members, treating an omitted `@enumValue` as the member name.
fn same_enum_members(a: &Members, b: &Members) -> bool {
    fn effective(member: &MemberShape) -> Node {
        match member.traits.get(&traits::ENUM_VALUE) {
            Some(value) => value.clone(),
            None => Node::from(member.name()),
        }
    }
    fn without_value(member: &MemberShape) -> impl Iterator<Item = (&ShapeId, &Node)> {
        member
            .traits
            .iter()
            .filter(|(id, _)| **id != traits::ENUM_VALUE)
    }
    a.len() == b.len()
        && a.iter().zip(b).all(|((name_a, a), (name_b, b))| {
            name_a == name_b
                && a.id == b.id
                && a.target == b.target
                && effective(a) == effective(b)
                && without_value(a).count() == without_value(b).count()
                && without_value(a).all(|(id, value)| b.traits.get(id) == Some(value))
        })
}

fn push_unique(ids: &mut Vec<ShapeId>, id: ShapeId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

// ---------------------------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------------------------

/// An owned root shape of any kind.
///
/// Every variant wraps a dedicated type with its own builder. Common accessors, such as
/// [`id`](Self::id) and [`traits`](Self::traits), are available directly; match on the variant,
/// or use `as_*`, for kind-specific data.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// `blob`, `boolean`, `document`, `string`, `timestamp`, and the numeric shapes.
    Simple(SimpleShape),
    /// `list`
    List(ListShape),
    /// `map`
    Map(MapShape),
    /// `structure`
    Structure(StructureShape),
    /// `union`
    Union(UnionShape),
    /// `enum`
    Enum(EnumShape),
    /// `intEnum`
    IntEnum(IntEnumShape),
    /// `service`
    Service(ServiceShape),
    /// `operation`
    Operation(OperationShape),
    /// `resource`
    Resource(ResourceShape),
}

macro_rules! dispatch {
    ($self:expr, $s:ident => $e:expr) => {
        match $self {
            Shape::Simple($s) => $e,
            Shape::List($s) => $e,
            Shape::Map($s) => $e,
            Shape::Structure($s) => $e,
            Shape::Union($s) => $e,
            Shape::Enum($s) => $e,
            Shape::IntEnum($s) => $e,
            Shape::Service($s) => $e,
            Shape::Operation($s) => $e,
            Shape::Resource($s) => $e,
        }
    };
}

macro_rules! shape_as {
    ($($variant:ident($ty:ident) => $as:ident;)*) => {
        $(
            #[doc = concat!("The [`", stringify!($ty), "`], if this is one.")]
            pub fn $as(&self) -> Option<&$ty> {
                match self {
                    Shape::$variant(shape) => Some(shape),
                    _ => None,
                }
            }
        )*
    };
}

impl Shape {
    pub(crate) fn common(&self) -> &Common {
        dispatch!(self, s => &s.common)
    }

    pub(crate) fn common_mut(&mut self) -> &mut Common {
        dispatch!(self, s => &mut s.common)
    }

    /// The shape ID.
    pub fn id(&self) -> &ShapeId {
        &self.common().id
    }

    /// The shape type.
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Shape::Simple(s) => s.shape_type,
            Shape::List(_) => ShapeType::List,
            Shape::Map(_) => ShapeType::Map,
            Shape::Structure(_) => ShapeType::Structure,
            Shape::Union(_) => ShapeType::Union,
            Shape::Enum(_) => ShapeType::Enum,
            Shape::IntEnum(_) => ShapeType::IntEnum,
            Shape::Service(_) => ShapeType::Service,
            Shape::Operation(_) => ShapeType::Operation,
            Shape::Resource(_) => ShapeType::Resource,
        }
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'_> {
        Traits::new(&self.common().traits)
    }

    /// Where the shape was declared, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &SourceLocation {
        &self.common().source
    }

    /// Returns `true` if this shape is in the prelude namespace.
    pub fn is_prelude(&self) -> bool {
        prelude::is_prelude_shape(self.id())
    }

    /// The members in order: a list's `member`, a map's `key` then `value`, or named members.
    /// Other shapes have no members.
    pub fn members(&self) -> MemberIter<'_> {
        match self {
            Shape::List(s) => {
                MemberIter(MemberIterInner::Fixed([Some(&s.member), None].into_iter()))
            }
            Shape::Map(s) => MemberIter(MemberIterInner::Fixed(
                [Some(&s.key), Some(&s.value)].into_iter(),
            )),
            _ => MemberIter(match self.named_members() {
                Some(m) => MemberIterInner::Named(m.values()),
                None => MemberIterInner::Fixed([None, None].into_iter()),
            }),
        }
    }

    /// A member by name.
    pub fn member(&self, name: &str) -> Option<&MemberShape> {
        match self {
            Shape::List(s) => (name == "member").then_some(&s.member),
            Shape::Map(s) => match name {
                "key" => Some(&s.key),
                "value" => Some(&s.value),
                _ => None,
            },
            _ => self.named_members()?.get(name),
        }
    }

    pub(crate) fn named_members(&self) -> Option<&Members> {
        match self {
            Shape::Structure(s) => Some(&s.members),
            Shape::Union(s) => Some(&s.members),
            Shape::Enum(s) => Some(&s.members),
            Shape::IntEnum(s) => Some(&s.members),
            _ => None,
        }
    }

    pub(crate) fn members_mut(&mut self) -> Vec<&mut MemberShape> {
        match self {
            Shape::List(s) => vec![&mut s.member],
            Shape::Map(s) => vec![&mut s.key, &mut s.value],
            Shape::Structure(s) => s.members.values_mut().collect(),
            Shape::Union(s) => s.members.values_mut().collect(),
            Shape::Enum(s) => s.members.values_mut().collect(),
            Shape::IntEnum(s) => s.members.values_mut().collect(),
            _ => Vec::new(),
        }
    }

    /// The position of a member in [`members`](Self::members) order.
    pub(crate) fn member_position(&self, name: &str) -> Option<usize> {
        match self {
            Shape::List(_) => (name == "member").then_some(0),
            Shape::Map(_) => match name {
                "key" => Some(0),
                "value" => Some(1),
                _ => None,
            },
            _ => self.named_members()?.get_index_of(name),
        }
    }

    /// The member at a position in [`members`](Self::members) order.
    pub(crate) fn member_at(&self, position: usize) -> Option<&MemberShape> {
        match self {
            Shape::List(s) => (position == 0).then_some(&s.member),
            Shape::Map(s) => [&s.key, &s.value].get(position).copied(),
            _ => self
                .named_members()?
                .get_index(position)
                .map(|(_, member)| member),
        }
    }

    pub(crate) fn member_count(&self) -> usize {
        match self {
            Shape::List(_) => 1,
            Shape::Map(_) => 2,
            _ => self.named_members().map_or(0, IndexMap::len),
        }
    }

    shape_as! {
        Simple(SimpleShape) => as_simple;
        List(ListShape) => as_list;
        Map(MapShape) => as_map;
        Structure(StructureShape) => as_structure;
        Union(UnionShape) => as_union;
        Enum(EnumShape) => as_enum;
        IntEnum(IntEnumShape) => as_int_enum;
        Service(ServiceShape) => as_service;
        Operation(OperationShape) => as_operation;
        Resource(ResourceShape) => as_resource;
    }
}

/// An iterator over the members of an owned shape, in order.
#[derive(Clone)]
pub struct MemberIter<'a>(MemberIterInner<'a>);

#[derive(Clone)]
enum MemberIterInner<'a> {
    Fixed(std::array::IntoIter<Option<&'a MemberShape>, 2>),
    Named(indexmap::map::Values<'a, String, MemberShape>),
}

impl<'a> Iterator for MemberIter<'a> {
    type Item = &'a MemberShape;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            MemberIterInner::Fixed(it) => it.by_ref().flatten().next(),
            MemberIterInner::Named(it) => it.next(),
        }
    }
}

impl fmt::Debug for MemberIter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.clone().map(MemberShape::name))
            .finish()
    }
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// A shape builder was given invalid input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeBuildError {
    message: String,
    source: Option<InvalidShapeIdError>,
    location: SourceLocation,
}

impl ShapeBuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
            location: SourceLocation::none(),
        }
    }

    fn invalid_id(source: InvalidShapeIdError) -> Self {
        Self {
            message: source.to_string(),
            source: Some(source),
            location: SourceLocation::none(),
        }
    }

    fn at(mut self, location: SourceLocation) -> Self {
        self.location = location;
        self
    }

    /// A description of the problem.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The builder's [source location](crate::SourceLocation), such as the location of the
    /// shape it was created from with `to_builder`, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &SourceLocation {
        &self.location
    }
}

impl fmt::Display for ShapeBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_located(
            f,
            &self.location,
            format_args!("invalid shape: {}", self.message),
        )
    }
}

impl std::error::Error for ShapeBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

/// State shared by every builder: the ID, traits, location, and the first error.
#[derive(Debug, Clone)]
struct BuilderCommon {
    id: Option<ShapeId>,
    traits: TraitMap,
    source: SourceLocation,
    error: Option<ShapeBuildError>,
}

impl BuilderCommon {
    fn new(id: impl IntoShapeId) -> Self {
        let mut this = Self {
            id: None,
            traits: TraitMap::new(),
            source: SourceLocation::none(),
            error: None,
        };
        match id.into_shape_id() {
            Ok(id) if id.is_member() => this.fail(ShapeBuildError::new(format!(
                "`{id}` is a member ID; root shapes need a root shape ID"
            ))),
            Ok(id) => this.id = Some(id),
            Err(err) => this.fail(ShapeBuildError::invalid_id(err)),
        }
        this
    }

    fn from_common(common: &Common) -> Self {
        Self {
            id: Some(common.id.clone()),
            traits: common.traits.clone(),
            source: common.source.clone(),
            error: None,
        }
    }

    fn fail(&mut self, error: ShapeBuildError) {
        self.error.get_or_insert(error);
    }

    fn shape_id(&mut self, id: impl IntoShapeId) -> Option<ShapeId> {
        match id.into_shape_id() {
            Ok(id) => Some(id),
            Err(err) => {
                self.fail(ShapeBuildError::invalid_id(err));
                None
            }
        }
    }

    fn add_trait(&mut self, id: impl IntoShapeId, value: Node) {
        if let Some(id) = self.shape_id(id) {
            self.traits.insert(id, value);
        }
    }

    fn remove_trait(&mut self, id: &str) {
        self.traits.shift_remove(id);
    }

    /// The ID of a member of this shape.
    fn member_id(&mut self, name: &str) -> Option<ShapeId> {
        let id = self.id.as_ref()?;
        match id.with_member(name) {
            Ok(id) => Some(id),
            Err(err) => {
                self.fail(ShapeBuildError::invalid_id(err));
                None
            }
        }
    }

    /// Checks that `member` belongs to this shape.
    fn check_member(&mut self, member: &MemberShape) -> bool {
        let Some(id) = &self.id else { return false };
        if member.id.root() != *id {
            self.fail(ShapeBuildError::new(format!(
                "member `{}` does not belong to `{id}`",
                member.id
            )));
            return false;
        }
        true
    }

    fn finish(self) -> Result<Common, ShapeBuildError> {
        if let Some(error) = self.error {
            return Err(error.at(self.source));
        }
        Ok(Common {
            id: self.id.expect("an ID is present when there is no error"),
            traits: self.traits,
            source: self.source,
        })
    }
}

macro_rules! common_builder_methods {
    () => {
        /// Applies a trait, replacing any existing value for the same trait.
        pub fn add_trait(mut self, id: impl IntoShapeId, value: impl Into<Node>) -> Self {
            self.common.add_trait(id, value.into());
            self
        }

        /// Removes a trait by absolute shape ID.
        pub fn remove_trait(mut self, id: impl AsRef<str>) -> Self {
            self.common.remove_trait(id.as_ref());
            self
        }

        /// Removes every trait.
        pub fn clear_traits(mut self) -> Self {
            self.common.traits.clear();
            self
        }

        /// Sets the source location. Shapes built in code default to [`SourceLocation::none`].
        pub fn source_location(mut self, location: SourceLocation) -> Self {
            self.common.source = location;
            self
        }
    };
}

macro_rules! common_accessors {
    () => {
        /// The shape ID.
        pub fn id(&self) -> &ShapeId {
            &self.common.id
        }

        /// The applied traits.
        pub fn traits(&self) -> Traits<'_> {
            Traits::new(&self.common.traits)
        }

        /// Where the shape was declared, or [`SourceLocation::none`].
        pub fn source_location(&self) -> &SourceLocation {
            &self.common.source
        }
    };
}

// ---------------------------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------------------------

/// A member of a list, map, structure, union, enum, or intEnum.
///
/// ```
/// use aws_smithy_lang::shape::{MemberShape, StructureShape};
/// use aws_smithy_lang::{traits, NodeObject};
///
/// let name = MemberShape::builder("ex#Input$name", "smithy.api#String")
///     .add_trait(traits::REQUIRED, NodeObject::new())
///     .build()?;
/// let input = StructureShape::builder("ex#Input").add_member(name).build()?;
/// assert!(input.member("name").unwrap().traits().contains(traits::REQUIRED));
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct MemberShape {
    /// `Container$name`
    pub(crate) id: ShapeId,
    pub(crate) target: ShapeId,
    pub(crate) traits: TraitMap,
    pub(crate) source: SourceLocation,
}

impl MemberShape {
    /// A builder for a member with ID `Container$name` targeting `target`.
    pub fn builder(id: impl IntoShapeId, target: impl IntoShapeId) -> MemberShapeBuilder {
        let mut builder = MemberShapeBuilder {
            id: None,
            target: None,
            traits: TraitMap::new(),
            source: SourceLocation::none(),
            error: None,
        };
        match id.into_shape_id() {
            Ok(id) if id.is_member() => builder.id = Some(id),
            Ok(id) => builder.fail(ShapeBuildError::new(format!(
                "`{id}` is not a member ID (`Container$name`)"
            ))),
            Err(err) => builder.fail(ShapeBuildError::invalid_id(err)),
        }
        builder.target(target)
    }

    /// The member ID, such as `com.example#Shape$member`.
    pub fn id(&self) -> &ShapeId {
        &self.id
    }

    /// The member name.
    pub fn name(&self) -> &str {
        self.id
            .member()
            .expect("member IDs always have a member component")
    }

    /// The target shape ID.
    pub fn target(&self) -> &ShapeId {
        &self.target
    }

    /// The applied traits.
    pub fn traits(&self) -> Traits<'_> {
        Traits::new(&self.traits)
    }

    /// Where the member was declared, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &SourceLocation {
        &self.source
    }

    /// A builder initialized with this member.
    pub fn to_builder(&self) -> MemberShapeBuilder {
        MemberShapeBuilder {
            id: Some(self.id.clone()),
            target: Some(self.target.clone()),
            traits: self.traits.clone(),
            source: self.source.clone(),
            error: None,
        }
    }
}

impl PartialEq for MemberShape {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.target == other.target && self.traits == other.traits
    }
}

impl Eq for MemberShape {}

/// Builds a [`MemberShape`].
#[derive(Debug, Clone)]
pub struct MemberShapeBuilder {
    id: Option<ShapeId>,
    target: Option<ShapeId>,
    traits: TraitMap,
    source: SourceLocation,
    error: Option<ShapeBuildError>,
}

impl MemberShapeBuilder {
    fn fail(&mut self, error: ShapeBuildError) {
        self.error.get_or_insert(error);
    }

    /// Sets the target shape.
    pub fn target(mut self, target: impl IntoShapeId) -> Self {
        match target.into_shape_id() {
            Ok(target) => self.target = Some(target),
            Err(err) => self.fail(ShapeBuildError::invalid_id(err)),
        }
        self
    }

    /// Applies a trait, replacing any existing value for the same trait.
    pub fn add_trait(mut self, id: impl IntoShapeId, value: impl Into<Node>) -> Self {
        match id.into_shape_id() {
            Ok(id) => {
                self.traits.insert(id, value.into());
            }
            Err(err) => self.fail(ShapeBuildError::invalid_id(err)),
        }
        self
    }

    /// Removes a trait by absolute shape ID.
    pub fn remove_trait(mut self, id: impl AsRef<str>) -> Self {
        self.traits.shift_remove(id.as_ref());
        self
    }

    /// Sets the source location. Members built in code default to [`SourceLocation::none`].
    pub fn source_location(mut self, location: SourceLocation) -> Self {
        self.source = location;
        self
    }

    /// Builds the member.
    pub fn build(self) -> Result<MemberShape, ShapeBuildError> {
        if let Some(error) = self.error {
            return Err(error.at(self.source));
        }
        Ok(MemberShape {
            id: self.id.expect("an ID is present when there is no error"),
            target: self
                .target
                .expect("a target is present when there is no error"),
            traits: self.traits,
            source: self.source,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Simple shapes
// ---------------------------------------------------------------------------------------------

/// A `blob`, `boolean`, `document`, `string`, `timestamp`, or numeric shape.
///
/// ```
/// use aws_smithy_lang::shape::{ShapeType, SimpleShape};
///
/// let name = SimpleShape::builder(ShapeType::String, "ex#Name").build()?;
/// assert_eq!(name.shape_type(), ShapeType::String);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleShape {
    pub(crate) common: Common,
    pub(crate) shape_type: ShapeType,
}

impl Eq for Common {}

impl SimpleShape {
    /// A builder for a simple shape. `shape_type` must be a
    /// [simple type](ShapeType::is_simple).
    pub fn builder(shape_type: ShapeType, id: impl IntoShapeId) -> SimpleShapeBuilder {
        let mut common = BuilderCommon::new(id);
        if !shape_type.is_simple() {
            common.fail(ShapeBuildError::new(format!(
                "`{shape_type}` is not a simple shape type"
            )));
        }
        SimpleShapeBuilder { common, shape_type }
    }

    common_accessors!();

    /// The shape type.
    pub fn shape_type(&self) -> ShapeType {
        self.shape_type
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> SimpleShapeBuilder {
        SimpleShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            shape_type: self.shape_type,
        }
    }
}

/// Builds a [`SimpleShape`].
#[derive(Debug, Clone)]
pub struct SimpleShapeBuilder {
    common: BuilderCommon,
    shape_type: ShapeType,
}

impl SimpleShapeBuilder {
    common_builder_methods!();

    /// Builds the shape.
    pub fn build(self) -> Result<SimpleShape, ShapeBuildError> {
        Ok(SimpleShape {
            common: self.common.finish()?,
            shape_type: self.shape_type,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Lists and maps
// ---------------------------------------------------------------------------------------------

/// A `list` shape.
///
/// ```
/// use aws_smithy_lang::shape::ListShape;
///
/// let names = ListShape::builder("ex#Names").member("smithy.api#String").build()?;
/// assert_eq!(names.member().id(), "ex#Names$member");
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListShape {
    pub(crate) common: Common,
    pub(crate) member: MemberShape,
}

impl ListShape {
    /// A builder for a list. Set its member with [`ListShapeBuilder::member`] or
    /// [`ListShapeBuilder::add_member`].
    pub fn builder(id: impl IntoShapeId) -> ListShapeBuilder {
        ListShapeBuilder {
            common: BuilderCommon::new(id),
            member: None,
        }
    }

    common_accessors!();

    /// The `member` member.
    pub fn member(&self) -> &MemberShape {
        &self.member
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> ListShapeBuilder {
        ListShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            member: Some(self.member.clone()),
        }
    }
}

/// Builds a [`ListShape`].
#[derive(Debug, Clone)]
pub struct ListShapeBuilder {
    common: BuilderCommon,
    member: Option<MemberShape>,
}

impl ListShapeBuilder {
    common_builder_methods!();

    /// Sets the `member` member's target, replacing any existing member.
    pub fn member(mut self, target: impl IntoShapeId) -> Self {
        let id = self.common.member_id("member");
        let target = self.common.shape_id(target);
        if let (Some(id), Some(target)) = (id, target) {
            self.member = Some(MemberShape {
                id,
                target,
                traits: TraitMap::new(),
                source: SourceLocation::none(),
            });
        }
        self
    }

    /// Sets the `member` member. Its ID must be `List$member`.
    pub fn add_member(mut self, member: MemberShape) -> Self {
        if self.common.check_member(&member) {
            if member.name() == "member" {
                self.member = Some(member);
            } else {
                self.common.fail(ShapeBuildError::new(format!(
                    "a list's only member is `member`, not `{}`",
                    member.name()
                )));
            }
        }
        self
    }

    /// Builds the shape.
    pub fn build(mut self) -> Result<ListShape, ShapeBuildError> {
        if self.member.is_none() {
            self.common
                .fail(ShapeBuildError::new("a list requires a `member`"));
        }
        let common = self.common.finish()?;
        Ok(ListShape {
            common,
            member: self.member.expect("checked above"),
        })
    }
}

/// A `map` shape.
///
/// ```
/// use aws_smithy_lang::shape::MapShape;
///
/// let tags = MapShape::builder("ex#Tags")
///     .key("smithy.api#String")
///     .value("smithy.api#String")
///     .build()?;
/// assert_eq!(tags.value().id(), "ex#Tags$value");
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapShape {
    pub(crate) common: Common,
    pub(crate) key: MemberShape,
    pub(crate) value: MemberShape,
}

impl MapShape {
    /// A builder for a map. Set its members with [`MapShapeBuilder::key`] and
    /// [`MapShapeBuilder::value`], or [`MapShapeBuilder::add_member`].
    pub fn builder(id: impl IntoShapeId) -> MapShapeBuilder {
        MapShapeBuilder {
            common: BuilderCommon::new(id),
            key: None,
            value: None,
        }
    }

    common_accessors!();

    /// The `key` member.
    pub fn key(&self) -> &MemberShape {
        &self.key
    }

    /// The `value` member.
    pub fn value(&self) -> &MemberShape {
        &self.value
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> MapShapeBuilder {
        MapShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            key: Some(self.key.clone()),
            value: Some(self.value.clone()),
        }
    }
}

/// Builds a [`MapShape`].
#[derive(Debug, Clone)]
pub struct MapShapeBuilder {
    common: BuilderCommon,
    key: Option<MemberShape>,
    value: Option<MemberShape>,
}

impl MapShapeBuilder {
    common_builder_methods!();

    fn simple_member(&mut self, name: &str, target: impl IntoShapeId) -> Option<MemberShape> {
        let id = self.common.member_id(name);
        let target = self.common.shape_id(target);
        Some(MemberShape {
            id: id?,
            target: target?,
            traits: TraitMap::new(),
            source: SourceLocation::none(),
        })
    }

    /// Sets the `key` member's target.
    pub fn key(mut self, target: impl IntoShapeId) -> Self {
        if let Some(member) = self.simple_member("key", target) {
            self.key = Some(member);
        }
        self
    }

    /// Sets the `value` member's target.
    pub fn value(mut self, target: impl IntoShapeId) -> Self {
        if let Some(member) = self.simple_member("value", target) {
            self.value = Some(member);
        }
        self
    }

    /// Sets the `key` or `value` member. Its ID must be `Map$key` or `Map$value`.
    pub fn add_member(mut self, member: MemberShape) -> Self {
        if self.common.check_member(&member) {
            match member.name() {
                "key" => self.key = Some(member),
                "value" => self.value = Some(member),
                other => self.common.fail(ShapeBuildError::new(format!(
                    "a map's members are `key` and `value`, not `{other}`"
                ))),
            }
        }
        self
    }

    /// Builds the shape.
    pub fn build(mut self) -> Result<MapShape, ShapeBuildError> {
        if self.key.is_none() || self.value.is_none() {
            self.common
                .fail(ShapeBuildError::new("a map requires `key` and `value`"));
        }
        let common = self.common.finish()?;
        Ok(MapShape {
            common,
            key: self.key.expect("checked above"),
            value: self.value.expect("checked above"),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Named-member shapes
// ---------------------------------------------------------------------------------------------

macro_rules! named_member_shape {
    (
        $(#[$doc:meta])*
        $name:ident, $builder:ident, $eq:path
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone)]
        pub struct $name {
            pub(crate) common: Common,
            pub(crate) members: Members,
        }

        impl $name {
            #[doc = concat!("A builder for a [`", stringify!($name), "`].")]
            pub fn builder(id: impl IntoShapeId) -> $builder {
                $builder {
                    common: BuilderCommon::new(id),
                    members: Members::new(),
                }
            }

            common_accessors!();

            /// The members, in declaration order.
            pub fn members(&self) -> impl ExactSizeIterator<Item = &MemberShape> + '_ {
                self.members.values()
            }

            /// A member by name.
            pub fn member(&self, name: &str) -> Option<&MemberShape> {
                self.members.get(name)
            }

            /// A builder initialized with this shape.
            pub fn to_builder(&self) -> $builder {
                $builder {
                    common: BuilderCommon::from_common(&self.common),
                    members: self.members.clone(),
                }
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.common == other.common && $eq(&self.members, &other.members)
            }
        }

        impl Eq for $name {}

        #[doc = concat!("Builds a [`", stringify!($name), "`]. Members keep insertion order.")]
        #[derive(Debug, Clone)]
        pub struct $builder {
            common: BuilderCommon,
            members: Members,
        }

        impl $builder {
            common_builder_methods!();

            /// Adds a member, or replaces the member with the same name. Its ID must belong to
            /// this shape.
            pub fn add_member(mut self, member: MemberShape) -> Self {
                if self.common.check_member(&member) {
                    self.members.insert(member.name().to_owned(), member);
                }
                self
            }

            /// Removes a member by name, preserving the order of the others.
            pub fn remove_member(mut self, name: &str) -> Self {
                self.members.shift_remove(name);
                self
            }

            fn named(mut self, name: &str, target: impl IntoShapeId, value: Option<Node>) -> Self {
                let id = self.common.member_id(name);
                let target = self.common.shape_id(target);
                if let (Some(id), Some(target)) = (id, target) {
                    let mut traits = TraitMap::new();
                    if let Some(value) = value {
                        traits.insert(traits::ENUM_VALUE, value);
                    }
                    let member = MemberShape {
                        id,
                        target,
                        traits,
                        source: SourceLocation::none(),
                    };
                    self.members.insert(name.to_owned(), member);
                }
                self
            }

            /// Builds the shape.
            pub fn build(self) -> Result<$name, ShapeBuildError> {
                Ok($name {
                    common: self.common.finish()?,
                    members: self.members,
                })
            }
        }
    };
}

named_member_shape!(
    /// A `structure` shape.
    ///
    /// ```
    /// use aws_smithy_lang::shape::StructureShape;
    ///
    /// let input = StructureShape::builder("ex#Input")
    ///     .member("name", "smithy.api#String")
    ///     .member("size", "smithy.api#Integer")
    ///     .build()?;
    /// let names: Vec<_> = input.members().map(|m| m.name()).collect();
    /// assert_eq!(names, ["name", "size"]);
    /// # Ok::<_, Box<dyn std::error::Error>>(())
    /// ```
    StructureShape, StructureShapeBuilder, same_members
);
named_member_shape!(
    /// A `union` shape.
    UnionShape, UnionShapeBuilder, same_members
);
named_member_shape!(
    /// An `enum` shape. Every member targets `smithy.api#Unit`.
    ///
    /// ```
    /// use aws_smithy_lang::shape::EnumShape;
    ///
    /// let color = EnumShape::builder("ex#Color")
    ///     .variant("RED")
    ///     .enum_value("GREEN", "green")
    ///     .build()?;
    /// assert_eq!(color.members().len(), 2);
    /// # Ok::<_, Box<dyn std::error::Error>>(())
    /// ```
    EnumShape, EnumShapeBuilder, same_enum_members
);
named_member_shape!(
    /// An `intEnum` shape. Every member targets `smithy.api#Unit` and has an integer
    /// `@enumValue`.
    IntEnumShape, IntEnumShapeBuilder, same_members
);

impl StructureShapeBuilder {
    /// Adds a member named `name` targeting `target`, or replaces the member with that name.
    pub fn member(self, name: &str, target: impl IntoShapeId) -> Self {
        self.named(name, target, None)
    }
}

impl UnionShapeBuilder {
    /// Adds a member named `name` targeting `target`, or replaces the member with that name.
    pub fn member(self, name: &str, target: impl IntoShapeId) -> Self {
        self.named(name, target, None)
    }
}

impl EnumShapeBuilder {
    /// Adds a member whose value is its name (no explicit `@enumValue`).
    pub fn variant(self, name: &str) -> Self {
        self.named(name, &prelude::UNIT, None)
    }

    /// Adds a member with an explicit string `@enumValue`.
    pub fn enum_value(self, name: &str, value: impl Into<String>) -> Self {
        self.named(name, &prelude::UNIT, Some(Node::from(value.into())))
    }
}

impl IntEnumShapeBuilder {
    /// Adds a member with an integer `@enumValue`.
    pub fn enum_value(self, name: &str, value: i32) -> Self {
        self.named(name, &prelude::UNIT, Some(Node::from(i64::from(value))))
    }
}

// ---------------------------------------------------------------------------------------------
// Service shapes
// ---------------------------------------------------------------------------------------------

/// A `service` shape.
///
/// ```
/// use aws_smithy_lang::shape::ServiceShape;
///
/// let service = ServiceShape::builder("ex#Weather")
///     .version("2006-03-01")
///     .add_operation("ex#GetForecast")
///     .add_resource("ex#City")
///     .build()?;
/// assert_eq!(service.direct_operations().len(), 1);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct ServiceShape {
    pub(crate) common: Common,
    pub(crate) version: Option<String>,
    pub(crate) operations: Vec<ShapeId>,
    pub(crate) resources: Vec<ShapeId>,
    pub(crate) errors: Vec<ShapeId>,
    pub(crate) rename: IndexMap<ShapeId, String>,
}

impl ServiceShape {
    /// A builder for a service.
    pub fn builder(id: impl IntoShapeId) -> ServiceShapeBuilder {
        ServiceShapeBuilder {
            common: BuilderCommon::new(id),
            version: None,
            operations: Vec::new(),
            resources: Vec::new(),
            errors: Vec::new(),
            rename: IndexMap::new(),
        }
    }

    common_accessors!();

    /// The service version, if declared.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// **Declaration-only:** operations listed directly in the service's `operations`.
    pub fn direct_operations(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.operations.iter()
    }

    /// **Declaration-only:** resources listed directly in the service's `resources`.
    pub fn direct_resources(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.resources.iter()
    }

    /// The service's common `errors`, which every contained operation can also return.
    pub fn common_errors(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.errors.iter()
    }

    /// The `rename` map: shape ID to its name within this service.
    pub fn rename(&self) -> impl ExactSizeIterator<Item = (&ShapeId, &str)> + '_ {
        self.rename.iter().map(|(id, name)| (id, name.as_str()))
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> ServiceShapeBuilder {
        ServiceShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            version: self.version.clone(),
            operations: self.operations.clone(),
            resources: self.resources.clone(),
            errors: self.errors.clone(),
            rename: self.rename.clone(),
        }
    }
}

impl PartialEq for ServiceShape {
    fn eq(&self, other: &Self) -> bool {
        self.common == other.common
            && self.version == other.version
            && same_set(&self.operations, &other.operations)
            && same_set(&self.resources, &other.resources)
            && same_set(&self.errors, &other.errors)
            && self.rename == other.rename
    }
}

impl Eq for ServiceShape {}

/// Builds a [`ServiceShape`]. Reference sets keep insertion order and ignore repeats.
#[derive(Debug, Clone)]
pub struct ServiceShapeBuilder {
    common: BuilderCommon,
    version: Option<String>,
    operations: Vec<ShapeId>,
    resources: Vec<ShapeId>,
    errors: Vec<ShapeId>,
    rename: IndexMap<ShapeId, String>,
}

impl ServiceShapeBuilder {
    common_builder_methods!();

    /// Sets the service version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Binds an operation directly to the service.
    pub fn add_operation(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.operations, id);
        }
        self
    }

    /// Binds a resource directly to the service.
    pub fn add_resource(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.resources, id);
        }
        self
    }

    /// Adds a common error.
    pub fn add_error(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.errors, id);
        }
        self
    }

    /// Renames `id` to `name` within the service.
    pub fn rename(mut self, id: impl IntoShapeId, name: impl Into<String>) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            self.rename.insert(id, name.into());
        }
        self
    }

    /// Builds the shape.
    pub fn build(self) -> Result<ServiceShape, ShapeBuildError> {
        Ok(ServiceShape {
            common: self.common.finish()?,
            version: self.version,
            operations: self.operations,
            resources: self.resources,
            errors: self.errors,
            rename: self.rename,
        })
    }
}

/// An `operation` shape.
///
/// ```
/// use aws_smithy_lang::shape::OperationShape;
///
/// let op = OperationShape::builder("ex#Ping").build()?;
/// // An omitted input or output is `smithy.api#Unit`.
/// assert_eq!(op.input(), "smithy.api#Unit");
/// assert!(op.declared_input().is_none());
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct OperationShape {
    pub(crate) common: Common,
    /// `None` when omitted, which means `smithy.api#Unit`.
    pub(crate) input: Option<ShapeId>,
    pub(crate) output: Option<ShapeId>,
    pub(crate) errors: Vec<ShapeId>,
}

impl OperationShape {
    /// A builder for an operation. Input and output default to `smithy.api#Unit`.
    pub fn builder(id: impl IntoShapeId) -> OperationShapeBuilder {
        OperationShapeBuilder {
            common: BuilderCommon::new(id),
            input: None,
            output: None,
            errors: Vec::new(),
        }
    }

    common_accessors!();

    /// The input structure; `smithy.api#Unit` when omitted.
    pub fn input(&self) -> &ShapeId {
        self.input.as_ref().unwrap_or_else(|| prelude::unit_id())
    }

    /// The output structure; `smithy.api#Unit` when omitted.
    pub fn output(&self) -> &ShapeId {
        self.output.as_ref().unwrap_or_else(|| prelude::unit_id())
    }

    /// The input as declared, or `None` if it was omitted.
    pub fn declared_input(&self) -> Option<&ShapeId> {
        self.input.as_ref()
    }

    /// The output as declared, or `None` if it was omitted.
    pub fn declared_output(&self) -> Option<&ShapeId> {
        self.output.as_ref()
    }

    /// **Declaration-only:** errors listed directly in the operation's `errors`.
    pub fn direct_errors(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.errors.iter()
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> OperationShapeBuilder {
        OperationShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            input: self.input.clone(),
            output: self.output.clone(),
            errors: self.errors.clone(),
        }
    }
}

impl PartialEq for OperationShape {
    fn eq(&self, other: &Self) -> bool {
        self.common == other.common
            && self.input() == other.input()
            && self.output() == other.output()
            && same_set(&self.errors, &other.errors)
    }
}

impl Eq for OperationShape {}

/// Builds an [`OperationShape`]. Errors keep insertion order and ignore repeats.
#[derive(Debug, Clone)]
pub struct OperationShapeBuilder {
    common: BuilderCommon,
    input: Option<ShapeId>,
    output: Option<ShapeId>,
    errors: Vec<ShapeId>,
}

impl OperationShapeBuilder {
    common_builder_methods!();

    /// Sets the input structure.
    pub fn input(mut self, id: impl IntoShapeId) -> Self {
        self.input = self.common.shape_id(id);
        self
    }

    /// Sets the output structure.
    pub fn output(mut self, id: impl IntoShapeId) -> Self {
        self.output = self.common.shape_id(id);
        self
    }

    /// Adds an error.
    pub fn add_error(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.errors, id);
        }
        self
    }

    /// Builds the shape.
    pub fn build(self) -> Result<OperationShape, ShapeBuildError> {
        Ok(OperationShape {
            common: self.common.finish()?,
            input: self.input,
            output: self.output,
            errors: self.errors,
        })
    }
}

/// A `resource` shape.
///
/// ```
/// use aws_smithy_lang::shape::ResourceShape;
///
/// let city = ResourceShape::builder("ex#City")
///     .identifier("cityId", "ex#CityId")
///     .read("ex#GetCity")
///     .add_resource("ex#Forecast")
///     .build()?;
/// assert_eq!(city.read().unwrap(), "ex#GetCity");
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct ResourceShape {
    pub(crate) common: Common,
    pub(crate) identifiers: IndexMap<String, ShapeId>,
    pub(crate) properties: IndexMap<String, ShapeId>,
    pub(crate) create: Option<ShapeId>,
    pub(crate) put: Option<ShapeId>,
    pub(crate) read: Option<ShapeId>,
    pub(crate) update: Option<ShapeId>,
    pub(crate) delete: Option<ShapeId>,
    pub(crate) list: Option<ShapeId>,
    pub(crate) operations: Vec<ShapeId>,
    pub(crate) collection_operations: Vec<ShapeId>,
    pub(crate) resources: Vec<ShapeId>,
}

impl ResourceShape {
    /// A builder for a resource.
    pub fn builder(id: impl IntoShapeId) -> ResourceShapeBuilder {
        ResourceShapeBuilder {
            common: BuilderCommon::new(id),
            identifiers: IndexMap::new(),
            properties: IndexMap::new(),
            lifecycle: Default::default(),
            operations: Vec::new(),
            collection_operations: Vec::new(),
            resources: Vec::new(),
        }
    }

    common_accessors!();

    /// Identifier names and their target shapes.
    pub fn identifiers(&self) -> impl ExactSizeIterator<Item = (&str, &ShapeId)> + '_ {
        self.identifiers.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Property names and their target shapes.
    pub fn properties(&self) -> impl ExactSizeIterator<Item = (&str, &ShapeId)> + '_ {
        self.properties.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The `create` lifecycle operation.
    pub fn create(&self) -> Option<&ShapeId> {
        self.create.as_ref()
    }

    /// The `put` lifecycle operation.
    pub fn put(&self) -> Option<&ShapeId> {
        self.put.as_ref()
    }

    /// The `read` lifecycle operation.
    pub fn read(&self) -> Option<&ShapeId> {
        self.read.as_ref()
    }

    /// The `update` lifecycle operation.
    pub fn update(&self) -> Option<&ShapeId> {
        self.update.as_ref()
    }

    /// The `delete` lifecycle operation.
    pub fn delete(&self) -> Option<&ShapeId> {
        self.delete.as_ref()
    }

    /// The `list` lifecycle operation.
    pub fn list(&self) -> Option<&ShapeId> {
        self.list.as_ref()
    }

    /// **Declaration-only:** non-lifecycle instance operations listed in `operations`.
    pub fn direct_operations(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.operations.iter()
    }

    /// **Declaration-only:** non-lifecycle collection operations listed in
    /// `collectionOperations`.
    pub fn direct_collection_operations(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.collection_operations.iter()
    }

    /// **Declaration-only:** child resources listed directly in `resources`.
    pub fn child_resources(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.resources.iter()
    }

    /// Lifecycle operations as `(property name, target)` in JSON AST order.
    pub(crate) fn lifecycle(&self) -> [(&'static str, Option<&ShapeId>); 6] {
        [
            ("create", self.create.as_ref()),
            ("put", self.put.as_ref()),
            ("read", self.read.as_ref()),
            ("update", self.update.as_ref()),
            ("delete", self.delete.as_ref()),
            ("list", self.list.as_ref()),
        ]
    }

    /// A builder initialized with this shape.
    pub fn to_builder(&self) -> ResourceShapeBuilder {
        ResourceShapeBuilder {
            common: BuilderCommon::from_common(&self.common),
            identifiers: self.identifiers.clone(),
            properties: self.properties.clone(),
            lifecycle: [
                self.create.clone(),
                self.put.clone(),
                self.read.clone(),
                self.update.clone(),
                self.delete.clone(),
                self.list.clone(),
            ],
            operations: self.operations.clone(),
            collection_operations: self.collection_operations.clone(),
            resources: self.resources.clone(),
        }
    }
}

impl PartialEq for ResourceShape {
    fn eq(&self, other: &Self) -> bool {
        self.common == other.common
            && self.identifiers == other.identifiers
            && self.properties == other.properties
            && self.lifecycle() == other.lifecycle()
            && same_set(&self.operations, &other.operations)
            && same_set(&self.collection_operations, &other.collection_operations)
            && same_set(&self.resources, &other.resources)
    }
}

impl Eq for ResourceShape {}

/// Builds a [`ResourceShape`]. Reference sets keep insertion order and ignore repeats.
#[derive(Debug, Clone)]
pub struct ResourceShapeBuilder {
    common: BuilderCommon,
    identifiers: IndexMap<String, ShapeId>,
    properties: IndexMap<String, ShapeId>,
    /// `create`, `put`, `read`, `update`, `delete`, `list`.
    lifecycle: [Option<ShapeId>; 6],
    operations: Vec<ShapeId>,
    collection_operations: Vec<ShapeId>,
    resources: Vec<ShapeId>,
}

macro_rules! lifecycle_setters {
    ($($name:ident = $index:expr;)*) => {
        $(
            #[doc = concat!("Sets the `", stringify!($name), "` lifecycle operation.")]
            pub fn $name(mut self, id: impl IntoShapeId) -> Self {
                self.lifecycle[$index] = self.common.shape_id(id);
                self
            }
        )*
    };
}

impl ResourceShapeBuilder {
    common_builder_methods!();

    fn named_ref(
        &mut self,
        what: &str,
        name: &str,
        target: impl IntoShapeId,
    ) -> Option<(String, ShapeId)> {
        if !ShapeId::is_valid_identifier(name) {
            self.common.fail(ShapeBuildError::new(format!(
                "resource {what} name `{name}` is not a valid identifier"
            )));
            return None;
        }
        Some((name.to_owned(), self.common.shape_id(target)?))
    }

    /// Adds an identifier named `name` targeting `target`.
    pub fn identifier(mut self, name: &str, target: impl IntoShapeId) -> Self {
        if let Some((name, target)) = self.named_ref("identifier", name, target) {
            self.identifiers.insert(name, target);
        }
        self
    }

    /// Adds a property named `name` targeting `target`.
    pub fn property(mut self, name: &str, target: impl IntoShapeId) -> Self {
        if let Some((name, target)) = self.named_ref("property", name, target) {
            self.properties.insert(name, target);
        }
        self
    }

    lifecycle_setters! {
        create = 0;
        put = 1;
        read = 2;
        update = 3;
        delete = 4;
        list = 5;
    }

    /// Binds a non-lifecycle instance operation.
    pub fn add_operation(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.operations, id);
        }
        self
    }

    /// Binds a non-lifecycle collection operation.
    pub fn add_collection_operation(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.collection_operations, id);
        }
        self
    }

    /// Binds a child resource.
    pub fn add_resource(mut self, id: impl IntoShapeId) -> Self {
        if let Some(id) = self.common.shape_id(id) {
            push_unique(&mut self.resources, id);
        }
        self
    }

    /// Builds the shape.
    pub fn build(self) -> Result<ResourceShape, ShapeBuildError> {
        let [create, put, read, update, delete, list] = self.lifecycle;
        Ok(ResourceShape {
            common: self.common.finish()?,
            identifiers: self.identifiers,
            properties: self.properties,
            create,
            put,
            read,
            update,
            delete,
            list,
            operations: self.operations,
            collection_operations: self.collection_operations,
            resources: self.resources,
        })
    }
}

macro_rules! into_shape {
    ($($variant:ident($ty:ident);)*) => {
        $(
            impl From<$ty> for Shape {
                fn from(value: $ty) -> Self {
                    Shape::$variant(value)
                }
            }
        )*
    };
}

into_shape! {
    Simple(SimpleShape);
    List(ListShape);
    Map(MapShape);
    Structure(StructureShape);
    Union(UnionShape);
    Enum(EnumShape);
    IntEnum(IntEnumShape);
    Service(ServiceShape);
    Operation(OperationShape);
    Resource(ResourceShape);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeObject;

    #[test]
    fn builders_report_invalid_input() {
        let err = StructureShape::builder("nope").build().unwrap_err();
        assert!(err.message().contains("nope"), "{err}");
        assert!(std::error::Error::source(&err).is_some());
        let err = StructureShape::builder("a#B$c").build().unwrap_err();
        assert!(err.message().contains("member ID"), "{err}");
        assert!(StructureShape::builder("a#B")
            .member("1bad", "a#T")
            .build()
            .is_err());
        assert!(StructureShape::builder("a#B")
            .member("ok", "bad target")
            .build()
            .is_err());
        let foreign = MemberShape::builder("a#Other$m", "a#T").build().unwrap();
        let err = StructureShape::builder("a#B")
            .add_member(foreign)
            .build()
            .unwrap_err();
        assert!(err.message().contains("does not belong"), "{err}");
        assert!(SimpleShape::builder(ShapeType::List, "a#B")
            .build()
            .is_err());
        assert!(ListShape::builder("a#L").build().is_err());
        assert!(MapShape::builder("a#M").key("a#K").build().is_err());
        assert!(MemberShape::builder("a#B", "a#T").build().is_err());
        assert!(ResourceShape::builder("a#R")
            .identifier("bad id", "a#T")
            .build()
            .is_err());
    }

    #[test]
    fn equality_ignores_location_and_unordered_sets() {
        let a = ServiceShape::builder("a#S")
            .add_operation("a#A")
            .add_operation("a#B")
            .add_operation("a#A")
            .build()
            .unwrap();
        assert_eq!(a.direct_operations().len(), 2);
        let b = ServiceShape::builder("a#S")
            .add_operation("a#B")
            .add_operation("a#A")
            .source_location(SourceLocation::new("x.json".into()))
            .build()
            .unwrap();
        assert_eq!(a, b);
        assert_ne!(a, a.to_builder().version("1").build().unwrap());

        // Member order is significant.
        let s1 = StructureShape::builder("a#S")
            .member("a", "a#T")
            .member("b", "a#T")
            .build()
            .unwrap();
        let s2 = StructureShape::builder("a#S")
            .member("b", "a#T")
            .member("a", "a#T")
            .build()
            .unwrap();
        assert_ne!(s1, s2);
        assert_eq!(s1, s1.to_builder().build().unwrap());

        // Omitted input/output equals explicit Unit.
        let op = OperationShape::builder("a#O").build().unwrap();
        let explicit = op.to_builder().input("smithy.api#Unit").build().unwrap();
        assert_eq!(op, explicit);
        assert!(explicit.declared_input().is_some());

        // Omitted enum values equal explicit values equal to the member name.
        let e1 = EnumShape::builder("a#E").variant("A").build().unwrap();
        let e2 = EnumShape::builder("a#E")
            .enum_value("A", "A")
            .build()
            .unwrap();
        let e3 = EnumShape::builder("a#E")
            .enum_value("A", "a")
            .build()
            .unwrap();
        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
        assert_eq!(Shape::from(e1.clone()), Shape::from(e2));
        assert_ne!(Shape::from(e1), Shape::from(s1));
    }

    #[test]
    fn shape_accessors() {
        let list: Shape = ListShape::builder("a#L")
            .member("a#T")
            .add_trait("a#t", NodeObject::new())
            .build()
            .unwrap()
            .into();
        assert_eq!(list.shape_type(), ShapeType::List);
        assert_eq!(list.id(), "a#L");
        assert!(list.traits().contains("a#t"));
        assert_eq!(
            list.members().map(MemberShape::name).collect::<Vec<_>>(),
            ["member"]
        );
        assert_eq!(list.member("member").unwrap().target(), "a#T");
        assert!(list.as_list().is_some() && list.as_map().is_none());
        assert!(list.source_location().is_none());
        let rebuilt = list
            .as_list()
            .unwrap()
            .to_builder()
            .remove_trait("a#t")
            .build()
            .unwrap();
        assert!(rebuilt.traits().is_empty());
    }
}

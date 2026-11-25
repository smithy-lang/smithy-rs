/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Relationships between shapes: the single definition of which shape references which.
//!
//! Everything that follows references uses this module: assembly-time reference resolution
//! and target validation, [`ShapeRef::relationships`], the cached [`RelationshipIndex`]
//! (forward and reverse), the [`knowledge`](crate::knowledge) indexes built on it, and
//! [`Walker`](crate::traversal::Walker).
//!
//! Every ordinary reference in a [`Model`] resolves, so each [`Relationship`]
//! always has a target. Applied traits are not relationships: trait definitions may be absent.
//!
//! ```
//! use aws_smithy_lang::relationships::RelationshipType;
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let model = Model::from_json_str("m", r#"{"smithy": "2.0", "shapes": {
//!     "ex#Ping": {"type": "operation", "input": {"target": "ex#PingInput"}},
//!     "ex#PingInput": {"type": "structure"}
//! }}"#)?;
//! let input = model.expect_shape(&ShapeId::from_static("ex#PingInput"))?;
//! let users: Vec<_> = input
//!     .incoming_relationships()
//!     .filter(|r| r.kind() == RelationshipType::Input)
//!     .map(|r| r.source().id().to_string())
//!     .collect();
//! assert_eq!(users, ["ex#Ping"]);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```

use crate::diagnostic::{escape_pointer_token, SourceLocation};
use crate::knowledge::KnowledgeIndex;
use crate::model::{Model, ModelData};
use crate::prelude;
use crate::shape::{MemberShape, MemberView, Shape, ShapeRef, ShapeType, ShapeView};
use crate::shape_id::ShapeId;
use std::fmt;
use std::iter::FusedIterator;

/// The kind of a [`Relationship`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipType {
    /// A service or resource to an operation in its `operations`.
    Operation,
    /// A resource to an operation in its `collectionOperations`.
    CollectionOperation,
    /// A service or resource to a resource in its `resources`.
    Resource,
    /// A service or operation to an error structure in its `errors`.
    Error,
    /// A resource to its `create` operation.
    Create,
    /// A resource to its `put` operation.
    Put,
    /// A resource to its `read` operation.
    Read,
    /// A resource to its `update` operation.
    Update,
    /// A resource to its `delete` operation.
    Delete,
    /// A resource to its `list` operation.
    List,
    /// A resource to the target of one of its `identifiers`.
    Identifier,
    /// A resource to the target of one of its `properties`.
    Property,
    /// An operation to its input structure (`smithy.api#Unit` when omitted).
    Input,
    /// An operation to its output structure (`smithy.api#Unit` when omitted).
    Output,
    /// A list, map, structure, union, enum, or intEnum to one of its members.
    Member,
    /// A member to the shape that contains it.
    MemberContainer,
    /// A member to its target shape.
    MemberTarget,
}

impl RelationshipType {
    /// Returns `true` for edges that bind an operation to a service or resource: `Operation`,
    /// `CollectionOperation`, and the `Create`, `Put`, `Read`, `Update`, `Delete`, and `List`
    /// lifecycle bindings.
    pub fn is_operation_binding(self) -> bool {
        use RelationshipType::*;
        matches!(
            self,
            Operation | CollectionOperation | Create | Put | Read | Update | Delete | List
        )
    }

    /// Returns `true` for edges that define service and resource containment: every
    /// [operation binding](Self::is_operation_binding), plus `Resource`.
    ///
    /// These are exactly the edges [`TopDownIndex`](crate::knowledge::TopDownIndex) follows.
    pub fn is_containment(self) -> bool {
        self == RelationshipType::Resource || self.is_operation_binding()
    }

    /// A stable rank used to order edges that share a source.
    pub(crate) fn rank(self) -> u8 {
        self as u8
    }

    /// The shape types a reference of this kind may target. These constraints are what make
    /// typed views infallible, so assembly enforces them before any model exists.
    pub(crate) fn allowed_targets(self) -> TargetKinds {
        use RelationshipType::*;
        match self {
            Operation | CollectionOperation | Create | Put | Read | Update | Delete | List => {
                TargetKinds::Only(&[ShapeType::Operation])
            }
            Resource => TargetKinds::Only(&[ShapeType::Resource]),
            Error | Input | Output => TargetKinds::Only(&[ShapeType::Structure]),
            Identifier => TargetKinds::Only(&[ShapeType::String, ShapeType::Enum]),
            Property | MemberTarget => TargetKinds::Data,
            Member | MemberContainer => TargetKinds::Data,
        }
    }

    /// How diagnostics describe a reference of this kind.
    pub(crate) fn describe(self) -> &'static str {
        use RelationshipType::*;
        match self {
            Operation => "`operations`",
            CollectionOperation => "`collectionOperations`",
            Resource => "`resources`",
            Error => "`errors`",
            Create => "resource `create`",
            Put => "resource `put`",
            Read => "resource `read`",
            Update => "resource `update`",
            Delete => "resource `delete`",
            List => "resource `list`",
            Identifier => "resource identifier",
            Property => "resource property",
            Input => "operation `input`",
            Output => "operation `output`",
            Member | MemberContainer | MemberTarget => "member",
        }
    }
}

/// The shape types a reference may target.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TargetKinds {
    Only(&'static [ShapeType]),
    /// Any data shape: not a service, resource, operation, or member.
    Data,
}

impl TargetKinds {
    pub(crate) fn allows(self, shape_type: ShapeType) -> bool {
        match self {
            TargetKinds::Only(allowed) => allowed.contains(&shape_type),
            TargetKinds::Data => !shape_type.is_service_shape() && shape_type != ShapeType::Member,
        }
    }

    pub(crate) fn describe(self) -> String {
        match self {
            TargetKinds::Only(allowed) => {
                let names: Vec<_> = allowed.iter().map(|t| t.as_str()).collect();
                names.join(" or ")
            }
            TargetKinds::Data => "data shape".to_owned(),
        }
    }
}

/// Where a reference was declared, relative to its shape's (or member's) location.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RefPath<'a> {
    /// `/{property}/target`
    Property(&'static str),
    /// `/{property}/{index}/target`
    Indexed(&'static str, usize),
    /// `/{property}/{name}/target`
    Named(&'static str, &'a str),
    /// `/{field}`, such as a member's `/target`.
    Field(&'static str),
    /// An omitted operation input or output, normalized to `smithy.api#Unit`.
    Implicit,
}

impl RefPath<'_> {
    /// The location of the reference. Computed only when a diagnostic needs it.
    pub(crate) fn locate(self, owner: &SourceLocation) -> SourceLocation {
        match self {
            RefPath::Property(property) => owner.child(&format!("/{property}/target")),
            RefPath::Indexed(property, index) => {
                owner.child(&format!("/{property}/{index}/target"))
            }
            RefPath::Named(property, name) => owner.child(&format!(
                "/{property}/{}/target",
                escape_pointer_token(name)
            )),
            RefPath::Field(field) => owner.child(&format!("/{field}")),
            RefPath::Implicit => owner.clone(),
        }
    }
}

/// One reference a shape declares to a root shape, before resolution.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeclaredRef<'a> {
    pub(crate) kind: RelationshipType,
    pub(crate) target: &'a ShapeId,
    pub(crate) path: RefPath<'a>,
}

/// Calls `f` with every reference `shape` declares to another root shape, in JSON AST order.
/// Member targets are reported by [`member_reference`]; member edges are structural.
///
/// This is the one place that knows which shape properties are references.
pub(crate) fn for_each_reference<'a>(shape: &'a Shape, mut f: impl FnMut(DeclaredRef<'a>)) {
    use RefPath::*;
    use RelationshipType as R;
    let list = |kind, property, ids: &'a [ShapeId], f: &mut dyn FnMut(DeclaredRef<'a>)| {
        for (index, target) in ids.iter().enumerate() {
            f(DeclaredRef {
                kind,
                target,
                path: Indexed(property, index),
            });
        }
    };
    match shape {
        Shape::Service(service) => {
            list(R::Operation, "operations", &service.operations, &mut f);
            list(R::Resource, "resources", &service.resources, &mut f);
            list(R::Error, "errors", &service.errors, &mut f);
        }
        Shape::Operation(operation) => {
            for (kind, property, declared) in [
                (R::Input, "input", &operation.input),
                (R::Output, "output", &operation.output),
            ] {
                f(match declared {
                    Some(target) => DeclaredRef {
                        kind,
                        target,
                        path: Property(property),
                    },
                    None => DeclaredRef {
                        kind,
                        target: prelude::unit_id(),
                        path: Implicit,
                    },
                });
            }
            list(R::Error, "errors", &operation.errors, &mut f);
        }
        Shape::Resource(resource) => {
            for (kind, property, map) in [
                (R::Identifier, "identifiers", &resource.identifiers),
                (R::Property, "properties", &resource.properties),
            ] {
                for (name, target) in map {
                    f(DeclaredRef {
                        kind,
                        target,
                        path: Named(property, name),
                    });
                }
            }
            let kinds = [R::Create, R::Put, R::Read, R::Update, R::Delete, R::List];
            for (kind, (property, target)) in kinds.into_iter().zip(resource.lifecycle()) {
                if let Some(target) = target {
                    f(DeclaredRef {
                        kind,
                        target,
                        path: Property(property),
                    });
                }
            }
            list(R::Operation, "operations", &resource.operations, &mut f);
            let collection = &resource.collection_operations;
            list(
                R::CollectionOperation,
                "collectionOperations",
                collection,
                &mut f,
            );
            list(R::Resource, "resources", &resource.resources, &mut f);
        }
        _ => {}
    }
}

/// The reference a member declares to its target.
pub(crate) fn member_reference(member: &MemberShape) -> DeclaredRef<'_> {
    DeclaredRef {
        kind: RelationshipType::MemberTarget,
        target: &member.target,
        path: RefPath::Field("target"),
    }
}

/// A directed edge from one shape or member to another.
#[derive(Clone, Copy)]
pub struct Relationship<'a> {
    source: ShapeRef<'a>,
    kind: RelationshipType,
    target: ShapeRef<'a>,
}

impl<'a> Relationship<'a> {
    pub(crate) fn new(source: ShapeRef<'a>, kind: RelationshipType, target: ShapeRef<'a>) -> Self {
        Self {
            source,
            kind,
            target,
        }
    }

    /// The shape or member the edge starts from.
    pub fn source(&self) -> ShapeRef<'a> {
        self.source
    }

    /// The kind of edge.
    pub fn kind(&self) -> RelationshipType {
        self.kind
    }

    /// The ID of the target.
    pub fn target_id(&self) -> &'a ShapeId {
        self.target.id()
    }

    /// The target shape or member. Always resolved.
    pub fn target(&self) -> ShapeRef<'a> {
        self.target
    }
}

impl fmt::Debug for Relationship<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -[{:?}]-> {}",
            self.source.id(),
            self.kind,
            self.target.id()
        )
    }
}

fn root<'a>(data: &'a ModelData, id: &ShapeId) -> ShapeRef<'a> {
    let shape = data
        .root(id)
        .expect("models only contain resolved references");
    ShapeRef::Shape(ShapeView::new(data, shape))
}

/// Outgoing relationships of `shape`, in a deterministic order following the JSON AST.
pub(crate) fn relationships<'a>(shape: ShapeRef<'a>) -> Vec<Relationship<'a>> {
    let mut out = Vec::new();
    let view = match shape {
        ShapeRef::Member(member) => {
            out.push(Relationship::new(
                shape,
                RelationshipType::MemberContainer,
                member.container().into(),
            ));
            out.push(Relationship::new(
                shape,
                RelationshipType::MemberTarget,
                member.target_shape().into(),
            ));
            return out;
        }
        ShapeRef::Shape(view) => view,
    };
    let data = view.data;
    for_each_reference(view.shape, |reference| {
        out.push(Relationship::new(
            shape,
            reference.kind,
            root(data, reference.target),
        ))
    });
    for member in view.members().iter() {
        out.push(Relationship::new(
            shape,
            RelationshipType::Member,
            ShapeRef::Member(member),
        ));
    }
    out
}

impl<'a> ShapeRef<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    ///
    /// Served from the model's cached [`RelationshipIndex`].
    pub fn relationships(&self) -> Relationships<'a> {
        RelationshipIndex::new(self.data()).outgoing_of(*self)
    }

    /// Incoming relationships: every edge whose target is this shape or member, sorted by
    /// source ID and then by relationship kind. Each edge keeps its original direction.
    ///
    /// Served from the model's cached [`RelationshipIndex`].
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        RelationshipIndex::new(self.data()).incoming_of(*self)
    }
}

impl<'a> ShapeView<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    pub fn relationships(&self) -> Relationships<'a> {
        ShapeRef::Shape(*self).relationships()
    }

    /// Incoming relationships, sorted by source ID and then by relationship kind.
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        ShapeRef::Shape(*self).incoming_relationships()
    }
}

impl<'a> MemberView<'a> {
    /// Outgoing relationships: the container, then the target.
    pub fn relationships(&self) -> Relationships<'a> {
        ShapeRef::Member(*self).relationships()
    }

    /// Incoming relationships: the container's `Member` edge to this member.
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        ShapeRef::Member(*self).incoming_relationships()
    }
}

/// A root shape or member of one model, numbered by the model's [`RelationshipIndex`].
///
/// A node ID is a small `Copy` value that doesn't borrow the model. An index can store node IDs
/// in owned data, such as data cached with
/// [`Model::knowledge_data`](crate::Model::knowledge_data), and resolve them to views with
/// [`RelationshipIndex::shape`]. [`index`](Self::index) numbers nodes densely from zero, so it
/// can also key a `Vec` or a bit set sized by [`RelationshipIndex::node_count`], and
/// [`from_index`](Self::from_index) turns a position back into a node ID.
///
/// Node IDs follow [`Model::shapes`](crate::Model::shapes) order, with each root shape
/// immediately followed by its members in member order.
///
/// A node ID is a position in one model's numbering, so other models use the same IDs for
/// different shapes. It is only meaningful for the model whose index produced it, and for clones
/// of that model. Passing it to another model's index panics if it is out of range, and
/// otherwise names an unrelated shape. To match shapes across models, such as two versions of
/// one service, compare their [`ShapeId`]s, and find each model's node with
/// [`RelationshipIndex::node`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// The node at `position`: the inverse of [`index`](Self::index).
    ///
    /// The position isn't checked against a model here. Methods that take a node check it
    /// instead, as they do for a node of another model: [`RelationshipIndex::shape`] panics
    /// unless `position` is less than [`RelationshipIndex::node_count`].
    ///
    /// # Panics
    ///
    /// If `position` is greater than `u32::MAX`.
    pub fn from_index(position: usize) -> Self {
        Self(index(position))
    }

    /// The position of this node, from zero up to [`RelationshipIndex::node_count`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A relationship between two nodes: [`Relationship`] without the views.
///
/// Returned by [`RelationshipIndex::outgoing`] and [`RelationshipIndex::incoming`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    source: NodeId,
    target: NodeId,
    kind: RelationshipType,
}

impl Edge {
    /// The shape or member the edge starts from.
    pub fn source(&self) -> NodeId {
        self.source
    }

    /// The kind of edge.
    pub fn kind(&self) -> RelationshipType {
        self.kind
    }

    /// The shape or member the edge points to.
    pub fn target(&self) -> NodeId {
        self.target
    }
}

/// Every ordinary relationship of a model, in both directions.
///
/// Uses `O(V + E)` memory and stores only indices, never references into the model.
pub(crate) struct RelationshipData {
    /// The node of each root, indexed by root position.
    root_nodes: Box<[NodeId]>,
    /// The root position of each node.
    node_roots: Box<[u32]>,
    edges: Box<[Edge]>,
    /// Outgoing edges of node `n` are `edges[outgoing[n]..outgoing[n + 1]]`.
    outgoing: Box<[u32]>,
    /// Incoming edges of node `n` are `incoming[incoming_offsets[n]..incoming_offsets[n + 1]]`.
    incoming_offsets: Box<[u32]>,
    /// Edge indices, sorted within each target by source ID and then relationship kind.
    incoming: Box<[u32]>,
}

fn index(value: usize) -> u32 {
    u32::try_from(value).expect("models are far smaller than u32::MAX nodes and edges")
}

impl RelationshipData {
    pub(crate) fn build(data: &ModelData) -> Self {
        let mut root_nodes = Vec::with_capacity(data.root_count());
        let mut node_roots = Vec::new();
        for (position, shape) in data.declarations().enumerate() {
            root_nodes.push(NodeId::from_index(node_roots.len()));
            node_roots.extend(std::iter::repeat_n(
                index(position),
                shape.member_count() + 1,
            ));
        }
        let mut this = Self {
            root_nodes: root_nodes.into(),
            node_roots: node_roots.into(),
            edges: Box::default(),
            outgoing: Box::default(),
            incoming_offsets: Box::default(),
            incoming: Box::default(),
        };
        let nodes = this.node_roots.len();

        let mut edges = Vec::new();
        let mut outgoing = Vec::with_capacity(nodes + 1);
        outgoing.push(0);
        let mut ids: Vec<&ShapeId> = Vec::with_capacity(nodes);
        for node in (0..nodes).map(NodeId::from_index) {
            let shape = this.shape(data, node);
            ids.push(shape.id());
            for relationship in relationships(shape) {
                edges.push(Edge {
                    source: node,
                    target: this
                        .node_of(data, relationship.target_id())
                        .expect("models only contain resolved references"),
                    kind: relationship.kind(),
                });
            }
            outgoing.push(index(edges.len()));
        }

        // Counting sort by target, then order each target's edges by source ID and kind.
        let mut incoming_offsets = vec![0u32; nodes + 1];
        for edge in &edges {
            incoming_offsets[edge.target.index() + 1] += 1;
        }
        for n in 0..nodes {
            incoming_offsets[n + 1] += incoming_offsets[n];
        }
        let mut fill = incoming_offsets.clone();
        let mut incoming = vec![0u32; edges.len()];
        for (i, edge) in edges.iter().enumerate() {
            let slot = &mut fill[edge.target.index()];
            incoming[*slot as usize] = index(i);
            *slot += 1;
        }
        for n in 0..nodes {
            let range = incoming_offsets[n] as usize..incoming_offsets[n + 1] as usize;
            incoming[range].sort_by(|&a, &b| {
                let (a, b) = (edges[a as usize], edges[b as usize]);
                ids[a.source.index()]
                    .cmp(ids[b.source.index()])
                    .then(a.kind.rank().cmp(&b.kind.rank()))
            });
        }

        this.edges = edges.into();
        this.outgoing = outgoing.into();
        this.incoming_offsets = incoming_offsets.into();
        this.incoming = incoming.into();
        this
    }

    fn node_count(&self) -> usize {
        self.node_roots.len()
    }

    /// The position of a node in per-node tables, after checking that it belongs to this model.
    fn slot(&self, node: NodeId) -> usize {
        let slot = node.index();
        assert!(
            slot < self.node_count(),
            "{node:?} is not a node of this model, which has {} nodes",
            self.node_count()
        );
        slot
    }

    /// The node of a root shape or member ID.
    pub(crate) fn node_of(&self, data: &ModelData, id: &ShapeId) -> Option<NodeId> {
        let position = data.root_position(id)?;
        let root = self.root_nodes[position];
        match id.member() {
            None => Some(root),
            Some(member) => {
                let member = data.root_at(position).member_index(member)?;
                Some(NodeId(root.0 + 1 + index(member)))
            }
        }
    }

    /// The shape or member of a node.
    pub(crate) fn shape<'m>(&self, data: &'m ModelData, node: NodeId) -> ShapeRef<'m> {
        let root = self.node_roots[self.slot(node)];
        let shape = data.root_at(root as usize);
        match (node.0 - self.root_nodes[root as usize].0).checked_sub(1) {
            None => ShapeRef::Shape(ShapeView::new(data, shape)),
            Some(member) => {
                let member = shape
                    .member_at(member as usize)
                    .expect("member nodes are in range");
                ShapeRef::Member(MemberView::new(data, shape, member))
            }
        }
    }

    fn outgoing_slice(&self, node: NodeId) -> &[Edge] {
        let slot = self.slot(node);
        &self.edges[self.outgoing[slot] as usize..self.outgoing[slot + 1] as usize]
    }

    fn incoming_slice(&self, node: NodeId) -> &[u32] {
        let slot = self.slot(node);
        let start = self.incoming_offsets[slot] as usize;
        let end = self.incoming_offsets[slot + 1] as usize;
        &self.incoming[start..end]
    }
}

/// Cached outgoing and incoming relationships of every shape and member.
///
/// This indexes every [`RelationshipType`] in both directions, including root-to-member,
/// member-to-container, and member-to-target edges. Applied traits are not relationships.
///
/// # Nodes
///
/// The index numbers every root shape and member with a [`NodeId`]. Most callers use the
/// [`ShapeId`] queries, [`get_outgoing`](Self::get_outgoing) and
/// [`get_incoming`](Self::get_incoming), which return views. An index that walks the graph or
/// keeps per-shape data can work with nodes instead: [`node`](Self::node) finds the node of a
/// shape, [`outgoing`](Self::outgoing) and [`incoming`](Self::incoming) return [`Edge`]s
/// between nodes without building views, and [`shape`](Self::shape) turns a node back into a
/// view.
///
/// ```
/// use aws_smithy_lang::relationships::{RelationshipIndex, RelationshipType};
/// use aws_smithy_lang::{Model, ShapeId};
///
/// let model = Model::from_idl_str("m.smithy", r#"
/// $version: "2"
/// namespace ex
///
/// structure Outer { inner: Inner }
/// structure Inner { value: String }
/// "#)?;
/// let relationships = model.knowledge::<RelationshipIndex<'_>>();
///
/// // Every member that can (transitively) contain a value of `ex#Inner`.
/// let start = relationships.node(&ShapeId::from_static("ex#Inner")).unwrap();
/// let mut visited = vec![false; relationships.node_count()];
/// let mut stack = vec![start];
/// let mut members = Vec::new();
/// while let Some(node) = stack.pop() {
///     for edge in relationships.incoming(node) {
///         let source = edge.source();
///         let follow = matches!(
///             edge.kind(),
///             RelationshipType::MemberTarget | RelationshipType::Member
///         );
///         if follow && !visited[source.index()] {
///             visited[source.index()] = true;
///             stack.push(source);
///             if edge.kind() == RelationshipType::MemberTarget {
///                 members.push(relationships.shape(source).id().to_string());
///             }
///         }
///     }
/// }
/// assert_eq!(members, ["ex#Outer$inner"]);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy)]
pub struct RelationshipIndex<'m> {
    pub(crate) data: &'m ModelData,
    pub(crate) index: &'m RelationshipData,
}

impl<'m> RelationshipIndex<'m> {
    pub(crate) fn new(data: &'m ModelData) -> Self {
        Self {
            data,
            index: data.knowledge.relationships(data),
        }
    }

    /// Outgoing relationships of a shape or member, in the same order as
    /// [`ShapeRef::relationships`]. Returns `None` if no such shape or member exists.
    pub fn get_outgoing(&self, shape: &ShapeId) -> Option<Relationships<'m>> {
        Some(self.relationships(self.node(shape)?, Direction::Outgoing))
    }

    /// Incoming relationships of a shape or member, sorted by source ID and then by
    /// relationship kind. Each edge keeps its original direction: [`Relationship::source`] is
    /// the referring shape and [`Relationship::target`] is the queried shape.
    ///
    /// Returns `None` if no such shape or member exists.
    pub fn get_incoming(&self, shape: &ShapeId) -> Option<Relationships<'m>> {
        Some(self.relationships(self.node(shape)?, Direction::Incoming))
    }

    /// The number of nodes: every root shape and member, including the prelude. Every node's
    /// [`index`](NodeId::index) is less than this.
    pub fn node_count(&self) -> usize {
        self.index.node_count()
    }

    /// The node of a shape or member. Returns `None` if no such shape or member exists.
    pub fn node(&self, shape: &ShapeId) -> Option<NodeId> {
        self.index.node_of(self.data, shape)
    }

    /// The shape or member of a node.
    ///
    /// # Panics
    ///
    /// If `node` is not a node of this model. A node of another model may instead name an
    /// unrelated shape.
    pub fn shape(&self, node: NodeId) -> ShapeRef<'m> {
        self.index.shape(self.data, node)
    }

    /// Outgoing edges of a node, in the same order as [`get_outgoing`](Self::get_outgoing).
    ///
    /// # Panics
    ///
    /// If `node` is not a node of this model, as for [`shape`](Self::shape).
    pub fn outgoing(&self, node: NodeId) -> Edges<'m> {
        let index: &'m RelationshipData = self.index;
        Edges {
            inner: EdgesInner::Outgoing(index.outgoing_slice(node).iter()),
        }
    }

    /// Incoming edges of a node, in the same order as [`get_incoming`](Self::get_incoming). Each
    /// edge keeps its original direction: its [`target`](Edge::target) is `node`.
    ///
    /// # Panics
    ///
    /// If `node` is not a node of this model, as for [`shape`](Self::shape).
    pub fn incoming(&self, node: NodeId) -> Edges<'m> {
        let index: &'m RelationshipData = self.index;
        Edges {
            inner: EdgesInner::Incoming {
                edges: &index.edges,
                ids: index.incoming_slice(node).iter(),
            },
        }
    }

    pub(crate) fn outgoing_of(&self, shape: ShapeRef<'m>) -> Relationships<'m> {
        self.relationships(self.expect_node(shape.id()), Direction::Outgoing)
    }

    pub(crate) fn incoming_of(&self, shape: ShapeRef<'m>) -> Relationships<'m> {
        self.relationships(self.expect_node(shape.id()), Direction::Incoming)
    }

    /// The node of a shape known to exist in this model.
    pub(crate) fn expect_node(&self, id: &ShapeId) -> NodeId {
        self.node(id).expect("views always belong to their model")
    }

    pub(crate) fn lookup(&self, id: &ShapeId) -> Option<(NodeId, ShapeRef<'m>)> {
        let node = self.node(id)?;
        Some((node, self.shape(node)))
    }

    /// The root shape of a node known to be a root.
    pub(crate) fn root(&self, node: NodeId) -> ShapeView<'m> {
        self.shape(node)
            .as_shape()
            .expect("containment and binding edges only connect root shapes")
    }

    fn relationships(&self, node: NodeId, direction: Direction) -> Relationships<'m> {
        let edges = match direction {
            Direction::Outgoing => self.outgoing(node),
            Direction::Incoming => self.incoming(node),
        };
        Relationships {
            index: *self,
            node,
            edges,
        }
    }
}

impl<'m> KnowledgeIndex<'m> for RelationshipIndex<'m> {
    fn from_model(model: &'m Model) -> Self {
        Self::new(model.data())
    }
}

impl fmt::Debug for RelationshipIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelationshipIndex")
            .field("edges", &self.index.edges.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Outgoing,
    Incoming,
}

#[derive(Clone)]
enum EdgesInner<'m> {
    /// A node's outgoing edges are contiguous.
    Outgoing(std::slice::Iter<'m, Edge>),
    /// A node's incoming edges are indices into every edge.
    Incoming {
        edges: &'m [Edge],
        ids: std::slice::Iter<'m, u32>,
    },
}

/// An iterator over the [`Edge`]s of a node, returned by [`RelationshipIndex::outgoing`] and
/// [`RelationshipIndex::incoming`].
#[derive(Clone)]
pub struct Edges<'m> {
    inner: EdgesInner<'m>,
}

impl Edges<'_> {
    fn direction(&self) -> &'static str {
        match self.inner {
            EdgesInner::Outgoing(_) => "outgoing",
            EdgesInner::Incoming { .. } => "incoming",
        }
    }
}

impl Iterator for Edges<'_> {
    type Item = Edge;

    fn next(&mut self) -> Option<Edge> {
        match &mut self.inner {
            EdgesInner::Outgoing(edges) => edges.next().copied(),
            EdgesInner::Incoming { edges, ids } => Some(edges[*ids.next()? as usize]),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match &self.inner {
            EdgesInner::Outgoing(edges) => edges.len(),
            EdgesInner::Incoming { ids, .. } => ids.len(),
        };
        (len, Some(len))
    }
}

impl ExactSizeIterator for Edges<'_> {}

impl FusedIterator for Edges<'_> {}

impl fmt::Debug for Edges<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Edges")
            .field("direction", &self.direction())
            .field("remaining", &self.len())
            .finish()
    }
}

/// An iterator over cached [`Relationship`]s, returned by [`RelationshipIndex`] and by
/// `relationships`/`incoming_relationships` on shape views.
#[derive(Clone)]
pub struct Relationships<'m> {
    index: RelationshipIndex<'m>,
    /// The queried shape: the source of outgoing edges, or the target of incoming edges.
    node: NodeId,
    edges: Edges<'m>,
}

impl<'m> Iterator for Relationships<'m> {
    type Item = Relationship<'m>;

    fn next(&mut self) -> Option<Relationship<'m>> {
        let edge = self.edges.next()?;
        Some(Relationship::new(
            self.index.shape(edge.source),
            edge.kind,
            self.index.shape(edge.target),
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.edges.size_hint()
    }
}

impl ExactSizeIterator for Relationships<'_> {}

impl FusedIterator for Relationships<'_> {}

impl fmt::Debug for Relationships<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Relationships")
            .field("shape", self.index.shape(self.node).id())
            .field("direction", &self.edges.direction())
            .field("remaining", &self.len())
            .finish()
    }
}

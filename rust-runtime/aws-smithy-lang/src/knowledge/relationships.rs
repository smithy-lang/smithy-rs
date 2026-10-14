/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cached forward and reverse relationships.

use crate::model::ModelData;
use crate::shape::{MemberView, ShapeRef, ShapeView};
use crate::shape_id::ShapeId;
use crate::traversal::{self, Relationship, RelationshipType};
use std::fmt;
use std::iter::FusedIterator;
use std::ops::Range;

/// A node is a root shape or a member. Nodes are numbered in declaration order (document, then
/// prelude), each root immediately followed by its members in member order.
type Node = u32;

#[derive(Clone, Copy)]
struct Edge {
    source: Node,
    target: Node,
    kind: RelationshipType,
}

/// Every ordinary relationship of a model, in both directions.
///
/// Uses `O(V + E)` memory and stores only indices, never references into the model.
pub(crate) struct RelationshipData {
    /// The node of each root, indexed by root position.
    root_nodes: Box<[Node]>,
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
        for (position, decl) in data.declarations().enumerate() {
            root_nodes.push(index(node_roots.len()));
            node_roots.extend(std::iter::repeat_n(
                index(position),
                decl.member_count() + 1,
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
        for node in 0..index(nodes) {
            let shape = this.shape(data, node);
            ids.push(shape.id());
            for relationship in traversal::relationships(shape) {
                edges.push(Edge {
                    source: node,
                    target: this
                        .node_of(data, relationship.target_id().as_str())
                        .expect("validated models only contain resolved references"),
                    kind: relationship.kind(),
                });
            }
            outgoing.push(index(edges.len()));
        }

        // Counting sort by target, then order each target's edges by source ID and kind.
        let mut incoming_offsets = vec![0u32; nodes + 1];
        for edge in &edges {
            incoming_offsets[edge.target as usize + 1] += 1;
        }
        for n in 0..nodes {
            incoming_offsets[n + 1] += incoming_offsets[n];
        }
        let mut fill = incoming_offsets.clone();
        let mut incoming = vec![0u32; edges.len()];
        for (i, edge) in edges.iter().enumerate() {
            let slot = &mut fill[edge.target as usize];
            incoming[*slot as usize] = index(i);
            *slot += 1;
        }
        for n in 0..nodes {
            let range = incoming_offsets[n] as usize..incoming_offsets[n + 1] as usize;
            incoming[range].sort_by(|&a, &b| {
                let (a, b) = (edges[a as usize], edges[b as usize]);
                ids[a.source as usize]
                    .cmp(ids[b.source as usize])
                    .then(a.kind.rank().cmp(&b.kind.rank()))
            });
        }

        this.edges = edges.into();
        this.outgoing = outgoing.into();
        this.incoming_offsets = incoming_offsets.into();
        this.incoming = incoming.into();
        this
    }

    /// The node of a root shape or member ID.
    pub(crate) fn node_of(&self, data: &ModelData, id: &str) -> Option<Node> {
        match id.split_once('$') {
            None => Some(self.root_nodes[data.root_position(id)?]),
            Some((root, member)) => {
                let position = data.root_position(root)?;
                let member = data.root_at(position).member_position(member)?;
                Some(self.root_nodes[position] + 1 + index(member))
            }
        }
    }

    /// The shape or member of a node.
    pub(crate) fn shape<'m>(&self, data: &'m ModelData, node: Node) -> ShapeRef<'m> {
        let root = self.node_roots[node as usize];
        let decl = data.root_at(root as usize);
        match (node - self.root_nodes[root as usize]).checked_sub(1) {
            None => ShapeRef::Shape(ShapeView::new(data, decl)),
            Some(member) => {
                let member = decl
                    .member_at(member as usize)
                    .expect("member nodes are in range");
                ShapeRef::Member(MemberView::new(data, decl, member))
            }
        }
    }

    fn outgoing_range(&self, node: Node) -> Range<u32> {
        self.outgoing[node as usize]..self.outgoing[node as usize + 1]
    }

    fn incoming_slice(&self, node: Node) -> &[u32] {
        let start = self.incoming_offsets[node as usize] as usize;
        let end = self.incoming_offsets[node as usize + 1] as usize;
        &self.incoming[start..end]
    }

    /// Outgoing `(kind, target)` pairs of a node, in relationship order.
    pub(crate) fn outgoing_edges(
        &self,
        node: Node,
    ) -> impl Iterator<Item = (RelationshipType, Node)> + '_ {
        let range = self.outgoing_range(node);
        self.edges[range.start as usize..range.end as usize]
            .iter()
            .map(|edge| (edge.kind, edge.target))
    }

    /// Incoming `(source, kind)` pairs of a node, sorted by source ID and then kind.
    pub(crate) fn incoming_edges(
        &self,
        node: Node,
    ) -> impl Iterator<Item = (Node, RelationshipType)> + '_ {
        self.incoming_slice(node).iter().map(|&edge| {
            let edge = self.edges[edge as usize];
            (edge.source, edge.kind)
        })
    }
}

/// Cached outgoing and incoming relationships of every shape and member.
///
/// This indexes every [`RelationshipType`] in both directions, including root-to-member,
/// member-to-container, and member-to-target edges. Applied traits are not relationships.
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
    pub fn get_outgoing(&self, shape: impl AsRef<str>) -> Option<Relationships<'m>> {
        let node = self.index.node_of(self.data, shape.as_ref())?;
        Some(self.outgoing_node(node))
    }

    /// Incoming relationships of a shape or member, sorted by source ID and then by
    /// relationship kind. Each edge keeps its original direction: [`Relationship::source`] is
    /// the referring shape and [`Relationship::target`] is the queried shape.
    ///
    /// Returns `None` if no such shape or member exists.
    pub fn get_incoming(&self, shape: impl AsRef<str>) -> Option<Relationships<'m>> {
        let node = self.index.node_of(self.data, shape.as_ref())?;
        Some(self.incoming_node(node))
    }

    pub(crate) fn outgoing_of(&self, shape: ShapeRef<'m>) -> Relationships<'m> {
        self.outgoing_node(self.node(shape.id()))
    }

    pub(crate) fn incoming_of(&self, shape: ShapeRef<'m>) -> Relationships<'m> {
        self.incoming_node(self.node(shape.id()))
    }

    /// The node of a shape known to exist in this model.
    pub(crate) fn node(&self, id: &ShapeId) -> Node {
        self.index
            .node_of(self.data, id.as_str())
            .expect("views always belong to their model")
    }

    pub(crate) fn lookup(&self, id: &str) -> Option<(Node, ShapeRef<'m>)> {
        let node = self.index.node_of(self.data, id)?;
        Some((node, self.shape(node)))
    }

    pub(crate) fn shape(&self, node: Node) -> ShapeRef<'m> {
        self.index.shape(self.data, node)
    }

    /// The root shape of a node known to be a root.
    pub(crate) fn root(&self, node: Node) -> ShapeView<'m> {
        self.shape(node)
            .as_shape()
            .expect("containment and binding edges only connect root shapes")
    }

    fn outgoing_node(&self, node: Node) -> Relationships<'m> {
        Relationships {
            index: *self,
            node,
            edges: EdgeIds::Outgoing(self.index.outgoing_range(node)),
        }
    }

    fn incoming_node(&self, node: Node) -> Relationships<'m> {
        Relationships {
            index: *self,
            node,
            edges: EdgeIds::Incoming(self.index.incoming_slice(node).iter()),
        }
    }
}

impl fmt::Debug for RelationshipIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelationshipIndex")
            .field("edges", &self.index.edges.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
enum EdgeIds<'m> {
    Outgoing(Range<u32>),
    Incoming(std::slice::Iter<'m, u32>),
}

/// An iterator over cached [`Relationship`]s, returned by [`RelationshipIndex`] and by
/// `relationships`/`incoming_relationships` on shape views.
#[derive(Clone)]
pub struct Relationships<'m> {
    index: RelationshipIndex<'m>,
    /// The queried shape: the source of outgoing edges, or the target of incoming edges.
    node: Node,
    edges: EdgeIds<'m>,
}

impl<'m> Relationships<'m> {
    fn relationship(&self, edge: u32) -> Relationship<'m> {
        let edge = self.index.index.edges[edge as usize];
        Relationship::new(
            self.index.shape(edge.source),
            edge.kind,
            self.index.shape(edge.target),
        )
    }
}

impl<'m> Iterator for Relationships<'m> {
    type Item = Relationship<'m>;

    fn next(&mut self) -> Option<Relationship<'m>> {
        let edge = match &mut self.edges {
            EdgeIds::Outgoing(range) => range.next()?,
            EdgeIds::Incoming(ids) => *ids.next()?,
        };
        Some(self.relationship(edge))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match &self.edges {
            EdgeIds::Outgoing(range) => range.len(),
            EdgeIds::Incoming(ids) => ids.len(),
        };
        (len, Some(len))
    }
}

impl ExactSizeIterator for Relationships<'_> {}

impl FusedIterator for Relationships<'_> {}

impl fmt::Debug for Relationships<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match self.edges {
            EdgeIds::Outgoing(_) => "outgoing",
            EdgeIds::Incoming(_) => "incoming",
        };
        f.debug_struct("Relationships")
            .field("shape", self.index.shape(self.node).id())
            .field("direction", &direction)
            .field("remaining", &self.len())
            .finish()
    }
}

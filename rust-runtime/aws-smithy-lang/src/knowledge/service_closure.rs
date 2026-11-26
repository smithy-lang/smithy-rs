/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Every shape and member reachable from a service.

use super::KnowledgeIndex;
use crate::model::Model;
use crate::relationships::{NodeId, RelationshipIndex};
use crate::shape::{ServiceView, ShapeRef, ShapeType};
use crate::shape_id::ShapeId;
use std::fmt;
use std::sync::OnceLock;

/// The closure of each service: the service and every shape and member reachable from it.
///
/// A closure follows every [relationship](crate::relationships::RelationshipType), as
/// [`Walker::new`](crate::traversal::Walker::new) does: operations and resources (including
/// those bound through resources), their inputs, outputs, and errors, resource identifiers and
/// properties, and every member and member target, recursively. Applied traits are not
/// relationships, so a shape that is only referenced by a trait value is not in the closure.
///
/// Each closure is computed the first time its service is requested and kept for the model's
/// lifetime, shared by every clone of the model. It is stored with one bit per
/// [node](crate::relationships::NodeId), so membership tests take constant time. This makes the
/// index a good basis for other indexes that scope their answers to a service.
///
/// ```
/// use aws_smithy_lang::knowledge::ServiceClosureIndex;
/// use aws_smithy_lang::{Model, ShapeId};
///
/// let model = Model::from_idl_str("things.smithy", r#"
/// $version: "2"
/// namespace ex
///
/// service Things { resources: [Thing] }
/// resource Thing { read: GetThing }
///
/// @readonly
/// operation GetThing {
///     output: GetThingOutput
/// }
///
/// structure GetThingOutput { name: Name }
///
/// string Name
/// string Unused
/// "#)?;
///
/// let closures = model.knowledge::<ServiceClosureIndex<'_>>();
/// let closure = closures.get(&ShapeId::from_static("ex#Things")).unwrap();
/// assert!(closure.contains(&ShapeId::from_static("ex#Name")));
/// assert!(closure.contains(&ShapeId::from_static("ex#GetThingOutput$name")));
/// assert!(!closure.contains(&ShapeId::from_static("ex#Unused")));
///
/// let shapes: Vec<_> = closure
///     .shapes()
///     .filter_map(|shape| shape.as_shape())
///     .filter(|shape| !shape.is_prelude())
///     .map(|shape| shape.id().to_string())
///     .collect();
/// assert_eq!(shapes, ["ex#Things", "ex#Thing", "ex#GetThing", "ex#GetThingOutput", "ex#Name"]);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy)]
pub struct ServiceClosureIndex<'m> {
    relationships: RelationshipIndex<'m>,
    closures: &'m ServiceClosures,
}

impl<'m> ServiceClosureIndex<'m> {
    /// The closure of `service`. Returns `None` if `service` does not exist or is not a service.
    pub fn get(&self, service: &ShapeId) -> Option<ServiceClosure<'m>> {
        let node = self.relationships.node(service)?;
        let nodes = self.closures.get(self.relationships, node)?;
        Some(ServiceClosure {
            relationships: self.relationships,
            service: self
                .relationships
                .root(node)
                .as_service()
                .expect("only services have closures"),
            nodes,
        })
    }
}

impl<'m> KnowledgeIndex<'m> for ServiceClosureIndex<'m> {
    fn from_model(model: &'m Model) -> Self {
        let relationships = model.knowledge::<RelationshipIndex<'_>>();
        Self {
            relationships,
            closures: model.data().knowledge.service_closures(relationships),
        }
    }
}

impl fmt::Debug for ServiceClosureIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceClosureIndex")
            .finish_non_exhaustive()
    }
}

/// The closure of one service, returned by [`ServiceClosureIndex::get`].
#[derive(Clone, Copy)]
pub struct ServiceClosure<'m> {
    relationships: RelationshipIndex<'m>,
    service: ServiceView<'m>,
    pub(super) nodes: &'m NodeSet,
}

impl<'m> ServiceClosure<'m> {
    /// The service.
    pub fn service(&self) -> ServiceView<'m> {
        self.service
    }

    /// The number of shapes and members in the closure, including the service.
    pub fn len(&self) -> usize {
        self.nodes.len
    }

    /// Always `false`: every closure contains its service.
    pub fn is_empty(&self) -> bool {
        self.nodes.len == 0
    }

    /// Returns `true` if the shape or member `shape` is in the closure.
    pub fn contains(&self, shape: &ShapeId) -> bool {
        self.relationships
            .node(shape)
            .is_some_and(|node| self.nodes.contains(node))
    }

    /// Returns `true` if `node` is in the closure. `node` must be a node of this model.
    pub fn contains_node(&self, node: NodeId) -> bool {
        self.nodes.contains(node)
    }

    /// The nodes in the closure, in ascending order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + 'm {
        self.nodes.iter()
    }

    /// The shapes and members in the closure, in [`Model::shapes`](crate::Model::shapes) order
    /// with each shape followed by its members.
    pub fn shapes(&self) -> impl Iterator<Item = ShapeRef<'m>> + 'm {
        let relationships = self.relationships;
        self.nodes().map(move |node| relationships.shape(node))
    }
}

impl fmt::Debug for ServiceClosure<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceClosure")
            .field("service", self.service.id())
            .field("len", &self.len())
            .finish()
    }
}

/// One lazily computed closure per service.
pub(crate) struct ServiceClosures {
    /// Every service node, in ascending order.
    services: Box<[(NodeId, OnceLock<NodeSet>)]>,
}

impl ServiceClosures {
    pub(crate) fn new(relationships: RelationshipIndex<'_>) -> Self {
        let services = relationships
            .data
            .declarations()
            .filter(|shape| shape.shape_type() == ShapeType::Service)
            .map(|shape| (relationships.expect_node(shape.id()), OnceLock::new()))
            .collect();
        Self { services }
    }

    /// The closure of `service`, or `None` if it is not a service.
    fn get(&self, relationships: RelationshipIndex<'_>, service: NodeId) -> Option<&NodeSet> {
        let slot = self
            .services
            .binary_search_by_key(&service, |(node, _)| *node)
            .ok()?;
        Some(self.services[slot].1.get_or_init(|| {
            let mut nodes = NodeSet::new(relationships.node_count());
            nodes.insert(service);
            let mut stack = vec![service];
            while let Some(node) = stack.pop() {
                for edge in relationships.outgoing(node) {
                    if nodes.insert(edge.target()) {
                        stack.push(edge.target());
                    }
                }
            }
            nodes
        }))
    }
}

/// A set of nodes of one model, with one bit per node.
pub(crate) struct NodeSet {
    words: Box<[u64]>,
    len: usize,
}

impl NodeSet {
    fn new(node_count: usize) -> Self {
        Self {
            words: vec![0; node_count.div_ceil(64)].into(),
            len: 0,
        }
    }

    /// Adds `node`, returning `true` if it was not already present.
    fn insert(&mut self, node: NodeId) -> bool {
        let (word, bit) = (node.index() / 64, 1u64 << (node.index() % 64));
        let added = self.words[word] & bit == 0;
        self.words[word] |= bit;
        self.len += usize::from(added);
        added
    }

    fn contains(&self, node: NodeId) -> bool {
        let (word, bit) = (node.index() / 64, 1u64 << (node.index() % 64));
        self.words.get(word).is_some_and(|word| word & bit != 0)
    }

    fn iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.words.iter().enumerate().flat_map(|(word, &bits)| {
            let remaining = Some(bits).filter(|&bits| bits != 0);
            std::iter::successors(remaining, |&bits| {
                Some(bits & (bits - 1)).filter(|&b| b != 0)
            })
            .map(move |bits| NodeId::from_index(word * 64 + bits.trailing_zeros() as usize))
        })
    }
}

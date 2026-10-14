/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Service-scoped parents of operations and resources.

use super::{Iter, RelationshipIndex, TopDownIndex};
use crate::shape::{ResourceView, ServiceView, ShapeRef, ShapeType, ShapeView};
use crate::shape_id::ShapeId;
use crate::traversal::RelationshipType;
use std::collections::HashSet;
use std::fmt;

/// A service or resource: something that can contain operations and resources.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub enum EntityView<'m> {
    /// A service.
    Service(ServiceView<'m>),
    /// A resource.
    Resource(ResourceView<'m>),
}

impl<'m> EntityView<'m> {
    fn new(shape: ShapeView<'m>) -> Self {
        if let Some(service) = shape.as_service() {
            EntityView::Service(service)
        } else {
            EntityView::Resource(
                shape
                    .as_resource()
                    .expect("only services and resources contain other shapes"),
            )
        }
    }

    /// The shape ID.
    pub fn id(&self) -> &'m ShapeId {
        self.as_shape().id()
    }

    /// The underlying untyped shape view.
    pub fn as_shape(&self) -> ShapeView<'m> {
        match self {
            EntityView::Service(service) => service.as_shape(),
            EntityView::Resource(resource) => resource.as_shape(),
        }
    }

    /// The service, if this is one.
    pub fn as_service(&self) -> Option<ServiceView<'m>> {
        match self {
            EntityView::Service(service) => Some(*service),
            EntityView::Resource(_) => None,
        }
    }

    /// The resource, if this is one.
    pub fn as_resource(&self) -> Option<ResourceView<'m>> {
        match self {
            EntityView::Resource(resource) => Some(*resource),
            EntityView::Service(_) => None,
        }
    }
}

impl<'m> From<EntityView<'m>> for ShapeRef<'m> {
    fn from(value: EntityView<'m>) -> Self {
        ShapeRef::Shape(value.as_shape())
    }
}

/// One binding of an operation or resource to a parent service or resource.
#[derive(Clone, Copy, Debug)]
pub struct ParentBinding<'m> {
    parent: EntityView<'m>,
    kind: RelationshipType,
}

impl<'m> ParentBinding<'m> {
    /// The parent service or resource.
    pub fn parent(&self) -> EntityView<'m> {
        self.parent
    }

    /// How the parent binds the child, such as [`RelationshipType::Read`] or
    /// [`RelationshipType::Resource`].
    pub fn kind(&self) -> RelationshipType {
        self.kind
    }
}

/// The unique chain of parents of an operation or resource within a service: the direct
/// parent first and the service last.
#[derive(Clone, Debug)]
pub struct ParentChain<'m> {
    entities: Vec<EntityView<'m>>,
}

impl<'m> ParentChain<'m> {
    /// The number of entities in the chain. A shape bound directly to the service has a chain
    /// of length one.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Always `false`: every chain ends with its service.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// The direct parent.
    pub fn parent(&self) -> EntityView<'m> {
        self.entities[0]
    }

    /// The service at the end of the chain.
    pub fn service(&self) -> ServiceView<'m> {
        self.entities
            .last()
            .and_then(EntityView::as_service)
            .expect("every chain ends with its service")
    }

    /// Iterates from the direct parent to the service.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = EntityView<'m>> + '_ {
        self.entities.iter().copied()
    }
}

impl<'m> IntoIterator for ParentChain<'m> {
    type Item = EntityView<'m>;
    type IntoIter = Iter<EntityView<'m>>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self.entities)
    }
}

/// An operation or resource has more than one parent within a service, so a unique parent or
/// parent chain does not exist.
///
/// Use [`BottomUpIndex::get_direct_bindings`] or [`BottomUpIndex::get_ancestors`] to see every
/// parent.
#[derive(Debug, Clone)]
pub struct AmbiguousParentError {
    service: ShapeId,
    entity: ShapeId,
    candidates: Vec<ShapeId>,
}

impl AmbiguousParentError {
    /// The service the query was scoped to.
    pub fn service(&self) -> &ShapeId {
        &self.service
    }

    /// The operation or resource with more than one parent. For
    /// [`BottomUpIndex::parent_chain`], this may be an ancestor of the queried shape.
    pub fn entity(&self) -> &ShapeId {
        &self.entity
    }

    /// The candidate parents, sorted by shape ID.
    pub fn candidates(&self) -> &[ShapeId] {
        &self.candidates
    }
}

impl fmt::Display for AmbiguousParentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` has multiple parents in service `{}`: ",
            self.entity, self.service
        )?;
        for (i, candidate) in self.candidates.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "`{candidate}`")?;
        }
        Ok(())
    }
}

impl std::error::Error for AmbiguousParentError {}

/// Parents of operations and resources, scoped to one service.
///
/// Every query takes a service and an operation or resource contained in it. Parents outside
/// the service are ignored, so a resource shared by two services has independent answers in
/// each.
///
/// Smithy models can bind one operation or resource through more than one parent (for example,
/// directly to the service and through a resource). This index exposes every binding rather
/// than silently choosing one; [`unique_parent`](Self::unique_parent) and
/// [`parent_chain`](Self::parent_chain) return [`AmbiguousParentError`] in that case.
#[derive(Clone, Copy)]
pub struct BottomUpIndex<'m> {
    relationships: RelationshipIndex<'m>,
}

/// A validated query: a service, an entity contained in it, and the service's closure.
struct Scope {
    service: u32,
    entity: u32,
    /// The service and everything it contains.
    members: HashSet<u32>,
}

impl<'m> BottomUpIndex<'m> {
    pub(crate) fn new(relationships: RelationshipIndex<'m>) -> Self {
        Self { relationships }
    }

    /// Every binding of `operation_or_resource` to a parent within `service`, sorted by parent
    /// ID and then by relationship kind.
    ///
    /// Returns `None` if `service` is not a service, `operation_or_resource` is not an operation
    /// or resource, or the service does not contain it.
    pub fn get_direct_bindings(
        &self,
        service: impl AsRef<str>,
        operation_or_resource: impl AsRef<str>,
    ) -> Option<Iter<ParentBinding<'m>>> {
        let scope = self.scope(service.as_ref(), operation_or_resource.as_ref())?;
        let bindings = self
            .parents(&scope, scope.entity)
            .map(|(parent, kind)| ParentBinding {
                parent: self.entity(parent),
                kind,
            })
            .collect();
        Some(Iter::new(bindings))
    }

    /// Every distinct service or resource above `operation_or_resource` within `service`, on
    /// any path, sorted by shape ID. Always includes the service.
    ///
    /// Returns `None` under the same conditions as
    /// [`get_direct_bindings`](Self::get_direct_bindings).
    pub fn get_ancestors(
        &self,
        service: impl AsRef<str>,
        operation_or_resource: impl AsRef<str>,
    ) -> Option<Iter<EntityView<'m>>> {
        let scope = self.scope(service.as_ref(), operation_or_resource.as_ref())?;
        let mut visited = HashSet::new();
        let mut stack = vec![scope.entity];
        while let Some(node) = stack.pop() {
            for (parent, _) in self.parents(&scope, node) {
                if visited.insert(parent) {
                    stack.push(parent);
                }
            }
        }
        let mut ancestors: Vec<_> = visited.into_iter().map(|n| self.entity(n)).collect();
        ancestors.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        Some(Iter::new(ancestors))
    }

    /// The single parent of `operation_or_resource` within `service`.
    ///
    /// Returns `Ok(None)` under the same conditions as
    /// [`get_direct_bindings`](Self::get_direct_bindings), and an error if more than one
    /// service or resource binds it. One parent binding the shape in several ways (for
    /// example, as both `read` and an instance operation) is still a unique parent.
    pub fn unique_parent(
        &self,
        service: impl AsRef<str>,
        operation_or_resource: impl AsRef<str>,
    ) -> Result<Option<EntityView<'m>>, AmbiguousParentError> {
        let Some(scope) = self.scope(service.as_ref(), operation_or_resource.as_ref()) else {
            return Ok(None);
        };
        self.unique_parent_of(&scope, scope.entity)
            .map(|parent| Some(self.entity(parent)))
    }

    /// The parents of `operation_or_resource` within `service`, from its direct parent to the
    /// service.
    ///
    /// Returns `Ok(None)` under the same conditions as
    /// [`get_direct_bindings`](Self::get_direct_bindings), and an error at the first step with
    /// more than one parent.
    pub fn parent_chain(
        &self,
        service: impl AsRef<str>,
        operation_or_resource: impl AsRef<str>,
    ) -> Result<Option<ParentChain<'m>>, AmbiguousParentError> {
        let Some(scope) = self.scope(service.as_ref(), operation_or_resource.as_ref()) else {
            return Ok(None);
        };
        let mut entities = Vec::new();
        let mut node = scope.entity;
        while node != scope.service {
            node = self.unique_parent_of(&scope, node)?;
            if entities.len() > scope.members.len() {
                unreachable!("validated models have no resource containment cycles");
            }
            entities.push(self.entity(node));
        }
        Ok(Some(ParentChain { entities }))
    }

    fn scope(&self, service: &str, entity: &str) -> Option<Scope> {
        let (service, service_shape) = self.relationships.lookup(service)?;
        let (entity, entity_shape) = self.relationships.lookup(entity)?;
        if service_shape.shape_type() != ShapeType::Service
            || !matches!(
                entity_shape.shape_type(),
                ShapeType::Operation | ShapeType::Resource
            )
        {
            return None;
        }
        let mut members = TopDownIndex::new(self.relationships).closure(service);
        if !members.contains(&entity) {
            return None;
        }
        members.insert(service);
        Some(Scope {
            service,
            entity,
            members,
        })
    }

    /// Containment parents of `node` within the scope, sorted by parent ID and then kind.
    fn parents<'s>(
        &self,
        scope: &'s Scope,
        node: u32,
    ) -> impl Iterator<Item = (u32, RelationshipType)> + 's
    where
        'm: 's,
    {
        self.relationships
            .index
            .incoming_edges(node)
            .filter(move |(source, kind)| kind.is_containment() && scope.members.contains(source))
    }

    fn unique_parent_of(&self, scope: &Scope, node: u32) -> Result<u32, AmbiguousParentError> {
        let mut parents: Vec<u32> = self.parents(scope, node).map(|(p, _)| p).collect();
        // Parents are sorted by ID, so repeated bindings from one parent are adjacent.
        parents.dedup();
        match parents.as_slice() {
            [parent] => Ok(*parent),
            _ => Err(AmbiguousParentError {
                service: self.relationships.shape(scope.service).id().clone(),
                entity: self.relationships.shape(node).id().clone(),
                candidates: parents
                    .iter()
                    .map(|&p| self.relationships.shape(p).id().clone())
                    .collect(),
            }),
        }
    }

    fn entity(&self, node: u32) -> EntityView<'m> {
        EntityView::new(self.relationships.root(node))
    }
}

impl fmt::Debug for BottomUpIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BottomUpIndex").finish_non_exhaustive()
    }
}

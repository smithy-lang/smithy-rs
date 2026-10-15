/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Complete service and resource closures.

use super::Iter;
use crate::relationships::RelationshipIndex;
use crate::shape::{OperationView, ResourceView, ServiceView, ShapeType};
use std::collections::HashSet;
use std::fmt;

/// Every operation and resource contained in a service or resource.
///
/// Containment follows only [containment relationships](crate::relationships::RelationshipType::is_containment):
/// directly bound operations, resource lifecycle operations (`create`, `put`, `read`, `update`,
/// `delete`, and `list`), instance and collection operations, and child resources, recursively.
/// The starting service or resource is excluded. Results are deduplicated and sorted by shape
/// ID.
///
/// Closures are computed per query from the cached relationship index rather than stored, so
/// persistent memory stays proportional to the model size.
///
/// The typed [`ServiceView::contained_operations`], [`ServiceView::contained_resources`],
/// [`ResourceView::contained_operations`], and [`ResourceView::contained_resources`]
/// conveniences delegate to this index.
#[derive(Clone, Copy)]
pub struct TopDownIndex<'m> {
    relationships: RelationshipIndex<'m>,
}

impl<'m> TopDownIndex<'m> {
    pub(crate) fn new(relationships: RelationshipIndex<'m>) -> Self {
        Self { relationships }
    }

    /// Every operation contained in a service or resource, sorted by shape ID.
    ///
    /// Returns `None` if `entity` does not exist or is neither a service nor a resource.
    pub fn get_contained_operations(
        &self,
        entity: impl AsRef<str>,
    ) -> Option<Iter<OperationView<'m>>> {
        Some(self.operations_of(self.entity(entity.as_ref())?))
    }

    /// Every resource contained in a service or resource, sorted by shape ID.
    ///
    /// Returns `None` if `entity` does not exist or is neither a service nor a resource.
    pub fn get_contained_resources(
        &self,
        entity: impl AsRef<str>,
    ) -> Option<Iter<ResourceView<'m>>> {
        Some(self.resources_of(self.entity(entity.as_ref())?))
    }

    /// The node of a service or resource.
    fn entity(&self, id: &str) -> Option<u32> {
        let (node, shape) = self.relationships.lookup(id)?;
        matches!(shape.shape_type(), ShapeType::Service | ShapeType::Resource).then_some(node)
    }

    /// Every node contained in `start`, excluding `start`, in no particular order.
    pub(crate) fn closure(&self, start: u32) -> HashSet<u32> {
        let mut visited = HashSet::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for (kind, target) in self.relationships.index.outgoing_edges(node) {
                if kind.is_containment() && target != start && visited.insert(target) {
                    stack.push(target);
                }
            }
        }
        visited
    }

    /// Returns `true` if `target` is contained in `start`.
    pub(crate) fn contains(&self, start: u32, target: u32) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for (kind, next) in self.relationships.index.outgoing_edges(node) {
                if !kind.is_containment() {
                    continue;
                }
                if next == target {
                    return true;
                }
                if visited.insert(next) {
                    stack.push(next);
                }
            }
        }
        false
    }

    fn operations_of(&self, start: u32) -> Iter<OperationView<'m>> {
        let mut operations: Vec<_> = self
            .closure(start)
            .into_iter()
            .filter_map(|node| self.relationships.root(node).as_operation())
            .collect();
        operations.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        Iter::new(operations)
    }

    fn resources_of(&self, start: u32) -> Iter<ResourceView<'m>> {
        let mut resources: Vec<_> = self
            .closure(start)
            .into_iter()
            .filter_map(|node| self.relationships.root(node).as_resource())
            .collect();
        resources.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        Iter::new(resources)
    }
}

impl fmt::Debug for TopDownIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopDownIndex").finish_non_exhaustive()
    }
}

fn index_for<'m>(shape: crate::shape::ShapeView<'m>) -> (TopDownIndex<'m>, u32) {
    let relationships = RelationshipIndex::new(shape.data);
    let node = relationships.node(shape.id());
    (TopDownIndex::new(relationships), node)
}

impl<'a> ServiceView<'a> {
    /// Every operation in the service, sorted by shape ID: operations bound directly to the
    /// service and every operation bound through its resources, including lifecycle and
    /// collection operations of nested resources.
    ///
    /// This is the complete set to use for code generation, validation, and auditing. Use
    /// [`direct_operations`](Self::direct_operations) only to inspect what the service
    /// declaration itself lists. Delegates to [`TopDownIndex`].
    pub fn contained_operations(&self) -> Iter<OperationView<'a>> {
        let (index, node) = index_for(self.as_shape());
        index.operations_of(node)
    }

    /// Every resource in the service, including nested child resources, sorted by shape ID.
    ///
    /// Use [`direct_resources`](Self::direct_resources) only to inspect what the service
    /// declaration itself lists. Delegates to [`TopDownIndex`].
    pub fn contained_resources(&self) -> Iter<ResourceView<'a>> {
        let (index, node) = index_for(self.as_shape());
        index.resources_of(node)
    }
}

impl<'a> ResourceView<'a> {
    /// Every operation in the resource, sorted by shape ID: lifecycle, instance, and collection
    /// operations of this resource and of every nested child resource.
    ///
    /// Use [`direct_operations`](Self::direct_operations) only to inspect what the resource
    /// declaration itself lists. Delegates to [`TopDownIndex`].
    pub fn contained_operations(&self) -> Iter<OperationView<'a>> {
        let (index, node) = index_for(self.as_shape());
        index.operations_of(node)
    }

    /// Every nested resource (children, grandchildren, and so on), sorted by shape ID.
    ///
    /// Use [`child_resources`](Self::child_resources) only to inspect what the resource
    /// declaration itself lists. Delegates to [`TopDownIndex`].
    pub fn contained_resources(&self) -> Iter<ResourceView<'a>> {
        let (index, node) = index_for(self.as_shape());
        index.resources_of(node)
    }
}

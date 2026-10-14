/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Operation input, output, errors, and reverse bindings.

use super::{Iter, RelationshipIndex, TopDownIndex};
use crate::model::unit_id;
use crate::shape::{MembersView, OperationView, ShapeRef, ShapeType, StructureView};
use crate::shape_id::ShapeId;
use crate::traits;
use crate::traversal::RelationshipType;
use indexmap::IndexSet;
use std::fmt;

/// Operation input, output, and errors, plus which operations and services bind a structure.
///
/// Reverse bindings are derived from the cached [`RelationshipIndex`].
#[derive(Clone, Copy)]
pub struct OperationIndex<'m> {
    relationships: RelationshipIndex<'m>,
}

impl<'m> OperationIndex<'m> {
    pub(crate) fn new(relationships: RelationshipIndex<'m>) -> Self {
        Self { relationships }
    }

    /// Knowledge about an operation. Returns `None` if `operation` does not exist or is not an
    /// operation.
    pub fn get(&self, operation: impl AsRef<str>) -> Option<OperationKnowledge<'m>> {
        let (node, shape) = self.relationships.lookup(operation.as_ref())?;
        Some(OperationKnowledge {
            operation: shape.as_operation()?,
            node,
            relationships: self.relationships,
        })
    }

    /// Operations whose input is `structure`, sorted by shape ID. Operations that omit their
    /// input bind `smithy.api#Unit`.
    ///
    /// Returns `None` if `structure` does not exist or is not a structure.
    pub fn input_bindings(&self, structure: impl AsRef<str>) -> Option<Iter<OperationView<'m>>> {
        self.operation_bindings(structure.as_ref(), RelationshipType::Input)
    }

    /// Operations whose output is `structure`, sorted by shape ID. Operations that omit their
    /// output bind `smithy.api#Unit`.
    ///
    /// Returns `None` if `structure` does not exist or is not a structure.
    pub fn output_bindings(&self, structure: impl AsRef<str>) -> Option<Iter<OperationView<'m>>> {
        self.operation_bindings(structure.as_ref(), RelationshipType::Output)
    }

    /// Operations and services that list `error` in their `errors`, sorted by shape ID.
    ///
    /// Returns `None` if `error` does not exist or is not a structure.
    pub fn error_bindings(&self, error: impl AsRef<str>) -> Option<Iter<ShapeRef<'m>>> {
        let node = self.structure(error.as_ref())?;
        let bindings = self
            .bindings(node, RelationshipType::Error)
            .map(|source| self.relationships.shape(source))
            .collect();
        Some(Iter::new(bindings))
    }

    /// Returns `true` if `structure` is bound as an operation input or is marked with
    /// `@input`.
    pub fn is_input_structure(&self, structure: impl AsRef<str>) -> bool {
        self.is_io_structure(structure.as_ref(), RelationshipType::Input, traits::INPUT)
    }

    /// Returns `true` if `structure` is bound as an operation output or is marked with
    /// `@output`.
    pub fn is_output_structure(&self, structure: impl AsRef<str>) -> bool {
        self.is_io_structure(structure.as_ref(), RelationshipType::Output, traits::OUTPUT)
    }

    fn is_io_structure(&self, id: &str, kind: RelationshipType, marker: &str) -> bool {
        let Some(node) = self.structure(id) else {
            return false;
        };
        self.relationships.shape(node).has_trait(marker)
            || self.bindings(node, kind).next().is_some()
    }

    fn structure(&self, id: &str) -> Option<u32> {
        let (node, shape) = self.relationships.lookup(id)?;
        (shape.shape_type() == ShapeType::Structure).then_some(node)
    }

    /// Sources of incoming `kind` edges, already sorted by source ID.
    fn bindings(&self, node: u32, kind: RelationshipType) -> impl Iterator<Item = u32> + 'm {
        self.relationships
            .index
            .incoming_edges(node)
            .filter(move |(_, edge)| *edge == kind)
            .map(|(source, _)| source)
    }

    fn operation_bindings(
        &self,
        id: &str,
        kind: RelationshipType,
    ) -> Option<Iter<OperationView<'m>>> {
        let node = self.structure(id)?;
        let operations = self
            .bindings(node, kind)
            .map(|source| {
                self.relationships
                    .root(source)
                    .as_operation()
                    .expect("only operations bind input and output")
            })
            .collect();
        Some(Iter::new(operations))
    }
}

impl fmt::Debug for OperationIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationIndex").finish_non_exhaustive()
    }
}

/// Knowledge about one operation, returned by [`OperationIndex::get`].
#[derive(Clone, Copy)]
pub struct OperationKnowledge<'m> {
    operation: OperationView<'m>,
    node: u32,
    relationships: RelationshipIndex<'m>,
}

impl<'m> OperationKnowledge<'m> {
    /// The operation.
    pub fn operation(&self) -> OperationView<'m> {
        self.operation
    }

    /// The input structure; `smithy.api#Unit` when the input is omitted.
    pub fn input_shape(&self) -> StructureView<'m> {
        structure(self.operation.input_shape().as_structure())
    }

    /// The output structure; `smithy.api#Unit` when the output is omitted.
    pub fn output_shape(&self) -> StructureView<'m> {
        structure(self.operation.output_shape().as_structure())
    }

    /// The input structure, or `None` if it is `smithy.api#Unit`.
    pub fn meaningful_input(&self) -> Option<StructureView<'m>> {
        Some(self.input_shape()).filter(|s| s.id() != unit_id())
    }

    /// The output structure, or `None` if it is `smithy.api#Unit`.
    pub fn meaningful_output(&self) -> Option<StructureView<'m>> {
        Some(self.output_shape()).filter(|s| s.id() != unit_id())
    }

    /// The input members in declaration order; empty for `smithy.api#Unit`.
    pub fn input_members(&self) -> MembersView<'m> {
        self.input_shape().members()
    }

    /// The output members in declaration order; empty for `smithy.api#Unit`.
    pub fn output_members(&self) -> MembersView<'m> {
        self.output_shape().members()
    }

    /// **Declaration-only:** the errors listed directly on the operation, in declaration order.
    ///
    /// This omits the common errors of any service. Use [`errors_in`](Self::errors_in) for the
    /// errors the operation can return within a service.
    pub fn direct_errors(&self) -> Iter<StructureView<'m>> {
        Iter::new(
            self.operation
                .direct_errors()
                .map(|id| self.error(id))
                .collect(),
        )
    }

    /// Every error the operation can return within `service`: the service's common errors in
    /// declaration order, then the operation's own errors in declaration order, keeping only the
    /// first occurrence of each.
    ///
    /// Returns `None` if `service` does not exist, is not a service, or does not contain this
    /// operation (directly or through any of its resources).
    pub fn errors_in(&self, service: impl AsRef<str>) -> Option<Iter<StructureView<'m>>> {
        let (service_node, service) = self.relationships.lookup(service.as_ref())?;
        let service = service.as_service()?;
        if !TopDownIndex::new(self.relationships).contains(service_node, self.node) {
            return None;
        }
        let ids: IndexSet<&ShapeId> = service
            .common_errors()
            .chain(self.operation.direct_errors())
            .collect();
        Some(Iter::new(
            ids.into_iter().map(|id| self.error(id)).collect(),
        ))
    }

    fn error(&self, id: &ShapeId) -> StructureView<'m> {
        let (_, shape) = self
            .relationships
            .lookup(id.as_str())
            .expect("validated models only contain resolved errors");
        structure(shape.as_structure())
    }
}

fn structure(view: Option<StructureView<'_>>) -> StructureView<'_> {
    view.expect("validated operations only reference structures")
}

impl fmt::Debug for OperationKnowledge<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationKnowledge")
            .field("operation", self.operation.id())
            .finish()
    }
}

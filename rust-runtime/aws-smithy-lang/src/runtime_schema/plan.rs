/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Selection and validation: the plan of what a [`RuntimeSchemaSet`](super::RuntimeSchemaSet)
//! contains.
//!
//! [`plan`] selects the roots, members, and operations of a scope, decodes every
//! schema-relevant trait, and checks collection cycles and index limits, reporting every
//! problem before anything is allocated. The resulting [`Plan`] borrows nothing: it refers
//! to model shapes by position and holds one empty slot per root, member, and operation,
//! which `materialize` fills. Because the model is immutable, a plan that was
//! built successfully can be materialized later, in any order, without new errors.

use super::facts::{OperationFacts, TraitFacts};
use super::owner::{SchemaHandle, SchemaSlot, SliceHandle};
use super::SchemaBuildError;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSet};
use crate::model::ModelData;
use crate::shape::{OperationView, ServiceView, ShapeView};
use crate::{Model, ShapeId, ShapeType};
use indexmap::{IndexMap, IndexSet};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

type SchemaShapeId<'a> = aws_smithy_schema::ShapeId<'a>;

type SchemaShapeType = aws_smithy_schema::ShapeType;

/// Which shapes to materialize.
#[derive(Debug, Clone)]
pub(super) enum Scope {
    /// Every data shape and operation outside the prelude, and the prelude shapes they use.
    AllShapes,
    /// The operations of one service (including those bound through resources), their
    /// inputs, outputs, and errors, and every shape those reach.
    Service {
        service: ShapeId,
        operations: OperationFilter,
    },
}

/// Which of a service's operations to select.
#[derive(Debug, Clone)]
pub(super) enum OperationFilter {
    /// Every operation the service contains, and the service's common errors.
    All,
    /// Only these operations, which must be contained in the service.
    Selected(Box<[ShapeId]>),
}

#[derive(Debug, Clone)]
pub(super) struct Options {
    pub(super) scope: Scope,
    pub(super) max_diagnostics: usize,
    /// The most roots, members, or model shapes a plan can index. `u32::MAX`, except in
    /// tests of the limit.
    pub(super) max_plan_entries: u32,
    pub(super) materialization: super::Materialization,
}

/// What a set contains. Borrows nothing; refers to model shapes by position.
#[derive(Debug)]
pub(super) struct Plan {
    /// The selected service, if built for one.
    pub(super) service: Option<ShapeId>,
    /// Selected data shapes, sorted by model position.
    pub(super) roots: Box<[RootPlan]>,
    /// The index in `roots` of the root at each model position, or [`NOT_SELECTED`]. Lets
    /// `schema` find a root with the model's own lookup, without a second table of shape
    /// IDs.
    pub(super) root_index: Box<[u32]>,
    /// The members of selected structures, unions, lists, and maps, grouped by container in
    /// `roots` order and in declaration order within each.
    pub(super) members: Box<[MemberPlan]>,
    /// Selected operations in selection order: model order, or sorted by shape ID for a
    /// service.
    pub(super) operations: IndexMap<ShapeId, OperationPlan>,
}

#[derive(Debug)]
pub(super) struct RootPlan {
    /// Position in [`ModelData::root_at`] order.
    pub(super) model_position: u32,
    /// This root's members are `members[member_start..member_start + member_len]`.
    pub(super) member_start: u32,
    /// Zero for shapes without member schemas, including enums.
    pub(super) member_len: u32,
    pub(super) schema: SchemaSlot,
}

/// [`Plan::root_index`] of a shape that was not selected.
pub(super) const NOT_SELECTED: u32 = u32::MAX;

#[derive(Debug)]
pub(super) struct MemberPlan {
    /// Index of the containing root in [`Plan::roots`].
    pub(super) container: u32,
    /// Index of the target root in [`Plan::roots`].
    pub(super) target: u32,
    pub(super) schema: SchemaSlot,
}

#[derive(Debug)]
pub(super) struct OperationPlan {
    /// Position in [`ModelData::root_at`] order.
    pub(super) model_position: u32,
    /// Indexes in [`Plan::roots`].
    pub(super) input: u32,
    pub(super) output: u32,
    /// Effective errors, in order, as indexes in [`Plan::roots`].
    pub(super) errors: Box<[u32]>,
    /// Published only when its schemas and error slice are complete (INV-6).
    pub(super) record: OnceLock<OperationRecord>,
}

/// The schemas of a materialized operation.
#[derive(Debug, Clone, Copy)]
pub(super) struct OperationRecord {
    pub(super) input: SchemaHandle,
    pub(super) output: SchemaHandle,
    pub(super) errors: SliceHandle,
}

/// The slot of a selected root or member schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    Root(usize),
    Member(usize),
}

impl Plan {
    /// The slot of a selected root or member, or `None` if `id` was not selected.
    pub(super) fn slot(&self, data: &ModelData, id: &ShapeId) -> Option<Slot> {
        let index = *self.root_index.get(data.root_position(id)?)?;
        if index == NOT_SELECTED {
            return None;
        }
        let index = index as usize;
        match id.member() {
            None => Some(Slot::Root(index)),
            Some(name) => {
                let root = &self.roots[index];
                if root.member_len == 0 {
                    return None;
                }
                let shape = data.root_at(root.model_position as usize);
                // Comparing a few names is faster than hashing one.
                let position = if root.member_len <= 8 {
                    ShapeView::new(data, shape)
                        .members()
                        .iter()
                        .position(|member| member.name() == name)?
                } else {
                    shape.member_index(name)?
                };
                Some(Slot::Member(root.member_start as usize + position))
            }
        }
    }

    /// The published schema in `slot`, if materialized.
    pub(super) fn get(&self, slot: Slot) -> Option<SchemaHandle> {
        match slot {
            Slot::Root(index) => self.roots[index].schema.get(),
            Slot::Member(index) => self.members[index].schema.get(),
        }
    }
}

/// The runtime schema type of a model shape type, or `None` for shapes that have no schema.
///
/// The match is exhaustive, so a new shape type cannot be added to this crate without
/// deciding how it is materialized.
pub(super) fn schema_type(shape_type: ShapeType) -> Option<SchemaShapeType> {
    Some(match shape_type {
        ShapeType::Blob => SchemaShapeType::Blob,
        ShapeType::Boolean => SchemaShapeType::Boolean,
        ShapeType::Document => SchemaShapeType::Document,
        ShapeType::String | ShapeType::Enum => SchemaShapeType::String,
        ShapeType::Byte => SchemaShapeType::Byte,
        ShapeType::Short => SchemaShapeType::Short,
        ShapeType::Integer | ShapeType::IntEnum => SchemaShapeType::Integer,
        ShapeType::Long => SchemaShapeType::Long,
        ShapeType::Float => SchemaShapeType::Float,
        ShapeType::Double => SchemaShapeType::Double,
        ShapeType::BigInteger => SchemaShapeType::BigInteger,
        ShapeType::BigDecimal => SchemaShapeType::BigDecimal,
        ShapeType::Timestamp => SchemaShapeType::Timestamp,
        ShapeType::List => SchemaShapeType::List,
        ShapeType::Map => SchemaShapeType::Map,
        ShapeType::Structure => SchemaShapeType::Structure,
        ShapeType::Union => SchemaShapeType::Union,
        ShapeType::Service | ShapeType::Resource | ShapeType::Operation | ShapeType::Member => {
            return None
        }
    })
}

/// Whether a shape's members are materialized as member schemas. Enum members are values,
/// not serialized members.
pub(super) fn has_member_schemas(shape_type: ShapeType) -> bool {
    matches!(
        shape_type,
        ShapeType::List | ShapeType::Map | ShapeType::Structure | ShapeType::Union
    )
}

/// A schema shape ID borrowing the parts of a model root shape ID, without allocating.
pub(super) fn root_id(id: &ShapeId) -> SchemaShapeId<'_> {
    debug_assert!(id.member().is_none(), "`{id}` is a member ID");
    SchemaShapeId::from_parts(id.as_str(), id.namespace(), id.name())
}

pub(super) fn member_id(id: &ShapeId) -> SchemaShapeId<'_> {
    match id.member() {
        Some(member) => {
            SchemaShapeId::from_parts_with_member(id.as_str(), id.namespace(), id.name(), member)
        }
        None => root_id(id),
    }
}

/// The selected shapes and operations.
struct Selection<'a> {
    roots: IndexMap<&'a ShapeId, ShapeView<'a>>,
    operations: Vec<OperationView<'a>>,
    /// The errors of each operation, in `operations` order.
    errors: Vec<Vec<&'a ShapeId>>,
}

impl<'a> Selection<'a> {
    fn select(model: &'a Model, scope: &Scope, diagnostics: &mut DiagnosticSet) -> Option<Self> {
        let mut selection = Selection {
            roots: IndexMap::new(),
            operations: Vec::new(),
            errors: Vec::new(),
        };
        match scope {
            Scope::AllShapes => {
                for shape in model.non_prelude_shapes() {
                    if schema_type(shape.shape_type()).is_some() {
                        selection.roots.insert(shape.id(), shape);
                    }
                }
                for operation in model.operations() {
                    let errors = operation.direct_errors().collect();
                    selection.add_operation(model, operation, errors);
                }
            }
            Scope::Service {
                service: id,
                operations: filter,
            } => {
                let service = select_service(model, id, diagnostics)?;
                let contained = contained_operations(model, service);
                let operations = match filter {
                    OperationFilter::All => contained,
                    OperationFilter::Selected(requested) => {
                        filter_operations(model, service, &contained, requested, diagnostics)?
                    }
                };
                for operation in operations {
                    // The service's common errors, then the operation's own, keeping the
                    // first occurrence: `OperationKnowledge::errors_in` without re-checking
                    // containment for every operation.
                    let errors: IndexSet<&ShapeId> = service
                        .common_errors()
                        .chain(operation.direct_errors())
                        .collect();
                    selection.add_operation(model, operation, errors.into_iter().collect());
                }
                // With a filter, common errors are only selected through the operations
                // that can return them.
                if let OperationFilter::All = filter {
                    for error in service.common_errors() {
                        selection.add_root(model, error);
                    }
                }
            }
        }
        selection.close(model);
        Some(selection)
    }

    fn add_operation(
        &mut self,
        model: &'a Model,
        operation: OperationView<'a>,
        errors: Vec<&'a ShapeId>,
    ) {
        self.add_root(model, operation.input());
        self.add_root(model, operation.output());
        for &error in &errors {
            self.add_root(model, error);
        }
        self.operations.push(operation);
        self.errors.push(errors);
    }

    /// Selects a data shape. Every reference in a model resolves to one.
    fn add_root(&mut self, model: &'a Model, id: &'a ShapeId) {
        if self.roots.contains_key(id) {
            return;
        }
        let shape = model
            .get_shape(id)
            .and_then(|shape| shape.as_shape())
            .expect("references resolve to root shapes");
        if schema_type(shape.shape_type()).is_some() {
            self.roots.insert(shape.id(), shape);
        }
    }

    /// Adds every shape targeted by a member of a selected shape, transitively.
    fn close(&mut self, model: &'a Model) {
        let mut next = 0;
        while let Some((_, &shape)) = self.roots.get_index(next) {
            if has_member_schemas(shape.shape_type()) {
                for member in shape.members().iter() {
                    self.add_root(model, member.target());
                }
            }
            next += 1;
        }
    }
}

/// Resolves the selected service, reporting a missing ID or a shape that is not a service.
fn select_service<'a>(
    model: &'a Model,
    id: &ShapeId,
    diagnostics: &mut DiagnosticSet,
) -> Option<ServiceView<'a>> {
    let Some(shape) = model.get_shape(id).and_then(|shape| shape.as_shape()) else {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::UnresolvedReference,
                format!("service `{id}` does not exist"),
            )
            .with_shape_id(id.clone()),
        );
        return None;
    };
    let service = shape.as_service();
    if service.is_none() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidTarget,
                format!("`{id}` is a {}, not a service", shape.shape_type()),
            )
            .with_source_location(shape.source_location().clone())
            .with_shape_id(id.clone()),
        );
    }
    service
}

/// Every operation contained in `service`, sorted by shape ID, as
/// [`TopDownIndex`](crate::knowledge::TopDownIndex) returns them: operations bound directly,
/// through resources (lifecycle, instance, and collection operations), and recursively.
///
/// Walks the service and resource declarations directly, rather than building the
/// model-wide relationship index, which costs more than the rest of a service build on large
/// models. Iterative, and visits each resource once, so shared resources and resource cycles
/// terminate.
pub(super) fn contained_operations<'a>(
    model: &'a Model,
    service: ServiceView<'a>,
) -> Vec<OperationView<'a>> {
    let operation = |id: &ShapeId| {
        model
            .get_shape(id)
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_operation())
            .expect("operation bindings resolve to operations")
    };
    let mut operations: IndexSet<&'a ShapeId> = service.direct_operations().collect();
    let mut visited: HashSet<&'a ShapeId> = HashSet::new();
    let mut stack: Vec<&'a ShapeId> = service.direct_resources().collect();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        let resource = model
            .get_shape(id)
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_resource())
            .expect("resource bindings resolve to resources");
        let lifecycle = [
            resource.create(),
            resource.put(),
            resource.read(),
            resource.update(),
            resource.delete(),
            resource.list(),
        ];
        operations.extend(lifecycle.into_iter().flatten());
        operations.extend(resource.direct_operations());
        operations.extend(resource.direct_collection_operations());
        stack.extend(resource.child_resources());
    }
    let mut operations: Vec<_> = operations.into_iter().map(operation).collect();
    operations.sort_unstable_by(|a, b| a.id().cmp(b.id()));
    operations
}

/// The requested operations of a service, in `contained` (service) order, without
/// duplicates. Reports every requested ID that is missing, is not an operation, or is not
/// contained in the service.
fn filter_operations<'a>(
    model: &'a Model,
    service: ServiceView<'a>,
    contained: &[OperationView<'a>],
    requested: &[ShapeId],
    diagnostics: &mut DiagnosticSet,
) -> Option<Vec<OperationView<'a>>> {
    let positions: HashMap<&ShapeId, usize> = contained
        .iter()
        .enumerate()
        .map(|(position, operation)| (operation.id(), position))
        .collect();
    let mut selected = vec![false; contained.len()];
    let mut valid = true;
    for id in requested {
        if let Some(&position) = positions.get(id) {
            selected[position] = true;
            continue;
        }
        valid = false;
        let diagnostic = match model.get_shape(id).and_then(|shape| shape.as_shape()) {
            None => Diagnostic::error(
                DiagnosticCode::UnresolvedReference,
                format!(
                    "operation `{id}` selected for service `{}` does not exist",
                    service.id()
                ),
            ),
            Some(shape) if shape.shape_type() != ShapeType::Operation => Diagnostic::error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "`{id}` selected for service `{}` is a {}, not an operation",
                    service.id(),
                    shape.shape_type()
                ),
            )
            .with_source_location(shape.source_location().clone()),
            Some(shape) => Diagnostic::error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "operation `{id}` is not contained in service `{}`",
                    service.id()
                ),
            )
            .with_source_location(shape.source_location().clone()),
        };
        diagnostics.push(diagnostic.with_shape_id(id.clone()));
    }
    valid.then(|| {
        contained
            .iter()
            .zip(selected)
            .filter_map(|(operation, selected)| selected.then_some(*operation))
            .collect()
    })
}

/// Trait facts decoded while planning, indexed like the plan, for materializing in the same
/// batch without decoding again.
pub(super) struct Prepared<'m> {
    pub(super) roots: Vec<TraitFacts<'m>>,
    pub(super) members: Vec<TraitFacts<'m>>,
    pub(super) operations: Vec<OperationFacts<'m>>,
    /// Every member, each after the members it references.
    pub(super) member_order: Vec<usize>,
}

/// Converts a plan index or count to `u32`, reporting a `ResourceLimit` if it exceeds `max`.
fn plan_index(value: usize, max: u32, what: &str, diagnostics: &mut DiagnosticSet) -> Option<u32> {
    match u32::try_from(value) {
        Ok(value) if value <= max => Some(value),
        _ => {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                format!("the selection has {value} {what}, more than the limit of {max}"),
            ));
            None
        }
    }
}

/// Selects and validates what `options` selects from `model`.
///
/// Reports every malformed schema-relevant trait, every list and map cycle, and selections
/// too large to index. On success, returns the plan and the facts decoded for it.
pub(super) fn plan<'m>(
    model: &'m Model,
    options: &Options,
) -> Result<(Plan, Prepared<'m>), SchemaBuildError> {
    let mut diagnostics = DiagnosticSet::with_limit(options.max_diagnostics);
    let fail = |diagnostics| Err(SchemaBuildError::new(diagnostics));
    let data = model.data();
    let max = options.max_plan_entries;

    let Some(selection) = Selection::select(model, &options.scope, &mut diagnostics) else {
        return fail(diagnostics);
    };
    // Model positions are stored as `u32`, so the whole model must fit; `NOT_SELECTED` is
    // never a valid index because the limit is at most `u32::MAX`.
    if plan_index(data.root_count(), max, "model shapes", &mut diagnostics).is_none()
        || plan_index(selection.roots.len(), max, "roots", &mut diagnostics).is_none()
    {
        return fail(diagnostics);
    }

    // Order the selected roots by model position, so `schema` can find them with the model's
    // own lookup. `order[i]` is the selection index of plan index `i`, `rank` the reverse,
    // and `by_position` the plan index at each model position.
    let positions: Vec<u32> = selection
        .roots
        .keys()
        .map(|id| {
            data.root_position(id)
                .expect("selected roots are in the model") as u32
        })
        .collect();
    let mut by_position = vec![NOT_SELECTED; data.root_count()];
    for (selection_index, &position) in positions.iter().enumerate() {
        by_position[position as usize] = selection_index as u32;
    }
    let mut order = Vec::with_capacity(positions.len());
    let mut rank = vec![0u32; positions.len()];
    for entry in by_position
        .iter_mut()
        .filter(|entry| **entry != NOT_SELECTED)
    {
        let selection_index = *entry as usize;
        rank[selection_index] = order.len() as u32;
        *entry = order.len() as u32;
        order.push(selection_index);
    }

    // Decode each schema-relevant trait application once, in plan order.
    let mut roots = Vec::with_capacity(order.len());
    let mut members = Vec::new();
    let mut prepared = Prepared {
        roots: Vec::with_capacity(order.len()),
        members: Vec::new(),
        operations: Vec::with_capacity(selection.operations.len()),
        member_order: Vec::new(),
    };
    let mut member_views = Vec::new();
    for (index, &selection_index) in order.iter().enumerate() {
        let (_, shape) = selection
            .roots
            .get_index(selection_index)
            .expect("in range");
        prepared.roots.push(TraitFacts::decode(
            shape.id(),
            shape.traits(),
            &mut diagnostics,
        ));
        let member_start = members.len();
        if has_member_schemas(shape.shape_type()) {
            for (position, view) in shape.members().iter().enumerate() {
                // `Schema::new_struct` asserts that each member schema's index is its position,
                // and member schemas are built with their position, so this holds by
                // construction; `index` hashes the name, so only check it in debug builds.
                debug_assert_eq!(view.index(), position, "`{}`", view.id());
                let target = selection
                    .roots
                    .get_index_of(view.target())
                    .expect("member targets are selected");
                members.push(MemberPlan {
                    container: index as u32,
                    target: rank[target],
                    schema: SchemaSlot::new(),
                });
                member_views.push(view);
                prepared.members.push(TraitFacts::decode(
                    view.id(),
                    view.traits(),
                    &mut diagnostics,
                ));
            }
        }
        if plan_index(members.len(), max, "members", &mut diagnostics).is_none() {
            return fail(diagnostics);
        }
        roots.push(RootPlan {
            model_position: positions[selection_index],
            member_start: member_start as u32,
            member_len: (members.len() - member_start) as u32,
            schema: SchemaSlot::new(),
        });
    }
    for operation in &selection.operations {
        prepared.operations.push(OperationFacts::decode(
            operation.id(),
            operation.traits(),
            &mut diagnostics,
        ));
    }

    // List and map members reference the member schemas of their targets, so those
    // references must not form a cycle.
    let dependencies = |member: usize| -> ([usize; 2], usize) {
        let target = &roots[members[member].target as usize];
        let start = target.member_start as usize;
        let shape_type = data.root_at(target.model_position as usize).shape_type();
        match shape_type {
            ShapeType::List => ([start, 0], 1),
            ShapeType::Map => ([start, start + 1], 2),
            _ => ([0, 0], 0),
        }
    };
    prepared.member_order = dependency_order(&member_views, dependencies, &mut diagnostics);
    if diagnostics.has_errors() {
        return fail(diagnostics);
    }

    let root_index = |id: &ShapeId| rank[selection.roots.get_index_of(id).expect("selected")];
    let mut operations = IndexMap::with_capacity(selection.operations.len());
    for (operation, errors) in selection.operations.iter().zip(&selection.errors) {
        operations.insert(
            operation.id().clone(),
            OperationPlan {
                model_position: data
                    .root_position(operation.id())
                    .expect("operations are in the model") as u32,
                input: root_index(operation.input()),
                output: root_index(operation.output()),
                errors: errors.iter().map(|id| root_index(id)).collect(),
                record: OnceLock::new(),
            },
        );
    }

    let plan = Plan {
        service: match &options.scope {
            Scope::AllShapes => None,
            Scope::Service { service, .. } => Some(service.clone()),
        },
        roots: roots.into(),
        root_index: by_position.into(),
        members: members.into(),
        operations,
    };
    Ok((plan, prepared))
}

/// Orders members so that each comes after the members it references, reporting every
/// cycle. Iterative, so deeply nested collections cannot overflow the stack.
fn dependency_order(
    members: &[crate::shape::MemberView<'_>],
    dependencies: impl Fn(usize) -> ([usize; 2], usize),
    diagnostics: &mut DiagnosticSet,
) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unseen,
        Building,
        Built,
    }
    let mut marks = vec![Mark::Unseen; members.len()];
    let mut order = Vec::with_capacity(members.len());
    // (member, next dependency to visit)
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for root in 0..members.len() {
        if marks[root] != Mark::Unseen {
            continue;
        }
        marks[root] = Mark::Building;
        stack.push((root, 0));
        while let Some((member, next)) = stack.last_mut() {
            let member = *member;
            let (targets, count) = dependencies(member);
            if *next == count {
                marks[member] = Mark::Built;
                order.push(member);
                stack.pop();
                continue;
            }
            let dependency = targets[*next];
            *next += 1;
            match marks[dependency] {
                Mark::Unseen => {
                    marks[dependency] = Mark::Building;
                    stack.push((dependency, 0));
                }
                Mark::Building => {
                    let start = stack
                        .iter()
                        .position(|&(entry, _)| entry == dependency)
                        .expect("a building member is on the stack");
                    let path: Vec<String> = stack[start..]
                        .iter()
                        .map(|&(entry, _)| format!("`{}`", members[entry].id()))
                        .chain(std::iter::once(format!("`{}`", members[dependency].id())))
                        .collect();
                    let view = members[member];
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidShape,
                            format!(
                                "list and map members form a cycle that does not pass through a \
                                 structure or union: {}",
                                path.join(" -> ")
                            ),
                        )
                        .with_source_location(view.source_location().clone())
                        .with_shape_id(view.id().clone()),
                    );
                }
                Mark::Built => {}
            }
        }
    }
    order
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Materialization of a schema graph with a [`Builder`].
//!
//! Everything here is safe code. [`build`] receives the model and a [`Builder`] branded with
//! the same lifetime, and returns a [`Graph`] of indexes that borrows nothing, so the type
//! checker proves that every schema borrows only the model, the arena, or `'static` data
//! (INV-4 in `owner`).

use super::facts::{OperationFacts, TraitFacts};
use super::owner::{Builder, SchemaIndex, SliceIndex};
use super::SchemaBuildError;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSet};
use crate::shape::{MemberView, OperationView, ServiceView, ShapeView};
use crate::{Model, ShapeId, ShapeType};
use aws_smithy_schema::{PayloadHint, Schema};
use indexmap::{IndexMap, IndexSet};
use std::collections::{HashMap, HashSet};

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
}

/// Indexes into the built storage. Borrows nothing.
#[derive(Debug)]
pub(super) struct Graph {
    /// Root and member schemas by shape ID.
    pub(super) schemas: HashMap<ShapeId, SchemaIndex>,
    /// The number of root (non-member) schemas in `schemas`.
    pub(super) root_count: usize,
    /// Operations in selection order.
    pub(super) operations: IndexMap<ShapeId, OperationRecord>,
    /// The selected service, if built for one.
    pub(super) service: Option<ShapeId>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OperationRecord {
    pub(super) input: SchemaIndex,
    pub(super) output: SchemaIndex,
    pub(super) errors: SliceIndex,
}

/// The runtime schema type of a model shape type, or `None` for shapes that have no schema.
///
/// The match is exhaustive, so a new shape type cannot be added to this crate without
/// deciding how it is materialized.
fn schema_type(shape_type: ShapeType) -> Option<SchemaShapeType> {
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
fn has_member_schemas(shape_type: ShapeType) -> bool {
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

fn member_id(id: &ShapeId) -> SchemaShapeId<'_> {
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

/// A member to materialize.
struct Member<'a> {
    view: MemberView<'a>,
    /// Position in its container's members.
    position: usize,
    /// Index of the target in `Selection::roots`.
    target: usize,
    facts: TraitFacts<'a>,
}

/// Builds the schemas selected by `options` with `arena`.
///
/// Validates every trait value and cycle before allocating. Internal invariant failures found
/// while allocating leave a partial graph, which `Arena::build` releases.
pub(super) fn build<'a>(
    model: &'a Model,
    arena: Builder<'a>,
    options: &Options,
) -> Result<Graph, SchemaBuildError> {
    let mut diagnostics = DiagnosticSet::with_limit(options.max_diagnostics);
    let fail = |diagnostics| Err(SchemaBuildError::new(diagnostics));

    let Some(selection) = Selection::select(model, &options.scope, &mut diagnostics) else {
        return fail(diagnostics);
    };
    let roots = &selection.roots;

    // Decode each schema-relevant trait application once.
    let mut root_facts = Vec::with_capacity(roots.len());
    let mut member_start = Vec::with_capacity(roots.len() + 1);
    let mut members = Vec::new();
    for shape in roots.values() {
        root_facts.push(TraitFacts::decode(
            shape.id(),
            shape.traits(),
            &mut diagnostics,
        ));
        member_start.push(members.len());
        if has_member_schemas(shape.shape_type()) {
            for (position, view) in shape.members().iter().enumerate() {
                members.push(Member {
                    view,
                    position,
                    target: roots
                        .get_index_of(view.target())
                        .expect("member targets are selected"),
                    facts: TraitFacts::decode(view.id(), view.traits(), &mut diagnostics),
                });
            }
        }
    }
    member_start.push(members.len());
    let operation_facts: Vec<_> = selection
        .operations
        .iter()
        .map(|operation| {
            OperationFacts::decode(operation.id(), operation.traits(), &mut diagnostics)
        })
        .collect();

    // List and map members reference the member schemas of their targets, so those must be
    // built first. Order members so dependencies come first, rejecting cycles.
    let dependencies = |member: &Member<'a>| -> ([usize; 2], usize) {
        let start = member_start[member.target];
        match roots[member.target].shape_type() {
            ShapeType::List => ([start, 0], 1),
            ShapeType::Map => ([start, start + 1], 2),
            _ => ([0, 0], 0),
        }
    };
    let order = dependency_order(&members, dependencies, &mut diagnostics);
    if diagnostics.has_errors() {
        return fail(diagnostics);
    }

    let struct_count = roots
        .values()
        .filter(|shape| matches!(shape.shape_type(), ShapeType::Structure | ShapeType::Union))
        .count();
    let operation_count = selection.operations.len();
    arena.reserve(
        roots.len() + members.len() + 2 * operation_count,
        struct_count + operation_count,
    );
    let mut schemas = HashMap::with_capacity(roots.len() + members.len());

    // Member schemas, dependencies first.
    let mut member_schemas: Vec<Option<&'a Schema<'a>>> = vec![None; members.len()];
    for &index in &order {
        let member = &members[index];
        let target = roots[member.target];
        let target_facts = &root_facts[member.target];
        let shape_type = schema_type(target.shape_type()).expect("members target data shapes");
        let mut schema = member.facts.apply(Schema::new_member(
            member_id(member.view.id()),
            shape_type,
            member.view.name(),
            member.position,
        ));
        // Target traits that a member inherits, as in `memberTraitChain` in
        // `SchemaGenerator.kt`.
        if target.shape_type() == ShapeType::Timestamp && member.facts.timestamp_format.is_none() {
            if let Some(format) = target_facts.timestamp_format {
                schema = schema.with_timestamp_format(format);
            }
        }
        if member.facts.media_type.is_none() {
            if let Some(media_type) = target_facts.media_type {
                schema = schema.with_media_type(media_type);
            }
        }
        if !member.facts.streaming && target_facts.streaming {
            schema = schema.with_streaming();
        }
        let (dependencies, count) = dependencies(member);
        let built =
            |i: usize| member_schemas[dependencies[i]].expect("dependencies are built first");
        match count {
            1 => schema = schema.with_list_member(built(0)),
            2 => schema = schema.with_map_members(built(0), built(1)),
            _ => {}
        }
        let (schema_index, schema) = arena.alloc(schema);
        member_schemas[index] = Some(schema);
        schemas.insert(member.view.id().clone(), schema_index);
    }
    let member_schemas: Vec<&'a Schema<'a>> = member_schemas
        .into_iter()
        .map(|schema| schema.expect("every member is built"))
        .collect();

    // Root schemas.
    let mut root_indexes = Vec::with_capacity(roots.len());
    let mut root_schemas = Vec::with_capacity(roots.len());
    let mut root_slices: Vec<Option<(SliceIndex, &'a [&'a Schema<'a>])>> = vec![None; roots.len()];
    for (position, shape) in roots.values().enumerate() {
        let id = root_id(shape.id());
        let shape_type = schema_type(shape.shape_type()).expect("roots are data shapes");
        let own = &member_schemas[member_start[position]..member_start[position + 1]];
        let schema = match shape.shape_type() {
            ShapeType::List => Schema::new_list(id, own[0]),
            ShapeType::Map => Schema::new_map(id, own[0], own[1]),
            ShapeType::Structure | ShapeType::Union => {
                // `new_struct` asserts this; check it so a bug is an error, not a panic.
                if let Some((i, _)) = own
                    .iter()
                    .enumerate()
                    .find(|(i, member)| member.member_index() != Some(*i))
                {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidShape,
                            format!(
                                "internal error: member {i} of `{}` has the wrong member index",
                                shape.id()
                            ),
                        )
                        .with_source_location(shape.source_location().clone())
                        .with_shape_id(shape.id().clone()),
                    );
                    return fail(diagnostics);
                }
                let slice = arena.alloc_slice(own.to_vec());
                root_slices[position] = Some(slice);
                root_facts[position].apply(Schema::new_struct(id, shape_type, slice.1))
            }
            // Generated schemas only carry root traits on structures and unions
            // (`traitSetterChain`); match them.
            _ => Schema::new(id, shape_type),
        };
        let (index, schema) = arena.alloc(schema);
        schemas.insert(shape.id().clone(), index);
        root_indexes.push(index);
        root_schemas.push(schema);
    }

    // Operation overlays.
    let mut operations = IndexMap::with_capacity(operation_count);
    for (position, operation) in selection.operations.iter().enumerate() {
        let facts = &operation_facts[position];
        let input = roots
            .get_index_of(operation.input())
            .expect("inputs are selected");

        // A structure root with the input's own ID, traits, and members, plus the
        // operation's HTTP binding and the hints derived from it. One per operation, so
        // operations that share an input (or `smithy.api#Unit`) do not share `@http`.
        let mut schema = overlay(operation.input(), input, &root_facts, &root_slices);
        if let Some(http) = facts.http_trait() {
            schema = schema.with_http(http);
        }
        let input_members = &members[member_start[input]..member_start[input + 1]];
        let mut has_body_members = false;
        let mut struct_payload = false;
        for member in input_members {
            // As in `noBodyMembersChain` and `payloadHintChain` in `SchemaGenerator.kt`.
            let framed_payload = member.facts.http_payload
                && matches!(
                    roots[member.target].shape_type(),
                    ShapeType::Structure | ShapeType::Union
                );
            struct_payload |= framed_payload;
            has_body_members |= framed_payload || !member.facts.has_http_binding();
        }
        schema = schema.with_payload_hint(if struct_payload {
            PayloadHint::StructPayload
        } else {
            PayloadHint::NoStructPayload
        });
        if !has_body_members {
            schema = schema.with_no_body_members();
        }
        let (input_index, _) = arena.alloc(schema);

        let output = roots
            .get_index_of(operation.output())
            .expect("outputs are selected");
        let output_index = if facts.xml_unwrapped_output {
            let schema = overlay(operation.output(), output, &root_facts, &root_slices);
            arena.alloc(schema.with_xml_unwrapped_output()).0
        } else {
            root_indexes[output]
        };

        let errors = selection.errors[position]
            .iter()
            .map(|id| root_schemas[roots.get_index_of(id).expect("errors are selected")])
            .collect();
        let (errors, _) = arena.alloc_slice(errors);

        operations.insert(
            operation.id().clone(),
            OperationRecord {
                input: input_index,
                output: output_index,
                errors,
            },
        );
    }

    Ok(Graph {
        schemas,
        root_count: roots.len(),
        operations,
        service: match &options.scope {
            Scope::AllShapes => None,
            Scope::Service { service, .. } => Some(service.clone()),
        },
    })
}

/// A new structure root for `id`, the selected root at `position`, that shares its members
/// and repeats its traits. `Schema` is not `Clone`, so an overlay is rebuilt from the same
/// decoded facts as the base schema.
fn overlay<'a>(
    id: &'a ShapeId,
    position: usize,
    root_facts: &[TraitFacts<'a>],
    root_slices: &[Option<(SliceIndex, &'a [&'a Schema<'a>])>],
) -> Schema<'a> {
    let (_, members) = root_slices[position].expect("operation inputs and outputs are structures");
    root_facts[position].apply(Schema::new_struct(
        root_id(id),
        SchemaShapeType::Structure,
        members,
    ))
}

/// Orders members so that each comes after the members it references, reporting every
/// cycle. Iterative, so deeply nested collections cannot overflow the stack.
fn dependency_order<'a>(
    members: &[Member<'a>],
    dependencies: impl Fn(&Member<'a>) -> ([usize; 2], usize),
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
            let (targets, count) = dependencies(&members[member]);
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
                        .map(|&(entry, _)| format!("`{}`", members[entry].view.id()))
                        .chain(std::iter::once(format!(
                            "`{}`",
                            members[dependency].view.id()
                        )))
                        .collect();
                    let view = members[member].view;
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

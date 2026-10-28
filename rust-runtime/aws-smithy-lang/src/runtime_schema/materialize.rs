/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Building the schemas of a [`Plan`], in batches.
//!
//! A [`Batch`] runs inside one [`Storage::with_builder`](super::owner::Storage::with_builder)
//! call, so batches are serialized (INV-8). It builds missing schemas on demand: a member
//! after the list and map members it references, a root after its members, and an operation
//! after the closure of data shapes it reaches. Every completed schema is published in its
//! plan slot at once, so a batch that panics leaves complete slots that a later batch reuses
//! (INV-6). Operation records are published last, after their schemas and error slice.
//!
//! Everything here is safe code except reading published handles back, which relies on the
//! plan only holding handles from this storage (INV-9).

use super::facts::{OperationFacts, TraitFacts};
use super::owner::{Builder, SchemaHandle, SchemaSlot};
use super::plan::{
    has_member_schemas, member_id, root_id, schema_type, OperationRecord, Plan, Prepared,
};
use crate::diagnostic::DiagnosticSet;
use crate::shape::{MemberView, ShapeView};
use crate::{Model, ShapeType};
use aws_smithy_schema::{PayloadHint, Schema};

type SchemaShapeType = aws_smithy_schema::ShapeType;

/// Where a batch gets trait facts.
pub(super) enum Facts<'m> {
    /// Decoded while planning, in the same batch.
    Prepared(Prepared<'m>),
    /// Decoded again for each shape built. The model is immutable and the plan validated it,
    /// so this cannot find new errors.
    Decode,
}

/// One serialized materialization batch.
pub(super) struct Batch<'p, 'b> {
    plan: &'p Plan,
    model: &'b Model,
    builder: Builder<'b>,
    facts: Facts<'b>,
    /// Diagnostics from decoding facts again; always empty.
    diagnostics: DiagnosticSet,
    /// Facts decoded again, with [`Facts::Decode`].
    scratch: TraitFacts<'b>,
    /// Roots whose closure this batch has built or is building, indexed like `Plan::roots`.
    /// Allocated on first use; a new batch starts with a fresh one, so a batch that panicked
    /// leaves no stale state.
    visited: Vec<bool>,
    /// Traversal stack reused by `member`; cleared before each use.
    stack: Vec<usize>,
}

impl<'p, 'b> Batch<'p, 'b> {
    /// A batch that builds `plan`'s schemas from `model` with `builder`.
    ///
    /// # Safety
    ///
    /// INV-9: every handle already published in `plan` must have been returned by a builder
    /// of the same storage as `builder`. Nothing ties `plan` to that storage, so a plan from
    /// another set would make the batch dereference that set's pointers.
    ///
    /// `model` needs no such condition: `builder`'s brand only accepts data that outlives
    /// the batch, which inside [`Storage::with_builder`](super::owner::Storage::with_builder)
    /// is the storage's own data or `'static` data, both sound.
    pub(super) unsafe fn new(
        plan: &'p Plan,
        model: &'b Model,
        builder: Builder<'b>,
        facts: Facts<'b>,
    ) -> Self {
        Self {
            plan,
            model,
            builder,
            facts,
            diagnostics: DiagnosticSet::with_limit(1),
            scratch: TraitFacts::default(),
            visited: Vec::new(),
            stack: Vec::new(),
        }
    }

    /// The schema of a published handle.
    fn published(&self, handle: SchemaHandle) -> &'b Schema<'b> {
        // SAFETY: INV-9: `handle` was read from a slot of `self.plan`, which holds only
        // handles from `self.builder`'s storage (the contract of `Batch::new`, and this batch
        // only publishes its own handles).
        unsafe { self.builder.published_schema(handle) }
    }

    fn publish(slot: &SchemaSlot, handle: SchemaHandle) {
        // Batches are serialized and every slot is checked before it is built, so the slot
        // is empty (INV-8).
        slot.set(handle);
    }

    fn root_shape(&self, root: usize) -> ShapeView<'b> {
        let data = self.model.data();
        ShapeView::new(
            data,
            data.root_at(self.plan.roots[root].model_position as usize),
        )
    }

    fn member_view(&self, member: usize) -> MemberView<'b> {
        let plan = &self.plan.members[member];
        let container = &self.plan.roots[plan.container as usize];
        self.root_shape(plan.container as usize)
            .members()
            .get_index(member - container.member_start as usize)
            .expect("planned members exist")
    }

    fn root_facts(&mut self, root: usize) -> &TraitFacts<'b> {
        match &self.facts {
            Facts::Prepared(prepared) => &prepared.roots[root],
            Facts::Decode => {
                let shape = self.root_shape(root);
                self.scratch =
                    TraitFacts::decode(shape.id(), shape.traits(), &mut self.diagnostics);
                &self.scratch
            }
        }
    }

    fn member_facts(&mut self, member: usize) -> &TraitFacts<'b> {
        match &self.facts {
            Facts::Prepared(prepared) => &prepared.members[member],
            Facts::Decode => {
                let view = self.member_view(member);
                self.scratch = TraitFacts::decode(view.id(), view.traits(), &mut self.diagnostics);
                &self.scratch
            }
        }
    }

    fn operation_facts(&mut self, operation: usize) -> OperationFacts<'b> {
        let plan = self.plan;
        let (_, operation_plan) = plan.operations.get_index(operation).expect("in range");
        let data = self.model.data();
        let shape = ShapeView::new(data, data.root_at(operation_plan.model_position as usize));
        match &self.facts {
            Facts::Prepared(prepared) => prepared.operations[operation],
            Facts::Decode => {
                OperationFacts::decode(shape.id(), shape.traits(), &mut self.diagnostics)
            }
        }
    }

    /// The members that `member`'s schema references: its target's list member, or map key
    /// and value.
    fn dependencies(&self, member: usize) -> ([usize; 2], usize) {
        let target = self.plan.members[member].target as usize;
        let start = self.plan.roots[target].member_start as usize;
        match self.root_shape(target).shape_type() {
            ShapeType::List => ([start, 0], 1),
            ShapeType::Map => ([start, start + 1], 2),
            _ => ([0, 0], 0),
        }
    }

    /// The schema of a member, building it and the members it references if needed.
    fn member(&mut self, member: usize) -> &'b Schema<'b> {
        let plan = self.plan;
        if let Some(handle) = plan.members[member].schema.get() {
            return self.published(handle);
        }
        // Dependencies first. The plan rejected cycles, so this terminates.
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push(member);
        while let Some(&top) = stack.last() {
            if plan.members[top].schema.get().is_some() {
                stack.pop();
                continue;
            }
            let (dependencies, count) = self.dependencies(top);
            if let Some(&missing) = dependencies[..count]
                .iter()
                .find(|&&dependency| plan.members[dependency].schema.get().is_none())
            {
                debug_assert!(stack.len() <= plan.members.len(), "a member cycle");
                stack.push(missing);
                continue;
            }
            self.build_member(top);
            stack.pop();
        }
        self.stack = stack;
        self.published(plan.members[member].schema.get().expect("built"))
    }

    /// Builds a member whose dependencies are built.
    fn build_member(&mut self, member: usize) {
        let plan = self.plan;
        let member_plan = &plan.members[member];
        let view = self.member_view(member);
        let target_index = member_plan.target as usize;
        let target = self.root_shape(target_index);
        let shape_type = schema_type(target.shape_type()).expect("members target data shapes");
        let position = member - plan.roots[member_plan.container as usize].member_start as usize;
        // Target traits that a member inherits, as in `memberTraitChain` in
        // `SchemaGenerator.kt`.
        let target_facts = self.root_facts(target_index);
        let (format, media_type, streaming) = (
            target_facts.timestamp_format,
            target_facts.media_type,
            target_facts.streaming,
        );
        let facts = self.member_facts(member);
        let inherit_format =
            target.shape_type() == ShapeType::Timestamp && facts.timestamp_format.is_none();
        let inherit_media_type = facts.media_type.is_none();
        let inherit_streaming = !facts.streaming && streaming;
        let mut schema = facts.apply(Schema::new_member(
            member_id(view.id()),
            shape_type,
            view.name(),
            position,
        ));
        if let (true, Some(format)) = (inherit_format, format) {
            schema = schema.with_timestamp_format(format);
        }
        if let (true, Some(media_type)) = (inherit_media_type, media_type) {
            schema = schema.with_media_type(media_type);
        }
        if inherit_streaming {
            schema = schema.with_streaming();
        }
        let (dependencies, count) = self.dependencies(member);
        let built = |batch: &Self, i: usize| {
            batch.published(
                plan.members[dependencies[i]]
                    .schema
                    .get()
                    .expect("built first"),
            )
        };
        match count {
            1 => schema = schema.with_list_member(built(self, 0)),
            2 => schema = schema.with_map_members(built(self, 0), built(self, 1)),
            _ => {}
        }
        let (handle, _) = self.builder.alloc(schema);
        Self::publish(&member_plan.schema, handle);
    }

    /// The schema of a root, building it and its members if needed. Does not build the roots
    /// its members target; see [`Batch::closure`].
    fn root(&mut self, root: usize) -> &'b Schema<'b> {
        let plan = self.plan;
        let root_plan = &plan.roots[root];
        if let Some(handle) = root_plan.schema.get() {
            return self.published(handle);
        }
        let shape = self.root_shape(root);
        let id = root_id(shape.id());
        let shape_type = schema_type(shape.shape_type()).expect("roots are data shapes");
        let start = root_plan.member_start as usize;
        let members = start..start + root_plan.member_len as usize;
        let schema = match shape.shape_type() {
            ShapeType::List => Schema::new_list(id, self.member(start)),
            ShapeType::Map => Schema::new_map(id, self.member(start), self.member(start + 1)),
            ShapeType::Structure | ShapeType::Union => {
                let members: Vec<_> = members.map(|member| self.member(member)).collect();
                let (_, members) = self.builder.alloc_slice(members);
                self.root_facts(root)
                    .apply(Schema::new_struct(id, shape_type, members))
            }
            // Generated schemas only carry root traits on structures and unions
            // (`traitSetterChain`); match them.
            _ => Schema::new(id, shape_type),
        };
        let (handle, schema) = self.builder.alloc(schema);
        Self::publish(&root_plan.schema, handle);
        schema
    }

    /// Builds `seeds` and every root their members reach, transitively.
    pub(super) fn closure(&mut self, seeds: impl IntoIterator<Item = usize>) {
        let plan = self.plan;
        if self.visited.is_empty() {
            self.visited = vec![false; plan.roots.len()];
        }
        let mut stack = Vec::new();
        for seed in seeds {
            if !std::mem::replace(&mut self.visited[seed], true) {
                stack.push(seed);
            }
        }
        while let Some(root) = stack.pop() {
            self.root(root);
            let root_plan = &plan.roots[root];
            let start = root_plan.member_start as usize;
            for member in &plan.members[start..start + root_plan.member_len as usize] {
                let target = member.target as usize;
                if !std::mem::replace(&mut self.visited[target], true) {
                    stack.push(target);
                }
            }
        }
    }

    /// A new structure root with the ID, members, and traits of `root`. `Schema` is not
    /// `Clone`, so an overlay is rebuilt from the same facts as the base schema.
    fn overlay(&mut self, root: usize) -> Schema<'b> {
        let base = self.root(root);
        let shape = self.root_shape(root);
        debug_assert!(
            matches!(shape.shape_type(), ShapeType::Structure),
            "operation inputs and outputs are structures"
        );
        self.root_facts(root).apply(Schema::new_struct(
            root_id(shape.id()),
            SchemaShapeType::Structure,
            base.members(),
        ))
    }

    /// The record of an operation, building its closure, overlays, and error slice if needed.
    pub(super) fn operation(&mut self, operation: usize) -> OperationRecord {
        let plan = self.plan;
        let (_, operation_plan) = plan.operations.get_index(operation).expect("in range");
        if let Some(&record) = operation_plan.record.get() {
            return record;
        }
        let input = operation_plan.input as usize;
        let output = operation_plan.output as usize;
        self.closure(
            [input, output]
                .into_iter()
                .chain(operation_plan.errors.iter().map(|&error| error as usize)),
        );
        let facts = self.operation_facts(operation);

        // A structure root with the input's own ID, traits, and members, plus the
        // operation's HTTP binding and the hints derived from it. One per operation, so
        // operations that share an input (or `smithy.api#Unit`) do not share `@http`.
        let mut schema = self.overlay(input);
        if let Some(http) = facts.http_trait() {
            schema = schema.with_http(http);
        }
        let input_plan = &plan.roots[input];
        let start = input_plan.member_start as usize;
        let mut has_body_members = false;
        let mut struct_payload = false;
        for member in start..start + input_plan.member_len as usize {
            let target = self.root_shape(plan.members[member].target as usize);
            let member_facts = self.member_facts(member);
            // As in `noBodyMembersChain` and `payloadHintChain` in `SchemaGenerator.kt`.
            let framed_payload = member_facts.http_payload
                && matches!(target.shape_type(), ShapeType::Structure | ShapeType::Union);
            struct_payload |= framed_payload;
            has_body_members |= framed_payload || !member_facts.has_http_binding();
        }
        schema = schema.with_payload_hint(if struct_payload {
            PayloadHint::StructPayload
        } else {
            PayloadHint::NoStructPayload
        });
        if !has_body_members {
            schema = schema.with_no_body_members();
        }
        let (input, _) = self.builder.alloc(schema);

        let output = if facts.xml_unwrapped_output {
            let schema = self.overlay(output).with_xml_unwrapped_output();
            self.builder.alloc(schema).0
        } else {
            self.root(output);
            plan.roots[output].schema.get().expect("built")
        };

        let errors = operation_plan
            .errors
            .iter()
            .map(|&error| self.root(error as usize))
            .collect();
        let (errors, _) = self.builder.alloc_slice(errors);

        let record = OperationRecord {
            input,
            output,
            errors,
        };
        // Published only now that every schema it refers to is complete (INV-6).
        let published = operation_plan.record.set(record);
        debug_assert!(published.is_ok(), "an operation was built twice");
        record
    }

    /// Builds every root, member, and operation of the plan, with facts prepared by the
    /// planner.
    pub(super) fn all(&mut self) {
        let plan = self.plan;
        let data = self.model.data();
        let structures = plan
            .roots
            .iter()
            .filter(|root| {
                let shape_type = data.root_at(root.model_position as usize).shape_type();
                has_member_schemas(shape_type)
                    && matches!(shape_type, ShapeType::Structure | ShapeType::Union)
            })
            .count();
        self.builder.reserve(
            plan.roots.len() + plan.members.len() + 2 * plan.operations.len(),
            structures + plan.operations.len(),
        );
        // Members in the planner's dependency order, so none has to search for its
        // dependencies.
        let order = match &mut self.facts {
            Facts::Prepared(prepared) => std::mem::take(&mut prepared.member_order),
            Facts::Decode => Vec::new(),
        };
        for member in order {
            if plan.members[member].schema.get().is_none() {
                self.build_member(member);
            }
        }
        for root in 0..plan.roots.len() {
            self.root(root);
        }
        // Every root is built, so operations need not walk their closures.
        self.visited = vec![true; plan.roots.len()];
        for operation in 0..plan.operations.len() {
            self.operation(operation);
        }
    }
}

impl Drop for Batch<'_, '_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            debug_assert!(
                !self.diagnostics.has_errors(),
                "a validated plan decoded with errors: {:?}",
                self.diagnostics
            );
        }
    }
}

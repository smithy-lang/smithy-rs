/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime [`aws_smithy_schema::Schema`]s built from a [`Model`].
//!
//! Requires the `runtime-schema` feature. It is optional because it adds a dependency on
//! `aws-smithy-schema` and, through it, the Smithy client runtime, which model tooling that
//! only loads, validates, or inspects models does not need.
//!
//! A [`RuntimeSchemaSet`] converts the serializable shapes of a model, once each, into the
//! same schema descriptors that generated code declares as statics. Protocols and codecs that
//! accept `&Schema<'_>` can then serialize and deserialize against a model loaded at
//! runtime, with no model lookups on the request path once a schema is built.
//!
//! ```
//! use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! let model = Model::from_json_str(
//!     "model.json",
//!     r#"{
//!         "smithy": "2.0",
//!         "shapes": {
//!             "example#GetThing": {
//!                 "type": "operation",
//!                 "input": { "target": "example#GetThingInput" },
//!                 "traits": { "smithy.api#http": { "method": "GET", "uri": "/things/{id}" } }
//!             },
//!             "example#GetThingInput": {
//!                 "type": "structure",
//!                 "members": {
//!                     "id": {
//!                         "target": "smithy.api#String",
//!                         "traits": { "smithy.api#httpLabel": {}, "smithy.api#required": {} }
//!                     }
//!                 }
//!             }
//!         }
//!     }"#,
//! )?;
//!
//! let schemas = RuntimeSchemaSet::try_from(&model)?;
//!
//! // The operation's input, with its `@http` binding, ready for a protocol.
//! let operation = schemas.operation(&ShapeId::new("example#GetThing")?).unwrap();
//! let input = operation.input();
//! assert_eq!(input.shape_id().as_str(), "example#GetThingInput");
//! assert_eq!(input.http().unwrap().uri(), "/things/{id}");
//! assert!(input.member_schema("id").unwrap().http_label().is_some());
//!
//! // Any data shape or member by ID.
//! let member = schemas.schema(&ShapeId::new("example#GetThingInput$id")?).unwrap();
//! assert_eq!(member.member_name(), Some("id"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What is built
//!
//! - One schema per selected root data shape, and one per member of every structure, union,
//!   list, and map. Enums are strings and intEnums are integers. Services, resources, and
//!   operations have no schema.
//! - Member schemas carry their own traits plus the `@timestampFormat`, `@mediaType`, and
//!   `@streaming` of their target, and members that target a list or map reference the
//!   target's member schemas. Each modeled member has exactly one schema.
//! - Structures and unions carry their schema-relevant traits; as in generated code, other
//!   roots carry none.
//! - Each operation has its own input root: the input structure's ID, members, and traits plus
//!   the operation's `@http` trait and the body hints derived from its HTTP bindings. The
//!   schema of the same structure returned by [`RuntimeSchemaSet::schema`] does not carry
//!   `@http`, because several operations can share one input.
//!
//! The traits carried are the known trait fields of `Schema`. Other traits, including custom
//! traits, are not yet retained.
//!
//! # When schemas are built
//!
//! Building a set always validates every selected shape, so lookups never fail. By default
//! ([`Materialization::Eager`]) it also builds every selected schema, and lookups never build
//! anything or take a lock. With [`Materialization::Lazy`], building allocates no schemas:
//!
//! - the first [`operation`](RuntimeSchemaSet::operation) lookup of an operation builds its
//!   input, output, and errors, and every shape they reach;
//! - the first [`schema`](RuntimeSchemaSet::schema) lookup of a data shape or member builds
//!   its root shape and every shape that reaches; and
//! - [`operations`](RuntimeSchemaSet::operations) builds each operation as it is reached,
//!   [`operation_ids`](RuntimeSchemaSet::operation_ids) builds nothing, and
//!   [`materialize_all`](RuntimeSchemaSet::materialize_all) builds whatever is left.
//!
//! Schemas already built are shared and never rebuilt, so a lazy set that has built
//! everything holds the same schemas as an eager one. A lookup that builds takes a lock that
//! serializes building and traverses the model; a lookup of something already built does
//! neither. Either way, the set keeps the complete model.
//!
//! # Lifetimes
//!
//! Schemas borrow their strings from the set's own clone of the model, so a set is cheap to
//! build and owns everything it returns. References cannot outlive the set:
//!
//! ```compile_fail,E0597
//! # use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
//! # use aws_smithy_lang::{Model, ShapeId};
//! # let model = Model::from_json_str("m", r#"{"smithy":"2.0"}"#).unwrap();
//! let id = ShapeId::new("smithy.api#String").unwrap();
//! let schema = {
//!     let schemas = RuntimeSchemaSet::try_from(&model).unwrap();
//!     schemas.schema(&id)
//! };
//! # let _ = schema;
//! ```
//!
//! Nor can operation descriptors:
//!
//! ```compile_fail,E0597
//! # use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
//! # use aws_smithy_lang::{Model, ShapeId};
//! # let model = Model::from_json_str("m", r#"{"smithy":"2.0"}"#).unwrap();
//! let id = ShapeId::new("example#GetThing").unwrap();
//! let operation = {
//!     let schemas = RuntimeSchemaSet::try_from(&model).unwrap();
//!     schemas.operation(&id)
//! };
//! # let _ = operation;
//! ```
//!
//! Nor can strings borrowed from a schema:
//!
//! ```compile_fail,E0597
//! # use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
//! # use aws_smithy_lang::{Model, ShapeId};
//! # let model = Model::from_json_str("m", r#"{"smithy":"2.0"}"#).unwrap();
//! let id = ShapeId::new("smithy.api#String").unwrap();
//! let name: &'static str = {
//!     let schemas = RuntimeSchemaSet::try_from(&model).unwrap();
//!     schemas.schema(&id).unwrap().shape_id().as_str()
//! };
//! ```
//!
//! # Header names
//!
//! `@httpHeader` names are interned with [`aws_smithy_schema::intern_header_name`], which never
//! frees them. That is a few bytes per distinct name, but it grows without bound in a process
//! that keeps loading models with new header names.

mod error;
mod facts;
mod materialize;
mod owner;
mod plan;

#[cfg(test)]
mod tests;

pub use aws_smithy_schema::OperationSchema;
pub use error::SchemaBuildError;

use crate::json_loader::DEFAULT_MAX_DIAGNOSTICS;
use crate::{Model, ShapeId};
use aws_smithy_schema::Schema;
use materialize::{Batch, Facts};
use owner::{SchemaHandle, Storage};
use plan::{OperationFilter, OperationRecord, Options, Plan, Scope, Slot};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

/// An immutable set of runtime schemas built from a [`Model`].
///
/// See the [module documentation](self).
pub struct RuntimeSchemaSet {
    /// What the set contains, and the handles of what has been built. Holds only handles
    /// from `storage` (INV-9): it is created with `storage`, never replaced, and only
    /// filled by batches of `storage`.
    plan: Plan,
    /// The model and the schemas that borrow it, which safe Rust cannot express for two
    /// fields of one struct; `owner` explains why that requires `unsafe` and why the
    /// alternatives were rejected.
    // TODO(dynamic-client): This keeps the complete model alive (about 29 MiB for EC2), even
    // when the set is scoped to a service or filtered to a few operations, whose schemas are
    // a small fraction of that. Investigate owning a model trimmed to the selected closure
    // instead, for example a `Model::project(service, operations)` that copies only the
    // selected shapes and the service. That needs a policy for source locations, traits,
    // and references to unselected shapes, and `model()` would then return the trimmed
    // model.
    storage: Storage<Model>,
    materialization: Materialization,
    /// Whether every slot and record of `plan` is filled, so `materialize_all` can return
    /// without a batch. Stored with release ordering after the batch that filled them, so a
    /// thread whose `materialize_all` returns early still observes every slot filled.
    complete: AtomicBool,
}

/// When a [`RuntimeSchemaSet`] builds its schemas.
///
/// The selected shapes are validated when the set is built either way, so a lazy set never
/// fails later: its lookups return the same schemas an eager set would, and only differ in
/// when they are allocated. Set with [`RuntimeSchemaSetBuilder::materialization`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Materialization {
    /// Every selected schema and operation is built before
    /// [`build`](RuntimeSchemaSetBuilder::build) returns, and lookups never build anything
    /// or take a lock. This is the default.
    ///
    /// Building eagerly reuses the trait values decoded while validating, so it is cheaper
    /// than building lazily and then calling [`RuntimeSchemaSet::materialize_all`].
    #[default]
    Eager,
    /// Nothing is built by [`build`](RuntimeSchemaSetBuilder::build). The first lookup of an
    /// operation builds the schemas it reaches, and the first lookup of a data shape builds
    /// the schemas that shape reaches; schemas already built are shared, never rebuilt.
    ///
    /// A first lookup takes a lock that serializes building, and decodes the traits of the
    /// shapes it builds; later lookups of the same operation or shape take no lock and build
    /// nothing. Use this when a client may use only a few of a service's operations, or to
    /// move the work out of startup.
    Lazy,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeSchemaSet>();
};

impl RuntimeSchemaSet {
    /// A builder for choosing which shapes to build.
    pub fn builder(model: &Model) -> RuntimeSchemaSetBuilder<'_> {
        RuntimeSchemaSetBuilder {
            model,
            options: Options {
                scope: Scope::AllShapes,
                max_diagnostics: DEFAULT_MAX_DIAGNOSTICS,
                max_plan_entries: u32::MAX,
                materialization: Materialization::Eager,
            },
        }
    }

    fn build(model: Model, options: &Options) -> Result<Self, SchemaBuildError> {
        // If planning fails or materializing panics, dropping `storage` releases everything
        // allocated (INV-3).
        let storage = Storage::new(model);
        let plan = storage.with_builder(|model, builder| {
            let (plan, prepared) = plan::plan(model, options)?;
            if options.materialization == Materialization::Lazy {
                return Ok(plan);
            }
            {
                // SAFETY: INV-9: `plan` is new and holds no handles.
                let mut batch =
                    unsafe { Batch::new(&plan, model, builder, Facts::Prepared(prepared)) };
                batch.all();
            }
            Ok::<_, SchemaBuildError>(plan)
        })?;
        let eager = options.materialization == Materialization::Eager;
        Ok(Self {
            plan,
            storage,
            materialization: options.materialization,
            complete: AtomicBool::new(eager),
        })
    }

    /// Runs a materialization batch.
    fn batch<R>(&self, run: impl for<'b> FnOnce(&mut Batch<'_, 'b>) -> R) -> R {
        self.storage.with_builder(|model, builder| {
            // SAFETY: INV-9: `self.plan` holds only handles from `self.storage` (see the
            // field).
            let mut batch = unsafe { Batch::new(&self.plan, model, builder, Facts::Decode) };
            run(&mut batch)
        })
    }

    /// The handle in `slot`, materializing the containing root's closure if needed.
    #[cold]
    fn materialize(&self, slot: Slot) -> SchemaHandle {
        let root = match slot {
            Slot::Root(root) => root,
            Slot::Member(member) => self.plan.members[member].container as usize,
        };
        self.batch(|batch| batch.closure([root]));
        self.plan.get(slot).expect("materialized")
    }

    /// The model the schemas were built from.
    pub fn model(&self) -> &Model {
        self.storage.data()
    }

    /// The service the set was built for, if it was built with
    /// [`RuntimeSchemaSetBuilder::for_service`] or
    /// [`RuntimeSchemaSetBuilder::for_service_operations`].
    pub fn service(&self) -> Option<&ShapeId> {
        self.plan.service.as_ref()
    }

    /// The schema of a data shape or member, or `None` if it was not selected.
    ///
    /// For an operation input, this is the shape's own schema, without the operation's
    /// `@http` trait; use [`RuntimeSchemaSet::operation`] to serialize a request.
    pub fn schema(&self, id: &ShapeId) -> Option<&Schema<'_>> {
        let slot = self.plan.slot(self.model().data(), id)?;
        let handle = match self.plan.get(slot) {
            Some(handle) => handle,
            None => self.materialize(slot),
        };
        // SAFETY: INV-9: `handle` comes from `self.plan`, which holds only handles from
        // `self.storage`.
        Some(unsafe { self.storage.schema(handle) })
    }

    /// The schemas of an operation, or `None` if it was not selected.
    ///
    /// The descriptor's [`input`](OperationSchema::input) is specific to this operation: it
    /// has the input structure's ID (`smithy.api#Unit` when the input is omitted), traits, and
    /// members plus the operation's `@http` trait, payload hint, and whether any member is
    /// bound to the body. Each operation has its own input, even when operations share an
    /// input structure. Its [`output`](OperationSchema::output) is the output structure's own
    /// schema, unless the operation has the `aws.customizations#s3UnwrappedXmlOutput` trait,
    /// which gives it its own output that records it.
    ///
    /// Its [`errors`](OperationSchema::errors), for a set built
    /// [`for_service`](RuntimeSchemaSetBuilder::for_service), are the service's common errors
    /// then the operation's own, without duplicates, as
    /// [`OperationKnowledge::errors_in`](crate::knowledge::OperationKnowledge::errors_in)
    /// returns them. Otherwise, they are only the operation's own errors.
    pub fn operation(&self, id: &ShapeId) -> Option<OperationSchema<'_>> {
        let (index, id, _) = self.plan.operations.get_full(id)?;
        Some(self.operation_schema(index, id))
    }

    /// Every selected operation: in model order, or sorted by shape ID for a service.
    pub fn operations(&self) -> impl ExactSizeIterator<Item = OperationSchema<'_>> + '_ {
        self.plan
            .operations
            .keys()
            .enumerate()
            .map(|(index, id)| self.operation_schema(index, id))
    }

    fn operation_schema<'a>(&'a self, index: usize, id: &'a ShapeId) -> OperationSchema<'a> {
        let (_, plan) = self.plan.operations.get_index(index).expect("in range");
        let record = match plan.record.get() {
            Some(record) => *record,
            None => self.materialize_operation(index),
        };
        // SAFETY: INV-9, as in `schema`: the record was published in `self.plan`.
        let (input, output, errors) = unsafe {
            (
                self.storage.schema(record.input),
                self.storage.schema(record.output),
                self.storage.slice(record.errors),
            )
        };
        OperationSchema::builder(plan::root_id(id))
            .input(input)
            .output(output)
            .errors(errors)
            .build()
    }

    #[cold]
    fn materialize_operation(&self, index: usize) -> OperationRecord {
        self.batch(|batch| batch.operation(index))
    }

    /// The IDs of the selected operations, in the order of
    /// [`operations`](Self::operations), without building any schema.
    pub fn operation_ids(&self) -> impl ExactSizeIterator<Item = &ShapeId> + '_ {
        self.plan.operations.keys()
    }

    /// Builds every selected schema and operation not built yet.
    ///
    /// For a [lazy](Materialization::Lazy) set, this moves the remaining work to a time and
    /// thread the caller chooses, for example after startup. It cannot fail, because the set
    /// was validated when it was built. It returns at once, without taking a lock, for an
    /// [eager](Materialization::Eager) set or once a call has completed.
    ///
    /// To build everything at startup, prefer building with [`Materialization::Eager`],
    /// which is cheaper.
    pub fn materialize_all(&self) {
        if self.complete.load(Ordering::Acquire) {
            return;
        }
        self.batch(|batch| batch.all());
        self.complete.store(true, Ordering::Release);
    }

    /// When the set builds its schemas.
    pub fn materialization(&self) -> Materialization {
        self.materialization
    }

    /// The number of root data shape schemas, excluding members and operation inputs.
    pub fn len(&self) -> usize {
        self.plan.roots.len()
    }

    /// Returns `true` if no data shapes were selected.
    pub fn is_empty(&self) -> bool {
        self.plan.roots.is_empty()
    }

    /// The schema in `slot` if it has been materialized, without materializing it.
    #[cfg(test)]
    fn materialized(&self, slot: Slot) -> Option<&Schema<'_>> {
        // SAFETY: INV-9, as in `schema`.
        self.plan
            .get(slot)
            .map(|handle| unsafe { self.storage.schema(handle) })
    }
}

impl TryFrom<&Model> for RuntimeSchemaSet {
    type Error = SchemaBuildError;

    /// Builds every data shape and operation outside the prelude, and the prelude shapes they
    /// use.
    fn try_from(model: &Model) -> Result<Self, Self::Error> {
        Self::builder(model).build()
    }
}

impl TryFrom<Model> for RuntimeSchemaSet {
    type Error = SchemaBuildError;

    /// Builds every data shape and operation outside the prelude, and the prelude shapes they
    /// use.
    fn try_from(model: Model) -> Result<Self, Self::Error> {
        let options = Self::builder(&model).options;
        Self::build(model, &options)
    }
}

impl fmt::Debug for RuntimeSchemaSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts what is built without building or waiting for anything.
        let built_operations = self
            .plan
            .operations
            .values()
            .filter(|operation| operation.record.get().is_some())
            .count();
        f.debug_struct("RuntimeSchemaSet")
            .field("service", &self.plan.service)
            .field("materialization", &self.materialization)
            .field("roots", &self.plan.roots.len())
            .field("operations", &self.plan.operations.len())
            .field("built_schemas", &self.storage.schema_count())
            .field("built_operations", &built_operations)
            .finish_non_exhaustive()
    }
}

/// Chooses which shapes a [`RuntimeSchemaSet`] contains.
#[derive(Debug)]
pub struct RuntimeSchemaSetBuilder<'m> {
    model: &'m Model,
    options: Options,
}

impl RuntimeSchemaSetBuilder<'_> {
    /// Builds every data shape and operation outside the prelude, and the prelude shapes they
    /// use. This is the default.
    pub fn all_shapes(mut self) -> Self {
        self.options.scope = Scope::AllShapes;
        self
    }

    /// Builds only what one service needs: its operations, including those bound through
    /// resources, their inputs, outputs, and errors (with the service's common errors), and
    /// every shape those reach.
    pub fn for_service(mut self, service: &ShapeId) -> Self {
        self.options.scope = Scope::Service {
            service: service.clone(),
            operations: OperationFilter::All,
        };
        self
    }

    /// Builds only the subset of schemas needed by the indicated operations. Those operations, their inputs,
    /// outputs, and errors (with the service's common errors), and every shape those reach.
    ///
    /// Each operation must be contained in the service, directly or through its resources;
    /// [`build`](Self::build) reports any that do not exist, are not operations, or are not
    /// in the service. Duplicates are ignored, and the operations keep the order of
    /// [`for_service`](Self::for_service), not the order given. Only the selected closure is
    /// validated, so a malformed trait reachable only from other operations is not an error.
    ///
    /// ```
    /// # use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
    /// # use aws_smithy_lang::{Model, ShapeId};
    /// # let model = Model::from_json_str("m.json", r#"{"smithy": "2.0", "shapes": {
    /// #     "example#Service": {"type": "service", "operations": [
    /// #         {"target": "example#GetThing"}, {"target": "example#PutThing"}]},
    /// #     "example#GetThing": {"type": "operation", "input": {"target": "example#GetInput"}},
    /// #     "example#PutThing": {"type": "operation", "input": {"target": "example#PutInput"}},
    /// #     "example#GetInput": {"type": "structure", "members": {}},
    /// #     "example#PutInput": {"type": "structure", "members": {}}
    /// # }}"#)?;
    /// let service = ShapeId::new("example#Service")?;
    /// let get_thing = ShapeId::new("example#GetThing")?;
    /// let schemas = RuntimeSchemaSet::builder(&model)
    ///     .for_service_operations(&service, &[get_thing.clone()])
    ///     .build()?;
    /// assert!(schemas.operation(&get_thing).is_some());
    /// assert!(schemas.operation(&ShapeId::new("example#PutThing")?).is_none());
    /// assert!(schemas.schema(&ShapeId::new("example#PutInput")?).is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn for_service_operations(mut self, service: &ShapeId, operations: &[ShapeId]) -> Self {
        self.options.scope = Scope::Service {
            service: service.clone(),
            operations: OperationFilter::Selected(operations.into()),
        };
        self
    }

    /// The maximum number of diagnostics to collect before stopping. Defaults to 1,000, like
    /// [`ModelAssembler::max_diagnostics`](crate::ModelAssembler::max_diagnostics).
    pub fn max_diagnostics(mut self, max: usize) -> Self {
        self.options.max_diagnostics = max;
        self
    }

    /// When the set builds its schemas: all before [`build`](Self::build) returns (the
    /// default), or each on first use. See [`Materialization`].
    ///
    /// This is independent of the scope: every scope can be built either way, and it does
    /// not matter whether this is called before or after choosing one.
    ///
    /// ```
    /// # use aws_smithy_lang::runtime_schema::{Materialization, RuntimeSchemaSet};
    /// # use aws_smithy_lang::{Model, ShapeId};
    /// # let model = Model::from_json_str("m.json", r#"{"smithy": "2.0", "shapes": {
    /// #     "example#Service": {"type": "service", "operations": [
    /// #         {"target": "example#GetThing"}]},
    /// #     "example#GetThing": {"type": "operation", "input": {"target": "example#GetInput"}},
    /// #     "example#GetInput": {"type": "structure", "members": {}}
    /// # }}"#)?;
    /// let service = ShapeId::new("example#Service")?;
    /// let schemas = RuntimeSchemaSet::builder(&model)
    ///     .for_service(&service)
    ///     .materialization(Materialization::Lazy)
    ///     .build()?;
    /// // Validated, but nothing is built until it is used.
    /// let get_thing = ShapeId::new("example#GetThing")?;
    /// assert_eq!(schemas.operation_ids().collect::<Vec<_>>(), [&get_thing]);
    /// let operation = schemas.operation(&get_thing).unwrap(); // built now
    /// assert_eq!(operation.input().shape_id().as_str(), "example#GetInput");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn materialization(mut self, materialization: Materialization) -> Self {
        self.options.materialization = materialization;
        self
    }

    /// Limits how many roots, members, or model shapes a plan can index.
    #[cfg(test)]
    fn max_plan_entries(mut self, max: u32) -> Self {
        self.options.max_plan_entries = max;
        self
    }

    /// Validates the selected shapes and, unless the set is
    /// [lazy](Materialization::Lazy), builds their schemas.
    ///
    /// Nothing is built if any schema-relevant trait value is malformed or list and map
    /// members form a cycle; see [`SchemaBuildError`].
    pub fn build(self) -> Result<RuntimeSchemaSet, SchemaBuildError> {
        RuntimeSchemaSet::build(self.model.clone(), &self.options)
    }
}

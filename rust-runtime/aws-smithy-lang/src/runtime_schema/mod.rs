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
//! A [`RuntimeSchemaSet`] converts every serializable shape of a model, once, into the same
//! schema descriptors that generated code declares as statics. Protocols and codecs that
//! accept `&Schema<'_>` can then serialize and deserialize against a model loaded at
//! runtime, with no model lookups on the request path.
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

mod build;
mod error;
mod facts;
mod owner;

#[cfg(test)]
mod tests;

pub use aws_smithy_schema::OperationSchema;
pub use error::SchemaBuildError;

use crate::json_loader::DEFAULT_MAX_DIAGNOSTICS;
use crate::{Model, ShapeId};
use aws_smithy_schema::Schema;
use build::{Graph, OperationFilter, OperationRecord, Options, Scope};
use owner::{Arena, Storage};
use std::fmt;

/// An immutable set of runtime schemas built from a [`Model`].
///
/// See the [module documentation](self).
pub struct RuntimeSchemaSet {
    // The schemas in `storage` borrow from `model`, which safe Rust cannot express for two
    // fields of one struct; `owner` explains why that requires `unsafe` and why the
    // alternatives were rejected. Declared before `model` so that it drops first (INV-4).
    storage: Storage,
    graph: Graph,
    model: Model,
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
            },
        }
    }

    fn build(model: Model, options: &Options) -> Result<Self, SchemaBuildError> {
        // The schemas borrow `model`, which only moves its `Arc` pointer, never the data
        // (INV-4). If `build` fails or panics, everything it allocated is released.
        let (graph, storage) = Arena::build(&model, |model, builder| {
            build::build(model, builder, options)
        })?;
        Ok(Self {
            storage,
            graph,
            model,
        })
    }

    /// The model the schemas were built from.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The service the set was built for, if it was built with
    /// [`RuntimeSchemaSetBuilder::for_service`] or
    /// [`RuntimeSchemaSetBuilder::for_service_operations`].
    pub fn service(&self) -> Option<&ShapeId> {
        self.graph.service.as_ref()
    }

    /// The schema of a data shape or member, or `None` if it was not selected.
    ///
    /// For an operation input, this is the shape's own schema, without the operation's
    /// `@http` trait; use [`RuntimeSchemaSet::operation`] to serialize a request.
    pub fn schema(&self, id: &ShapeId) -> Option<&Schema<'_>> {
        let index = *self.graph.schemas.get(id)?;
        // SAFETY: INV-4: the schemas in `self.storage` borrow only model data behind
        // `self.model`'s `Arc`, which never moves and stays alive as long as `self`, and
        // `self.storage` stays alive for the returned borrow of `self`. `index` comes from
        // `self.graph`, which the same `Arena::build` call produced with `self.storage`, so
        // it addresses this storage (`Graph` itself borrows nothing).
        Some(unsafe { self.storage.schema(index) })
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
        let (id, record) = self.graph.operations.get_key_value(id)?;
        Some(self.operation_schema(id, record))
    }

    /// Every selected operation: in model order, or sorted by shape ID for a service.
    pub fn operations(&self) -> impl ExactSizeIterator<Item = OperationSchema<'_>> + '_ {
        self.graph
            .operations
            .iter()
            .map(|(id, record)| self.operation_schema(id, record))
    }

    fn operation_schema<'a>(
        &'a self,
        id: &'a ShapeId,
        record: &OperationRecord,
    ) -> OperationSchema<'a> {
        // SAFETY: INV-4, as in `schema`.
        let (input, output, errors) = unsafe {
            (
                self.storage.schema(record.input),
                self.storage.schema(record.output),
                self.storage.slice(record.errors),
            )
        };
        OperationSchema::builder(build::root_id(id))
            .input(input)
            .output(output)
            .errors(errors)
            .build()
    }

    /// The number of root data shape schemas, excluding members and operation inputs.
    pub fn len(&self) -> usize {
        self.graph.root_count
    }

    /// Returns `true` if no data shapes were selected.
    pub fn is_empty(&self) -> bool {
        self.graph.root_count == 0
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
        f.debug_struct("RuntimeSchemaSet")
            .field("service", &self.graph.service)
            .field("roots", &self.graph.root_count)
            .field("schemas", &self.storage.schema_count())
            .field("operations", &self.graph.operations.len())
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

    /// Validates the selected shapes and builds their schemas.
    ///
    /// Nothing is built if any schema-relevant trait value is malformed or list and map
    /// members form a cycle; see [`SchemaBuildError`].
    pub fn build(self) -> Result<RuntimeSchemaSet, SchemaBuildError> {
        RuntimeSchemaSet::build(self.model.clone(), &self.options)
    }
}

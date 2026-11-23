/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Indexes defined outside the crate, acquired with `Model::knowledge`.
//!
//! An integration test is a separate crate, so these indexes use only the public API, as a
//! downstream crate's would. The lint checks that the documented `Index<'_>` spelling compiles
//! when hidden lifetimes are denied.

#![deny(elided_lifetimes_in_paths)]

use aws_smithy_lang::knowledge::{Iter, KnowledgeIndex, OperationIndex, TopDownIndex};
use aws_smithy_lang::shape::{OperationView, StructureView};
use aws_smithy_lang::{Model, ShapeId};

const THINGS: ShapeId = ShapeId::from_static("ex#Things");
const THROTTLED: ShapeId = ShapeId::from_static("ex#Throttled");
const NOT_FOUND: ShapeId = ShapeId::from_static("ex#NotFound");

fn model() -> Model {
    Model::from_idl_str(
        "things.smithy",
        r#"
$version: "2"
namespace ex

service Things {
    operations: [Ping]
    resources: [Thing]
    errors: [Throttled]
}

resource Thing {
    identifiers: { id: String }
    read: GetThing
}

operation Ping {}

@readonly
operation GetThing {
    input := {
        @required
        id: String
    }
    errors: [NotFound]
}

@error("client")
structure Throttled {}

@error("client")
structure NotFound {}
"#,
    )
    .unwrap_or_else(|e| panic!("{:?}", e.diagnostics()))
}

/// Every error that some operation of a service can return, built on two built-in indexes.
struct ServiceErrorsIndex<'m> {
    top_down: TopDownIndex<'m>,
    operations: OperationIndex<'m>,
}

impl<'m> KnowledgeIndex<'m> for ServiceErrorsIndex<'m> {
    fn from_model(model: &'m Model) -> Self {
        Self {
            top_down: model.knowledge::<TopDownIndex<'_>>(),
            operations: model.knowledge::<OperationIndex<'_>>(),
        }
    }
}

impl<'m> ServiceErrorsIndex<'m> {
    /// The errors, sorted by shape ID. `None` if `service` is not a service or resource.
    fn errors(&self, service: &ShapeId) -> Option<Iter<StructureView<'m>>> {
        let mut errors = Vec::new();
        for operation in self.top_down.get_contained_operations(service)? {
            let operation = self.operations.get(operation.id()).unwrap();
            errors.extend(operation.errors_in(service).unwrap());
        }
        errors.sort_by(|a, b| a.id().cmp(b.id()));
        errors.dedup_by(|a, b| a.id() == b.id());
        Some(errors.into())
    }

    /// The operations of `service` that can return `error`, sorted by shape ID. `None` if
    /// `error` is not a structure.
    fn operations_returning(
        &self,
        service: &ShapeId,
        error: &ShapeId,
    ) -> Option<Iter<OperationView<'m>>> {
        let bindings: Vec<_> = self.operations.error_bindings(error)?.collect();
        // A common error of the service can be returned by every operation in it.
        if bindings.iter().any(|binding| binding.id() == service) {
            return self.top_down.get_contained_operations(service);
        }
        Some(
            bindings
                .into_iter()
                .filter_map(|binding| binding.as_shape()?.as_operation())
                .filter(|operation| self.top_down.contains(service, operation.id()))
                .collect(),
        )
    }
}

/// An index that owns its data instead of borrowing the model.
struct OperationCount(usize);

impl KnowledgeIndex<'_> for OperationCount {
    fn from_model(model: &Model) -> Self {
        Self(model.operations().count())
    }
}

fn ids<T>(items: impl IntoIterator<Item = T>, id: impl Fn(&T) -> &ShapeId) -> Vec<String> {
    items
        .into_iter()
        .map(|item| id(&item).to_string())
        .collect()
}

#[test]
fn index_defined_outside_the_crate() {
    let model = model();
    // Views borrow the model, not the index handle, so they outlive a temporary handle.
    let errors = model
        .knowledge::<ServiceErrorsIndex<'_>>()
        .errors(&THINGS)
        .unwrap();
    assert_eq!(ids(errors, |e| e.id()), ["ex#NotFound", "ex#Throttled"]);

    let index = model.knowledge::<ServiceErrorsIndex<'_>>();
    assert!(index.errors(&ShapeId::from_static("ex#Missing")).is_none());
}

#[test]
fn index_outside_the_crate_can_check_containment() {
    let model = model();
    let index = model.knowledge::<ServiceErrorsIndex<'_>>();
    let operations = |error: &ShapeId| {
        ids(index.operations_returning(&THINGS, error).unwrap(), |o| {
            o.id()
        })
    };
    assert_eq!(operations(&THROTTLED), ["ex#GetThing", "ex#Ping"]);
    assert_eq!(operations(&NOT_FOUND), ["ex#GetThing"]);
    assert!(index
        .operations_returning(&THINGS, &ShapeId::from_static("ex#Missing"))
        .is_none());
}

#[test]
fn index_type_can_be_inferred() {
    let model = model();
    let top_down: TopDownIndex<'_> = model.knowledge();
    assert_eq!(top_down.get_contained_operations(&THINGS).unwrap().len(), 2);
    let index: ServiceErrorsIndex<'_> = model.knowledge();
    assert_eq!(index.errors(&THINGS).unwrap().len(), 2);
}

#[test]
fn index_can_own_its_data() {
    let model = model();
    assert_eq!(model.knowledge::<OperationCount>().0, 2);
}

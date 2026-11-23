/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Indexes and cached data defined outside the crate, acquired with `Model::knowledge` and
//! `Model::knowledge_data`.
//!
//! An integration test is a separate crate, so these indexes use only the public API, as a
//! downstream crate's would. The lint checks that the documented `Index<'_>` spelling compiles
//! when hidden lifetimes are denied.

#![deny(elided_lifetimes_in_paths)]

use aws_smithy_lang::knowledge::{
    Iter, KnowledgeData, KnowledgeIndex, OperationIndex, ServiceClosureIndex, TopDownIndex,
};
use aws_smithy_lang::relationships::{NodeId, RelationshipIndex};
use aws_smithy_lang::shape::{OperationView, ServiceView, StructureView};
use aws_smithy_lang::{traits, Model, ShapeId};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Barrier};

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

service Other {
    operations: [Ping]
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

/// For each `@error` structure, the services whose closure contains it, in model order.
struct ErrorServices(BTreeMap<NodeId, Vec<NodeId>>);

impl KnowledgeData for ErrorServices {
    fn build_knowledge_data(model: &Model) -> Self {
        let relationships = model.knowledge::<RelationshipIndex<'_>>();
        let closures = model.knowledge::<ServiceClosureIndex<'_>>();
        let mut errors: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for service in model.services() {
            let closure = closures.get(service.id()).unwrap();
            let service = relationships.node(service.id()).unwrap();
            for node in closure.nodes() {
                if relationships.shape(node).has_trait(&traits::ERROR) {
                    errors.entry(node).or_default().push(service);
                }
            }
        }
        Self(errors)
    }
}

/// Answers from cached data that refers to shapes by node.
#[derive(Clone)]
struct ErrorServicesIndex<'m> {
    relationships: RelationshipIndex<'m>,
    data: Arc<ErrorServices>,
}

impl<'m> KnowledgeIndex<'m> for ErrorServicesIndex<'m> {
    fn from_model(model: &'m Model) -> Self {
        Self {
            relationships: model.knowledge::<RelationshipIndex<'_>>(),
            data: model.knowledge_data::<ErrorServices>(),
        }
    }
}

impl<'m> ErrorServicesIndex<'m> {
    /// The services that can return `error`. `None` if `error` does not exist.
    fn services(&self, error: &ShapeId) -> Option<Iter<ServiceView<'m>>> {
        let node = self.relationships.node(error)?;
        let services = self.data.0.get(&node).map_or(&[][..], Vec::as_slice);
        Some(
            services
                .iter()
                .filter_map(|&service| self.relationships.shape(service).as_service())
                .collect(),
        )
    }
}

fn ids<T>(items: impl IntoIterator<Item = T>, id: impl Fn(&T) -> &ShapeId) -> Vec<String> {
    items
        .into_iter()
        .map(|item| id(&item).to_string())
        .collect()
}

/// The message of a caught panic.
fn panic_message<T>(result: std::thread::Result<T>) -> String {
    let payload = result.err().expect("expected a panic");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload.downcast_ref::<&str>().unwrap().to_string(),
    }
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

#[test]
fn index_outside_the_crate_can_cache_data_by_node() {
    let model = model();
    let index = model.knowledge::<ErrorServicesIndex<'_>>();
    let services = |error: &ShapeId| ids(index.services(error).unwrap(), |s| s.id());
    assert_eq!(services(&THROTTLED), ["ex#Things", "ex#Other"]);
    assert_eq!(services(&NOT_FOUND), ["ex#Things"]);
    assert!(services(&THINGS).is_empty());
    assert!(index
        .services(&ShapeId::from_static("ex#Missing"))
        .is_none());

    let clone = model.clone();
    let again = clone.knowledge::<ErrorServicesIndex<'_>>();
    assert!(Arc::ptr_eq(&index.data, &again.data));
}

static COUNTED_BUILDS: AtomicUsize = AtomicUsize::new(0);

/// The build number.
struct Counted(usize);

impl KnowledgeData for Counted {
    fn build_knowledge_data(_: &Model) -> Self {
        Self(COUNTED_BUILDS.fetch_add(1, SeqCst) + 1)
    }
}

#[test]
fn data_is_built_once_per_model() {
    let model = model();
    let first = model.knowledge_data::<Counted>();
    assert_eq!(first.0, 1);
    let clone = model.clone();
    assert!(Arc::ptr_eq(&first, &clone.knowledge_data::<Counted>()));
    assert!(Arc::ptr_eq(&first, &model.knowledge_data::<Counted>()));
    // An equivalent model loaded separately has its own data.
    let other = self::model();
    assert_eq!(other.knowledge_data::<Counted>().0, 2);
    assert_eq!(COUNTED_BUILDS.load(SeqCst), 2);
}

/// The operations of `ex#Things`.
struct ThingsOperations(Vec<String>);

impl KnowledgeData for ThingsOperations {
    fn build_knowledge_data(model: &Model) -> Self {
        let top_down = model.knowledge::<TopDownIndex<'_>>();
        let operations = top_down.get_contained_operations(&THINGS).unwrap();
        Self(ids(operations, |o| o.id()))
    }
}

/// Built from other data.
struct ThingsOperationCount(usize);

impl KnowledgeData for ThingsOperationCount {
    fn build_knowledge_data(model: &Model) -> Self {
        Self(model.knowledge_data::<ThingsOperations>().0.len())
    }
}

#[test]
fn data_can_depend_on_other_data() {
    let model = model();
    assert_eq!(model.knowledge_data::<ThingsOperationCount>().0, 2);
    assert_eq!(
        model.knowledge_data::<ThingsOperations>().0,
        ["ex#GetThing", "ex#Ping"]
    );
}

struct SelfCycle;

impl KnowledgeData for SelfCycle {
    fn build_knowledge_data(model: &Model) -> Self {
        model.knowledge_data::<SelfCycle>();
        SelfCycle
    }
}

struct CycleA;

impl KnowledgeData for CycleA {
    fn build_knowledge_data(model: &Model) -> Self {
        model.knowledge_data::<CycleB>();
        CycleA
    }
}

struct CycleB;

impl KnowledgeData for CycleB {
    fn build_knowledge_data(model: &Model) -> Self {
        model.knowledge_data::<CycleA>();
        CycleB
    }
}

/// Checks a cycle panic message, which names each type with `std::any::type_name`.
fn assert_cycle(message: &str, chain: &[&str]) {
    let names = message
        .strip_prefix("knowledge data depends on itself: ")
        .unwrap_or_else(|| panic!("unexpected message: {message}"));
    let names: Vec<_> = names.split(" -> ").collect();
    assert_eq!(names.len(), chain.len(), "{message}");
    for (name, expected) in names.iter().zip(chain) {
        assert!(name.ends_with(expected), "{message}");
    }
}

#[test]
fn data_cycles_panic_with_the_chain() {
    let model = model();
    let message = panic_message(catch_unwind(AssertUnwindSafe(|| {
        model.knowledge_data::<SelfCycle>()
    })));
    assert_cycle(&message, &["SelfCycle", "SelfCycle"]);
    let message = panic_message(catch_unwind(AssertUnwindSafe(|| {
        model.knowledge_data::<CycleA>()
    })));
    assert_cycle(&message, &["CycleA", "CycleB", "CycleA"]);
    // The same thread can still build data for this model.
    assert_eq!(model.knowledge_data::<ThingsOperations>().0.len(), 2);
}

static FLAKY_BUILDS: AtomicUsize = AtomicUsize::new(0);

/// Fails on its first build.
struct Flaky(usize);

impl KnowledgeData for Flaky {
    fn build_knowledge_data(_: &Model) -> Self {
        let build = FLAKY_BUILDS.fetch_add(1, SeqCst) + 1;
        assert!(build > 1, "the first build fails");
        Self(build)
    }
}

#[test]
fn data_whose_build_panics_is_built_again() {
    let model = model();
    let message = panic_message(catch_unwind(AssertUnwindSafe(|| {
        model.knowledge_data::<Flaky>()
    })));
    assert_eq!(message, "the first build fails");
    // A panicking build doesn't leave the type marked as being built, so this isn't a cycle.
    let second = model.knowledge_data::<Flaky>();
    assert_eq!(second.0, 2);
    assert!(Arc::ptr_eq(&second, &model.knowledge_data::<Flaky>()));
}

static RACED_BUILDS: AtomicUsize = AtomicUsize::new(0);

struct Raced;

impl KnowledgeData for Raced {
    fn build_knowledge_data(_: &Model) -> Self {
        RACED_BUILDS.fetch_add(1, SeqCst);
        Raced
    }
}

#[test]
fn concurrent_first_use_shares_one_value() {
    const THREADS: usize = 8;
    let model = model();
    let barrier = Barrier::new(THREADS);
    let values: Vec<Arc<Raced>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    model.knowledge_data::<Raced>()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(values.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])));
    assert!(Arc::ptr_eq(&values[0], &model.knowledge_data::<Raced>()));
    // Racing threads may each build, but only one value is kept.
    assert!((1..=THREADS).contains(&RACED_BUILDS.load(SeqCst)));
}

struct Dropped;

impl KnowledgeData for Dropped {
    fn build_knowledge_data(_: &Model) -> Self {
        Dropped
    }
}

#[test]
fn data_is_dropped_with_the_last_clone_of_its_model() {
    let model = model();
    let data = Arc::downgrade(&model.knowledge_data::<Dropped>());
    let clone = model.clone();
    drop(model);
    assert!(data.upgrade().is_some());
    drop(clone);
    assert!(data.upgrade().is_none());
}

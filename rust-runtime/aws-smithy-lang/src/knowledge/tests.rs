/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use crate::model::Model;
use crate::relationships::RelationshipType::{self, *};
use crate::shape::{OperationView, ShapeRef};
use crate::shape_id::ShapeId;
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

/// - `ex#Service` binds operations directly and through a resource tree with every binding kind.
///   `ex#SharedOp` is bound both directly and by `ex#Thing`; `ex#Shared` is a child of both
///   `ex#Thing` and `ex#Child`.
/// - `ex#Hidden` has no direct operations: its only operation is reached through two levels
///   of resources. This is the shape of the bug the crate-root example once had.
/// - `ex#Other` shares `ex#Shared` with `ex#Service`.
const MODEL: &str = include_str!("../../tests/fixtures/java/knowledge/structural.json");

fn model() -> Model {
    Model::from_json_str("knowledge.json", MODEL).unwrap()
}

fn ids<'a, T: 'a>(
    items: impl IntoIterator<Item = T>,
    id: impl Fn(&T) -> &'a ShapeId,
) -> Vec<String> {
    items
        .into_iter()
        .map(|item| id(&item).to_string())
        .collect()
}

fn op_ids<'m>(items: impl IntoIterator<Item = OperationView<'m>>) -> Vec<String> {
    items.into_iter().map(|op| op.id().to_string()).collect()
}

fn ex(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| format!("ex#{name}")).collect()
}

#[test]
fn top_down_includes_every_binding_kind_once_sorted() {
    let model = model();
    let top_down = model.knowledge().top_down();
    let operations = op_ids(
        top_down
            .get_contained_operations(&ShapeId::from_static("ex#Service"))
            .unwrap(),
    );
    assert_eq!(
        operations,
        ex(&[
            "BatchThing",
            "ChildOp",
            "CreateThing",
            "DeleteThing",
            "DirectOp",
            "GetThing",
            "ListThings",
            "PutThing",
            "SharedOp",
            "SharedRead",
            "ThingAction",
            "UpdateThing",
        ])
    );
    let resources = ids(
        top_down
            .get_contained_resources(&ShapeId::from_static("ex#Service"))
            .unwrap(),
        |r| r.id(),
    );
    assert_eq!(resources, ex(&["Child", "Shared", "Thing"]));

    // Resources: the start is excluded, children are included.
    let thing_ops = op_ids(
        top_down
            .get_contained_operations(&ShapeId::from_static("ex#Thing"))
            .unwrap(),
    );
    assert!(!thing_ops.contains(&"ex#DirectOp".to_owned()));
    assert_eq!(thing_ops.len(), 11);
    let thing_resources = ids(
        top_down
            .get_contained_resources(&ShapeId::from_static("ex#Thing"))
            .unwrap(),
        |r| r.id(),
    );
    assert_eq!(thing_resources, ex(&["Child", "Shared"]));
    assert_eq!(
        op_ids(
            top_down
                .get_contained_operations(&ShapeId::from_static("ex#Child"))
                .unwrap()
        ),
        ex(&["ChildOp", "SharedRead"])
    );
    assert_eq!(
        top_down
            .get_contained_resources(&ShapeId::from_static("ex#Shared"))
            .unwrap()
            .len(),
        0
    );

    // Only services and resources have closures.
    assert!(top_down
        .get_contained_operations(&ShapeId::from_static("ex#GetThing"))
        .is_none());
    assert!(top_down
        .get_contained_operations(&ShapeId::from_static("ex#Missing"))
        .is_none());
    assert!(top_down
        .get_contained_resources(&ShapeId::from_static("ex#Plain"))
        .is_none());
    assert!(top_down
        .get_contained_operations(&ShapeId::from_static("ex#GetThingInput$alpha"))
        .is_none());
}

/// The crate-root example once iterated `service.operations()`, which silently missed every
/// operation bound through a resource.
#[test]
fn nested_resource_only_operation_is_found() {
    let model = model();
    let hidden = model
        .expect_shape(&ShapeId::from_static("ex#Hidden"))
        .unwrap()
        .expect_service()
        .unwrap();
    assert_eq!(hidden.direct_operations().len(), 0);
    assert_eq!(op_ids(hidden.contained_operations()), ex(&["DeepOp"]));
    assert_eq!(
        ids(hidden.contained_resources(), |r| r.id()),
        ex(&["Inner", "Outer"])
    );
    let outer = model
        .expect_shape(&ShapeId::from_static("ex#Outer"))
        .unwrap()
        .expect_resource()
        .unwrap();
    assert_eq!(op_ids(outer.contained_operations()), ex(&["DeepOp"]));
    assert_eq!(ids(outer.contained_resources(), |r| r.id()), ex(&["Inner"]));
    assert_eq!(outer.child_resources().len(), 1);
}

#[test]
fn typed_conveniences_match_the_index() {
    let model = model();
    let top_down = model.knowledge().top_down();
    for service in model.services() {
        assert_eq!(
            op_ids(service.contained_operations()),
            op_ids(top_down.get_contained_operations(service.id()).unwrap())
        );
        // Direct declarations are a subset of the complete closure.
        let contained = op_ids(service.contained_operations());
        for direct in service.direct_operations() {
            assert!(contained.contains(&direct.to_string()));
        }
    }
    for resource in model.resources() {
        assert_eq!(
            ids(resource.contained_resources(), |r| r.id()),
            ids(
                top_down.get_contained_resources(resource.id()).unwrap(),
                |r| r.id()
            )
        );
    }
}

#[test]
fn operation_input_output_and_members() {
    let model = model();
    let operations = model.knowledge().operations();

    let get = operations
        .get(&ShapeId::from_static("ex#GetThing"))
        .unwrap();
    assert_eq!(get.operation().id(), "ex#GetThing");
    assert_eq!(get.input_shape().id(), "ex#GetThingInput");
    assert_eq!(get.meaningful_input().unwrap().id(), "ex#GetThingInput");
    assert_eq!(get.meaningful_output().unwrap().id(), "ex#GetThingOutput");
    assert_eq!(
        get.input_members().names().collect::<Vec<_>>(),
        ["zeta", "alpha", "mid"]
    );
    assert_eq!(get.output_members().names().collect::<Vec<_>>(), ["thing"]);

    let direct = operations
        .get(&ShapeId::from_static("ex#DirectOp"))
        .unwrap();
    assert_eq!(direct.input_shape().id(), "smithy.api#Unit");
    assert_eq!(direct.output_shape().id(), "smithy.api#Unit");
    assert!(direct.meaningful_input().is_none());
    assert!(direct.meaningful_output().is_none());
    assert!(direct.input_members().is_empty());

    assert!(operations.get(&ShapeId::from_static("ex#Plain")).is_none());
    assert!(operations
        .get(&ShapeId::from_static("ex#Missing"))
        .is_none());
}

#[test]
fn operation_errors() {
    let model = model();
    let operations = model.knowledge().operations();
    let get = operations
        .get(&ShapeId::from_static("ex#GetThing"))
        .unwrap();
    assert_eq!(
        ids(get.direct_errors(), |e| e.id()),
        ex(&["NotFound", "Common"])
    );
    // Service errors first, then operation errors; `Common` keeps its first position.
    assert_eq!(
        ids(
            get.errors_in(&ShapeId::from_static("ex#Service")).unwrap(),
            |e| e.id()
        ),
        ex(&["Throttled", "Common", "NotFound"])
    );
    // A service that does not contain the operation, or something that is not a service.
    assert!(get.errors_in(&ShapeId::from_static("ex#Hidden")).is_none());
    assert!(get.errors_in(&ShapeId::from_static("ex#Other")).is_none());
    assert!(get.errors_in(&ShapeId::from_static("ex#Thing")).is_none());
    assert!(get.errors_in(&ShapeId::from_static("ex#Missing")).is_none());
    let unbound = operations.get(&ShapeId::from_static("ex#Unbound")).unwrap();
    assert!(unbound
        .errors_in(&ShapeId::from_static("ex#Service"))
        .is_none());
    assert_eq!(ids(unbound.direct_errors(), |e| e.id()), ex(&["NotFound"]));
    // Deep and directly bound operations both inherit service errors.
    assert_eq!(
        ids(
            operations
                .get(&ShapeId::from_static("ex#SharedRead"))
                .unwrap()
                .errors_in(&ShapeId::from_static("ex#Service"))
                .unwrap(),
            |e| e.id()
        ),
        ex(&["Throttled", "Common"])
    );
    assert_eq!(
        operations
            .get(&ShapeId::from_static("ex#DeepOp"))
            .unwrap()
            .errors_in(&ShapeId::from_static("ex#Hidden"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn operation_reverse_bindings() {
    let model = model();
    let operations = model.knowledge().operations();
    assert_eq!(
        op_ids(
            operations
                .input_bindings(&ShapeId::from_static("ex#SharedInput"))
                .unwrap()
        ),
        ex(&["CreateThing", "PutThing"])
    );
    assert_eq!(
        op_ids(
            operations
                .output_bindings(&ShapeId::from_static("ex#GetThingOutput"))
                .unwrap()
        ),
        ex(&["GetThing"])
    );
    assert_eq!(
        operations
            .input_bindings(&ShapeId::from_static("ex#Plain"))
            .unwrap()
            .len(),
        0
    );
    assert!(operations
        .input_bindings(&ShapeId::from_static("ex#GetThing"))
        .is_none());
    assert!(operations
        .output_bindings(&ShapeId::from_static("ex#Missing"))
        .is_none());
    // Omitted input normalizes to Unit, which is therefore bound.
    assert!(op_ids(
        operations
            .input_bindings(&ShapeId::from_static("smithy.api#Unit"))
            .unwrap()
    )
    .contains(&"ex#DirectOp".to_owned()));

    let not_found: Vec<_> = operations
        .error_bindings(&ShapeId::from_static("ex#NotFound"))
        .unwrap()
        .map(|s| (s.id().to_string(), s.shape_type()))
        .collect();
    assert_eq!(
        not_found,
        [
            ("ex#GetThing".to_owned(), crate::ShapeType::Operation),
            ("ex#Unbound".to_owned(), crate::ShapeType::Operation),
            ("ex#UpdateThing".to_owned(), crate::ShapeType::Operation),
        ]
    );
    let common: Vec<_> = ids(
        operations
            .error_bindings(&ShapeId::from_static("ex#Common"))
            .unwrap(),
        |s| s.id(),
    );
    assert_eq!(common, ex(&["GetThing", "Service"]));
    assert!(operations
        .error_bindings(&ShapeId::from_static("ex#GetThing"))
        .is_none());

    assert!(operations.is_input_structure(&ShapeId::from_static("ex#SharedInput")));
    assert!(operations.is_input_structure(&ShapeId::from_static("ex#MarkedInput")));
    assert!(!operations.is_input_structure(&ShapeId::from_static("ex#MarkedOutput")));
    assert!(!operations.is_input_structure(&ShapeId::from_static("ex#Plain")));
    assert!(!operations.is_input_structure(&ShapeId::from_static("ex#Missing")));
    assert!(operations.is_output_structure(&ShapeId::from_static("ex#GetThingOutput")));
    assert!(operations.is_output_structure(&ShapeId::from_static("ex#MarkedOutput")));
    assert!(!operations.is_output_structure(&ShapeId::from_static("ex#GetThingInput")));
}

fn parent_ids(bindings: Iter<ParentBinding<'_>>) -> Vec<(String, RelationshipType)> {
    bindings
        .map(|b| (b.parent().id().to_string(), b.kind()))
        .collect()
}

fn chain_ids(chain: &ParentChain<'_>) -> Vec<String> {
    chain.iter().map(|e| e.id().to_string()).collect()
}

#[test]
fn bottom_up_unique_chains() {
    let model = model();
    let bottom_up = model.knowledge().bottom_up();
    assert_eq!(
        parent_ids(
            bottom_up
                .get_direct_bindings(
                    &ShapeId::from_static("ex#Service"),
                    &ShapeId::from_static("ex#GetThing")
                )
                .unwrap()
        ),
        [("ex#Thing".to_owned(), Read)]
    );
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#GetThing"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["Thing", "Service"]));
    assert_eq!(chain.parent().id(), "ex#Thing");
    assert_eq!(chain.service().id(), "ex#Service");
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#ChildOp"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["Child", "Thing", "Service"]));
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#DirectOp"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["Service"]));
    assert!(matches!(chain.parent(), EntityView::Service(_)));
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Hidden"),
            &ShapeId::from_static("ex#DeepOp"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["Inner", "Outer", "Hidden"]));
    let collected: Vec<_> = chain.into_iter().map(|e| e.id().to_string()).collect();
    assert_eq!(collected, ex(&["Inner", "Outer", "Hidden"]));
    assert_eq!(
        bottom_up
            .unique_parent(
                &ShapeId::from_static("ex#Service"),
                &ShapeId::from_static("ex#BatchThing")
            )
            .unwrap()
            .unwrap()
            .id(),
        "ex#Thing"
    );
    assert_eq!(
        parent_ids(
            bottom_up
                .get_direct_bindings(
                    &ShapeId::from_static("ex#Service"),
                    &ShapeId::from_static("ex#BatchThing")
                )
                .unwrap()
        ),
        [("ex#Thing".to_owned(), CollectionOperation)]
    );
}

#[test]
fn bottom_up_reports_ambiguity() {
    let model = model();
    let bottom_up = model.knowledge().bottom_up();

    // Bound directly to the service and through a resource.
    assert_eq!(
        parent_ids(
            bottom_up
                .get_direct_bindings(
                    &ShapeId::from_static("ex#Service"),
                    &ShapeId::from_static("ex#SharedOp")
                )
                .unwrap()
        ),
        [
            ("ex#Service".to_owned(), Operation),
            ("ex#Thing".to_owned(), Operation)
        ]
    );
    let err = bottom_up
        .unique_parent(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#SharedOp"),
        )
        .unwrap_err();
    assert_eq!(err.service(), "ex#Service");
    assert_eq!(err.entity(), "ex#SharedOp");
    assert_eq!(
        ids(err.candidates().iter(), |c| *c),
        ex(&["Service", "Thing"])
    );
    assert_eq!(
        err.to_string(),
        "`ex#SharedOp` has multiple parents in service `ex#Service`: `ex#Service`, `ex#Thing`"
    );
    assert!(bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#SharedOp")
        )
        .is_err());

    // A shared child resource: the operation has a unique parent, but the chain is ambiguous
    // one step up.
    assert_eq!(
        bottom_up
            .unique_parent(
                &ShapeId::from_static("ex#Service"),
                &ShapeId::from_static("ex#SharedRead")
            )
            .unwrap()
            .unwrap()
            .id(),
        "ex#Shared"
    );
    let err = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#SharedRead"),
        )
        .unwrap_err();
    assert_eq!(err.entity(), "ex#Shared");
    assert_eq!(
        ids(err.candidates().iter(), |c| *c),
        ex(&["Child", "Thing"])
    );
    assert_eq!(
        ids(
            bottom_up
                .get_ancestors(
                    &ShapeId::from_static("ex#Service"),
                    &ShapeId::from_static("ex#SharedRead")
                )
                .unwrap(),
            |e| e.id()
        ),
        ex(&["Child", "Service", "Shared", "Thing"])
    );
}

#[test]
fn bottom_up_is_service_scoped() {
    let model = model();
    let bottom_up = model.knowledge().bottom_up();
    // `ex#Shared` is ambiguous in `ex#Service` but has one parent in `ex#Other`.
    assert_eq!(
        parent_ids(
            bottom_up
                .get_direct_bindings(
                    &ShapeId::from_static("ex#Other"),
                    &ShapeId::from_static("ex#Shared")
                )
                .unwrap()
        ),
        [("ex#Other".to_owned(), Resource)]
    );
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Other"),
            &ShapeId::from_static("ex#SharedRead"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["Shared", "Other"]));
    assert_eq!(
        ids(
            bottom_up
                .get_ancestors(
                    &ShapeId::from_static("ex#Other"),
                    &ShapeId::from_static("ex#SharedRead")
                )
                .unwrap(),
            |e| e.id()
        ),
        ex(&["Other", "Shared"])
    );

    // Not contained, wrong kinds, or missing.
    assert!(bottom_up
        .get_direct_bindings(
            &ShapeId::from_static("ex#Other"),
            &ShapeId::from_static("ex#GetThing")
        )
        .is_none());
    assert!(bottom_up
        .unique_parent(
            &ShapeId::from_static("ex#Other"),
            &ShapeId::from_static("ex#GetThing")
        )
        .unwrap()
        .is_none());
    assert!(bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#Other"),
            &ShapeId::from_static("ex#GetThing")
        )
        .unwrap()
        .is_none());
    assert!(bottom_up
        .get_direct_bindings(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#Unbound")
        )
        .is_none());
    assert!(bottom_up
        .get_direct_bindings(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#Service")
        )
        .is_none());
    assert!(bottom_up
        .get_direct_bindings(
            &ShapeId::from_static("ex#Thing"),
            &ShapeId::from_static("ex#GetThing")
        )
        .is_none());
    assert!(bottom_up
        .get_direct_bindings(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#Plain")
        )
        .is_none());
    assert!(bottom_up
        .get_ancestors(
            &ShapeId::from_static("ex#Missing"),
            &ShapeId::from_static("ex#GetThing")
        )
        .is_none());
}

#[test]
fn one_parent_binding_twice_is_unique() {
    let model = Model::from_json_str(
        "m",
        r#"{"smithy": "2.0", "shapes": {
            "ex#S": {"type": "service", "resources": [{"target": "ex#R"}]},
            "ex#R": {"type": "resource", "read": {"target": "ex#Op"},
                     "operations": [{"target": "ex#Op"}]},
            "ex#Op": {"type": "operation"}
        }}"#,
    )
    .unwrap();
    let bottom_up = model.knowledge().bottom_up();
    assert_eq!(
        parent_ids(
            bottom_up
                .get_direct_bindings(
                    &ShapeId::from_static("ex#S"),
                    &ShapeId::from_static("ex#Op")
                )
                .unwrap()
        ),
        [("ex#R".to_owned(), Operation), ("ex#R".to_owned(), Read)]
    );
    let chain = bottom_up
        .parent_chain(
            &ShapeId::from_static("ex#S"),
            &ShapeId::from_static("ex#Op"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chain_ids(&chain), ex(&["R", "S"]));
}

#[test]
fn relationship_index_outgoing_and_incoming() {
    let model = model();
    let relationships = model.knowledge().relationships();
    let outgoing: Vec<_> = relationships
        .get_outgoing(&ShapeId::from_static("ex#GetThing"))
        .unwrap()
        .map(|r| (r.kind(), r.target_id().to_string()))
        .collect();
    assert_eq!(
        outgoing,
        [
            (Input, "ex#GetThingInput".to_owned()),
            (Output, "ex#GetThingOutput".to_owned()),
            (Error, "ex#NotFound".to_owned()),
            (Error, "ex#Common".to_owned()),
        ]
    );
    let incoming: Vec<_> = relationships
        .get_incoming(&ShapeId::from_static("ex#Common"))
        .unwrap()
        .map(|r| {
            assert_eq!(r.target_id(), "ex#Common");
            (r.source().id().to_string(), r.kind())
        })
        .collect();
    assert_eq!(
        incoming,
        [
            ("ex#GetThing".to_owned(), Error),
            ("ex#Service".to_owned(), Error)
        ]
    );
    // Sorted by source, then kind.
    let incoming: Vec<_> = relationships
        .get_incoming(&ShapeId::from_static("ex#SharedOp"))
        .unwrap()
        .map(|r| (r.source().id().to_string(), r.kind()))
        .collect();
    assert_eq!(
        incoming,
        [
            ("ex#Service".to_owned(), Operation),
            ("ex#Thing".to_owned(), Operation)
        ]
    );
    let member: Vec<_> = relationships
        .get_incoming(&ShapeId::from_static("ex#GetThingInput$alpha"))
        .unwrap()
        .map(|r| (r.source().id().to_string(), r.kind()))
        .collect();
    assert_eq!(member, [("ex#GetThingInput".to_owned(), Member)]);
    assert_eq!(
        relationships
            .get_incoming(&ShapeId::from_static("ex#Plain"))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        relationships
            .get_outgoing(&ShapeId::from_static("ex#Plain"))
            .unwrap()
            .len(),
        0
    );
    assert!(relationships
        .get_outgoing(&ShapeId::from_static("ex#Missing"))
        .is_none());
    assert!(relationships
        .get_incoming(&ShapeId::from_static("ex#Plain$nope"))
        .is_none());
    // Not a shape ID, so it cannot be passed to a lookup.
    assert!(ShapeId::new("ex#GetThingInput$alpha$x").is_err());
    // Prelude shapes are indexed too.
    assert!(
        relationships
            .get_incoming(&ShapeId::from_static("smithy.api#String"))
            .unwrap()
            .len()
            >= 4
    );

    let view = model
        .expect_shape(&ShapeId::from_static("ex#Common"))
        .unwrap();
    assert_eq!(view.incoming_relationships().len(), 2);
    assert_eq!(
        format!("{:?}", view.incoming_relationships()),
        r#"Relationships { shape: ShapeId("ex#Common"), direction: "incoming", remaining: 2 }"#
    );
}

/// Every edge appears exactly once in its source's outgoing list and once in its target's
/// incoming list.
fn assert_symmetric(model: &Model) {
    let relationships = model.knowledge().relationships();
    let mut outgoing = BTreeMap::new();
    let mut incoming = BTreeMap::new();
    let count = |map: &mut BTreeMap<(String, u8, String), usize>,
                 r: crate::relationships::Relationship<'_>| {
        *map.entry((
            r.source().id().to_string(),
            r.kind().rank(),
            r.target_id().to_string(),
        ))
        .or_default() += 1;
    };
    let mut all = Vec::new();
    for shape in model.shapes() {
        all.push(ShapeRef::Shape(shape));
        all.extend(shape.members().iter().map(ShapeRef::Member));
    }
    for shape in &all {
        let direct: Vec<_> = crate::relationships::relationships(*shape)
            .into_iter()
            .map(|r| format!("{r:?}"))
            .collect();
        let cached: Vec<_> = shape.relationships().map(|r| format!("{r:?}")).collect();
        assert_eq!(direct, cached, "{}", shape.id());
        for r in relationships.get_outgoing(shape.id()).unwrap() {
            assert_eq!(r.source().id(), shape.id());
            assert!(model.contains_shape(r.target_id()));
            count(&mut outgoing, r);
        }
        let mut previous: Option<(String, u8)> = None;
        for r in relationships.get_incoming(shape.id()).unwrap() {
            assert_eq!(r.target_id(), shape.id());
            let key = (r.source().id().to_string(), r.kind().rank());
            assert!(
                previous.as_ref().is_none_or(|p| *p <= key),
                "unsorted incoming"
            );
            previous = Some(key);
            count(&mut incoming, r);
        }
    }
    assert_eq!(outgoing, incoming);
}

#[test]
fn relationships_are_symmetric_and_match_traversal() {
    assert_symmetric(&model());
}

#[test]
fn caches_are_shared_by_clones_and_threads() {
    let model = model();
    let clone = model.clone();
    // Initialize through a clone; the original observes the same cache.
    let first = clone.knowledge().relationships().index as *const _;
    let second = model.knowledge().relationships().index as *const _;
    assert!(std::ptr::eq(first, second));

    let fresh = Model::from_json_str("m", MODEL).unwrap();
    let addresses: Vec<usize> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let model = fresh.clone();
                scope.spawn(move || {
                    let top_down = model.knowledge().top_down();
                    let ops = op_ids(
                        top_down
                            .get_contained_operations(&ShapeId::from_static("ex#Service"))
                            .unwrap(),
                    );
                    assert_eq!(ops.len(), 12);
                    model.knowledge().relationships().index as *const _ as usize
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(addresses.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn deterministic_across_independent_loads() {
    let snapshot = |model: &Model| {
        let knowledge = model.knowledge();
        let mut out = Vec::new();
        for service in model.services() {
            out.push(op_ids(service.contained_operations()));
            for op in service.contained_operations() {
                if let Some(errors) = knowledge
                    .operations()
                    .get(op.id())
                    .unwrap()
                    .errors_in(service.id())
                {
                    out.push(ids(errors, |e| e.id()));
                }
                out.push(ids(
                    knowledge
                        .bottom_up()
                        .get_ancestors(service.id(), op.id())
                        .unwrap(),
                    |e| e.id(),
                ));
            }
        }
        out
    };
    let a = snapshot(&model());
    let b = snapshot(&model());
    assert_eq!(a, b);
}

#[test]
fn send_sync_and_debug() {
    fn assert<T: Send + Sync + Clone + std::fmt::Debug>(_: &T) {}
    let model = model();
    let knowledge = model.knowledge();
    assert(&knowledge);
    assert(&knowledge.relationships());
    assert(&knowledge.top_down());
    assert(&knowledge.operations());
    assert(&knowledge.bottom_up());
    assert(
        &knowledge
            .operations()
            .get(&ShapeId::from_static("ex#GetThing"))
            .unwrap(),
    );
    assert(
        &knowledge
            .relationships()
            .get_incoming(&ShapeId::from_static("ex#Common"))
            .unwrap(),
    );
    assert(
        &knowledge
            .top_down()
            .get_contained_operations(&ShapeId::from_static("ex#Service"))
            .unwrap(),
    );
    let bottom_up = knowledge.bottom_up();
    assert(
        &bottom_up
            .get_direct_bindings(
                &ShapeId::from_static("ex#Service"),
                &ShapeId::from_static("ex#GetThing"),
            )
            .unwrap(),
    );
    assert(
        &bottom_up
            .parent_chain(
                &ShapeId::from_static("ex#Service"),
                &ShapeId::from_static("ex#GetThing"),
            )
            .unwrap()
            .unwrap(),
    );
    let err = bottom_up
        .unique_parent(
            &ShapeId::from_static("ex#Service"),
            &ShapeId::from_static("ex#SharedOp"),
        )
        .unwrap_err();
    assert(&err);
    fn assert_error<E: std::error::Error + Send + Sync + 'static>(_: &E) {}
    assert_error(&err);
    assert_eq!(format!("{knowledge:?}"), "Knowledge { .. }");
}

#[test]
fn relationship_type_classification() {
    for kind in [
        Operation,
        CollectionOperation,
        Create,
        Put,
        Read,
        Update,
        Delete,
        List,
    ] {
        assert!(kind.is_operation_binding());
        assert!(kind.is_containment());
    }
    assert!(!Resource.is_operation_binding());
    assert!(Resource.is_containment());
    for kind in [
        Error,
        Identifier,
        Property,
        Input,
        Output,
        Member,
        MemberContainer,
        MemberTarget,
    ] {
        assert!(!kind.is_containment(), "{kind:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Property tests against simple uncached oracles over random acyclic containment graphs.

const LIFECYCLE: [&str; 6] = ["create", "put", "read", "update", "delete", "list"];
const OPS: usize = 6;
const RESOURCES: usize = 5;
const SERVICES: usize = 2;
const ERRORS: usize = 3;

#[derive(Debug, Clone)]
struct GenResource {
    /// Child resources; only higher-numbered resources, so containment is acyclic.
    children: u32,
    operations: u32,
    collection: u32,
    lifecycle: [Option<usize>; 6],
}

#[derive(Debug, Clone)]
struct Graph {
    resources: Vec<GenResource>,
    service_ops: Vec<u32>,
    service_resources: Vec<u32>,
    service_errors: Vec<u32>,
    op_errors: Vec<u32>,
}

fn bits(mask: u32, n: usize) -> impl Iterator<Item = usize> {
    (0..n).filter(move |i| mask & (1 << i) != 0)
}

fn graph() -> impl Strategy<Value = Graph> {
    let resource = |i: usize| {
        (
            0u32..(1 << RESOURCES),
            0u32..(1 << OPS),
            0u32..(1 << OPS),
            proptest::array::uniform6(proptest::option::weighted(0.3, 0..OPS)),
        )
            .prop_map(
                move |(children, operations, collection, lifecycle)| GenResource {
                    children: children & !((1u32 << (i + 1)) - 1),
                    operations,
                    collection: collection & !operations,
                    lifecycle,
                },
            )
    };
    (
        (0..RESOURCES).map(resource).collect::<Vec<_>>(),
        proptest::collection::vec(0u32..(1 << OPS), SERVICES),
        proptest::collection::vec(0u32..(1 << RESOURCES), SERVICES),
        proptest::collection::vec(0u32..(1 << ERRORS), SERVICES),
        proptest::collection::vec(0u32..(1 << ERRORS), OPS),
    )
        .prop_map(
            |(resources, service_ops, service_resources, service_errors, op_errors)| Graph {
                resources,
                service_ops,
                service_resources,
                service_errors,
                op_errors,
            },
        )
}

fn refs(prefix: &str, ids: impl Iterator<Item = usize>) -> String {
    let refs: Vec<_> = ids
        .map(|i| format!(r#"{{"target":"ex#{prefix}{i}"}}"#))
        .collect();
    format!("[{}]", refs.join(","))
}

impl Graph {
    fn json(&self) -> String {
        let mut shapes = Vec::new();
        for s in 0..SERVICES {
            // Service errors are listed in reverse so their order differs from operation order.
            let mut errors: Vec<_> = bits(self.service_errors[s], ERRORS).collect();
            errors.reverse();
            shapes.push(format!(
                r#""ex#S{s}":{{"type":"service","operations":{},"resources":{},"errors":{}}}"#,
                refs("O", bits(self.service_ops[s], OPS)),
                refs("R", bits(self.service_resources[s], RESOURCES)),
                refs("E", errors.into_iter()),
            ));
        }
        for (r, resource) in self.resources.iter().enumerate() {
            let mut fields = vec![
                format!(
                    r#""operations":{}"#,
                    refs("O", bits(resource.operations, OPS))
                ),
                format!(
                    r#""collectionOperations":{}"#,
                    refs("O", bits(resource.collection, OPS))
                ),
                format!(
                    r#""resources":{}"#,
                    refs("R", bits(resource.children, RESOURCES))
                ),
            ];
            for (name, op) in LIFECYCLE.iter().zip(resource.lifecycle) {
                if let Some(op) = op {
                    fields.push(format!(r#""{name}":{{"target":"ex#O{op}"}}"#));
                }
            }
            shapes.push(format!(
                r#""ex#R{r}":{{"type":"resource",{}}}"#,
                fields.join(",")
            ));
        }
        for o in 0..OPS {
            shapes.push(format!(
                r#""ex#O{o}":{{"type":"operation","errors":{}}}"#,
                refs("E", bits(self.op_errors[o], ERRORS))
            ));
        }
        for e in 0..ERRORS {
            shapes.push(format!(
                r#""ex#E{e}":{{"type":"structure","traits":{{"smithy.api#error":"client"}}}}"#
            ));
        }
        format!(r#"{{"smithy":"2.0","shapes":{{{}}}}}"#, shapes.join(","))
    }

    /// Direct containment edges `(parent, child)` using shape IDs.
    fn edges(&self) -> Vec<(String, String)> {
        let mut edges = Vec::new();
        for s in 0..SERVICES {
            for o in bits(self.service_ops[s], OPS) {
                edges.push((format!("ex#S{s}"), format!("ex#O{o}")));
            }
            for r in bits(self.service_resources[s], RESOURCES) {
                edges.push((format!("ex#S{s}"), format!("ex#R{r}")));
            }
        }
        for (r, resource) in self.resources.iter().enumerate() {
            let parent = format!("ex#R{r}");
            let ops = bits(resource.operations, OPS)
                .chain(bits(resource.collection, OPS))
                .chain(resource.lifecycle.iter().flatten().copied());
            for o in ops {
                edges.push((parent.clone(), format!("ex#O{o}")));
            }
            for c in bits(resource.children, RESOURCES) {
                edges.push((parent.clone(), format!("ex#R{c}")));
            }
        }
        edges
    }

    fn closure(&self, start: &str) -> BTreeSet<String> {
        let edges = self.edges();
        let mut seen = BTreeSet::new();
        let mut stack = vec![start.to_owned()];
        while let Some(node) = stack.pop() {
            for (parent, child) in &edges {
                if *parent == node && seen.insert(child.clone()) {
                    stack.push(child.clone());
                }
            }
        }
        seen
    }

    /// Distinct direct parents of `entity` within the closure of `service`.
    fn parents(&self, service: &str, entity: &str) -> BTreeSet<String> {
        let mut scope = self.closure(service);
        scope.insert(service.to_owned());
        self.edges()
            .into_iter()
            .filter(|(parent, child)| child == entity && scope.contains(parent))
            .map(|(parent, _)| parent)
            .collect()
    }
}

/// Parses a generated shape ID.
fn id(value: &str) -> ShapeId {
    ShapeId::new(value).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn knowledge_matches_oracles(graph in graph()) {
        let model = Model::from_json_str("generated", &graph.json()).unwrap();
        let knowledge = model.knowledge();
        assert_symmetric(&model);
        let entities: Vec<String> = (0..RESOURCES)
            .map(|r| format!("ex#R{r}"))
            .chain((0..OPS).map(|o| format!("ex#O{o}")))
            .collect();

        for start in (0..SERVICES).map(|s| format!("ex#S{s}")).chain((0..RESOURCES).map(|r| format!("ex#R{r}"))) {
            let expected = graph.closure(&start);
            let ops = op_ids(knowledge.top_down().get_contained_operations(&id(&start)).unwrap());
            let resources = ids(knowledge.top_down().get_contained_resources(&id(&start)).unwrap(), |r| r.id());
            let expected_ops: Vec<_> = expected.iter().filter(|id| id.starts_with("ex#O")).cloned().collect();
            let expected_resources: Vec<_> = expected.iter().filter(|id| id.starts_with("ex#R")).cloned().collect();
            prop_assert_eq!(ops, expected_ops);
            prop_assert_eq!(resources, expected_resources);
        }

        for s in 0..SERVICES {
            let service = format!("ex#S{s}");
            let closure = graph.closure(&service);
            for entity in &entities {
                let bottom_up = knowledge.bottom_up();
                if !closure.contains(entity) {
                    prop_assert!(bottom_up.get_direct_bindings(&id(&service), &id(entity)).is_none());
                    prop_assert!(bottom_up.parent_chain(&id(&service), &id(entity)).unwrap().is_none());
                    continue;
                }
                let parents = graph.parents(&service, entity);
                let actual: BTreeSet<_> = bottom_up
                    .get_direct_bindings(&id(&service), &id(entity))
                    .unwrap()
                    .map(|b| b.parent().id().to_string())
                    .collect();
                prop_assert_eq!(&actual, &parents);
                match bottom_up.unique_parent(&id(&service), &id(entity)) {
                    Ok(parent) => {
                        prop_assert_eq!(parents.len(), 1);
                        prop_assert_eq!(parent.unwrap().id().as_str(), parents.iter().next().unwrap());
                    }
                    Err(err) => {
                        prop_assert!(parents.len() > 1);
                        prop_assert_eq!(ids(err.candidates().iter(), |c| *c), parents.iter().cloned().collect::<Vec<_>>());
                    }
                }
                // Ancestors: every entity with a path down to `entity` within the service.
                let mut expected_ancestors = BTreeSet::new();
                let mut stack = vec![entity.clone()];
                while let Some(node) = stack.pop() {
                    for parent in graph.parents(&service, &node) {
                        if expected_ancestors.insert(parent.clone()) {
                            stack.push(parent);
                        }
                    }
                }
                let ancestors = ids(bottom_up.get_ancestors(&id(&service), &id(entity)).unwrap(), |e| e.id());
                prop_assert_eq!(ancestors, expected_ancestors.iter().cloned().collect::<Vec<_>>());
                // A chain exists exactly when every step is unique, and it is a valid path.
                if let Ok(Some(chain)) = bottom_up.parent_chain(&id(&service), &id(entity)) {
                    let chain = chain_ids(&chain);
                    prop_assert_eq!(chain.last().unwrap(), &service);
                    let mut child = entity.clone();
                    for parent in &chain {
                        prop_assert_eq!(graph.parents(&service, &child), BTreeSet::from([parent.clone()]));
                        child = parent.clone();
                    }
                }
            }

            for o in 0..OPS {
                let op = format!("ex#O{o}");
                let errors = knowledge.operations().get(&id(&op)).unwrap().errors_in(&id(&service));
                if !closure.contains(&op) {
                    prop_assert!(errors.is_none());
                    continue;
                }
                let mut expected: Vec<String> = Vec::new();
                let mut service_errors: Vec<_> = bits(graph.service_errors[s], ERRORS).collect();
                service_errors.reverse();
                for e in service_errors.into_iter().chain(bits(graph.op_errors[o], ERRORS)) {
                    let id = format!("ex#E{e}");
                    if !expected.contains(&id) {
                        expected.push(id);
                    }
                }
                prop_assert_eq!(ids(errors.unwrap(), |e| e.id()), expected);
            }
        }
    }
}

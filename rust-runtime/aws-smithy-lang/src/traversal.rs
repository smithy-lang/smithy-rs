/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shape relationships and forward traversal.
//!
//! Every ordinary reference in a [`Model`](crate::Model) resolves, so each [`Relationship`] always has a
//! target. Applied traits are not relationships: trait definitions may be absent.
//!
//! [`Walker`] is the general-purpose graph primitive. For standard Smithy questions, such as
//! every operation in a service, prefer the [`knowledge`](crate::knowledge) indexes: a custom
//! walker predicate can easily omit a lifecycle or collection binding.

use crate::knowledge::{RelationshipIndex, Relationships};
use crate::model::ModelData;
use crate::shape::{MemberView, ShapeRef, ShapeView};
use crate::shape_id::ShapeId;
use std::collections::HashSet;
use std::fmt;
use std::marker::PhantomData;

/// The kind of a [`Relationship`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipType {
    /// A service or resource to an operation in its `operations`.
    Operation,
    /// A resource to an operation in its `collectionOperations`.
    CollectionOperation,
    /// A service or resource to a resource in its `resources`.
    Resource,
    /// A service or operation to an error structure in its `errors`.
    Error,
    /// A resource to its `create` operation.
    Create,
    /// A resource to its `put` operation.
    Put,
    /// A resource to its `read` operation.
    Read,
    /// A resource to its `update` operation.
    Update,
    /// A resource to its `delete` operation.
    Delete,
    /// A resource to its `list` operation.
    List,
    /// A resource to the target of one of its `identifiers`.
    Identifier,
    /// A resource to the target of one of its `properties`.
    Property,
    /// An operation to its input structure (`smithy.api#Unit` when omitted).
    Input,
    /// An operation to its output structure (`smithy.api#Unit` when omitted).
    Output,
    /// A list, map, structure, union, enum, or intEnum to one of its members.
    Member,
    /// A member to the shape that contains it.
    MemberContainer,
    /// A member to its target shape.
    MemberTarget,
}

impl RelationshipType {
    /// Returns `true` for edges that bind an operation to a service or resource: `Operation`,
    /// `CollectionOperation`, and the `Create`, `Put`, `Read`, `Update`, `Delete`, and `List`
    /// lifecycle bindings.
    pub fn is_operation_binding(self) -> bool {
        use RelationshipType::*;
        matches!(
            self,
            Operation | CollectionOperation | Create | Put | Read | Update | Delete | List
        )
    }

    /// Returns `true` for edges that define service and resource containment: every
    /// [operation binding](Self::is_operation_binding), plus `Resource`.
    ///
    /// These are exactly the edges [`TopDownIndex`](crate::knowledge::TopDownIndex) follows.
    pub fn is_containment(self) -> bool {
        self == RelationshipType::Resource || self.is_operation_binding()
    }

    /// A stable rank used to order edges that share a source.
    pub(crate) fn rank(self) -> u8 {
        self as u8
    }
}

/// A directed edge from one shape or member to another.
#[derive(Clone, Copy)]
pub struct Relationship<'a> {
    source: ShapeRef<'a>,
    kind: RelationshipType,
    target: ShapeRef<'a>,
}

impl<'a> Relationship<'a> {
    pub(crate) fn new(source: ShapeRef<'a>, kind: RelationshipType, target: ShapeRef<'a>) -> Self {
        Self {
            source,
            kind,
            target,
        }
    }

    /// The shape or member the edge starts from.
    pub fn source(&self) -> ShapeRef<'a> {
        self.source
    }

    /// The kind of edge.
    pub fn kind(&self) -> RelationshipType {
        self.kind
    }

    /// The ID of the target.
    pub fn target_id(&self) -> &'a ShapeId {
        self.target.id()
    }

    /// The target shape or member. Always resolved.
    pub fn target(&self) -> ShapeRef<'a> {
        self.target
    }
}

impl fmt::Debug for Relationship<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -[{:?}]-> {}",
            self.source.id(),
            self.kind,
            self.target.id()
        )
    }
}

fn root<'a>(data: &'a ModelData, id: &ShapeId) -> ShapeRef<'a> {
    let decl = data
        .root(id.as_str())
        .expect("validated models only contain resolved references");
    ShapeRef::Shape(ShapeView::new(data, decl))
}

/// Outgoing relationships of `shape`, in a deterministic order following the JSON AST.
pub(crate) fn relationships<'a>(shape: ShapeRef<'a>) -> Vec<Relationship<'a>> {
    let mut out = Vec::new();
    let mut push = |kind, target| {
        out.push(Relationship {
            source: shape,
            kind,
            target,
        })
    };
    let view = match shape {
        ShapeRef::Member(member) => {
            push(RelationshipType::MemberContainer, member.container().into());
            push(RelationshipType::MemberTarget, member.target_shape().into());
            return out;
        }
        ShapeRef::Shape(view) => view,
    };
    let data = view.data;
    let mut push_all = |kind, ids: &mut dyn Iterator<Item = &'a ShapeId>| {
        for id in ids {
            push(kind, root(data, id));
        }
    };
    use RelationshipType::*;
    if let Some(service) = view.as_service() {
        push_all(Operation, &mut service.direct_operations());
        push_all(Resource, &mut service.direct_resources());
        push_all(Error, &mut service.common_errors());
    } else if let Some(operation) = view.as_operation() {
        push_all(Input, &mut std::iter::once(operation.input()));
        push_all(Output, &mut std::iter::once(operation.output()));
        push_all(Error, &mut operation.direct_errors());
    } else if let Some(resource) = view.as_resource() {
        push_all(Identifier, &mut resource.identifiers().map(|(_, id)| id));
        push_all(Property, &mut resource.properties().map(|(_, id)| id));
        for (kind, id) in [
            (Create, resource.create()),
            (Put, resource.put()),
            (Read, resource.read()),
            (Update, resource.update()),
            (Delete, resource.delete()),
            (List, resource.list()),
        ] {
            push_all(kind, &mut id.into_iter());
        }
        push_all(Operation, &mut resource.direct_operations());
        push_all(
            CollectionOperation,
            &mut resource.direct_collection_operations(),
        );
        push_all(Resource, &mut resource.child_resources());
    }
    for member in view.members().iter() {
        push(Member, ShapeRef::Member(member));
    }
    out
}

impl<'a> ShapeRef<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    ///
    /// Served from the model's cached [`RelationshipIndex`].
    pub fn relationships(&self) -> Relationships<'a> {
        RelationshipIndex::new(self.data()).outgoing_of(*self)
    }

    /// Incoming relationships: every edge whose target is this shape or member, sorted by
    /// source ID and then by relationship kind. Each edge keeps its original direction.
    ///
    /// Served from the model's cached [`RelationshipIndex`].
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        RelationshipIndex::new(self.data()).incoming_of(*self)
    }
}

impl<'a> ShapeView<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    pub fn relationships(&self) -> Relationships<'a> {
        ShapeRef::Shape(*self).relationships()
    }

    /// Incoming relationships, sorted by source ID and then by relationship kind.
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        ShapeRef::Shape(*self).incoming_relationships()
    }
}

impl<'a> MemberView<'a> {
    /// Outgoing relationships: the container, then the target.
    pub fn relationships(&self) -> Relationships<'a> {
        ShapeRef::Member(*self).relationships()
    }

    /// Incoming relationships: the container's `Member` edge to this member.
    pub fn incoming_relationships(&self) -> Relationships<'a> {
        ShapeRef::Member(*self).incoming_relationships()
    }
}

/// Forward depth-first traversal over unique reachable shapes and members.
///
/// The walk includes the starting shape and follows every relationship accepted by the
/// predicate (all of them by default). It uses an explicit stack and a visited set, so
/// recursive shapes and deep models cannot overflow the call stack.
///
/// ```
/// use aws_smithy_lang::traversal::{RelationshipType, Walker};
/// use aws_smithy_lang::Model;
///
/// let model = Model::from_json_str("m", r#"{"smithy": "2.0", "shapes": {
///     "ex#Service": {"type": "service", "operations": [{"target": "ex#Ping"}]},
///     "ex#Ping": {"type": "operation", "input": {"target": "ex#PingInput"}},
///     "ex#PingInput": {"type": "structure", "members": {"s": {"target": "smithy.api#String"}}}
/// }}"#)?;
/// let service = model.expect_shape("ex#Service")?;
/// let closure: Vec<_> = Walker::new()
///     .filter(|rel| rel.kind() != RelationshipType::Output)
///     .walk(service)
///     .filter(|shape| !shape.is_member())
///     .map(|shape| shape.id().to_string())
///     .collect();
/// assert_eq!(closure, ["ex#Service", "ex#Ping", "ex#PingInput", "smithy.api#String"]);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
pub struct Walker<'m, F = fn(&Relationship<'m>) -> bool> {
    predicate: F,
    _shapes: PhantomData<ShapeRef<'m>>,
}

impl<'m> Walker<'m> {
    /// A walker that follows every relationship.
    pub fn new() -> Self {
        Self {
            predicate: |_| true,
            _shapes: PhantomData,
        }
    }
}

impl Default for Walker<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'m, F: Fn(&Relationship<'m>) -> bool> Walker<'m, F> {
    /// Only follows relationships for which `predicate` returns `true`.
    pub fn filter<G: Fn(&Relationship<'m>) -> bool>(self, predicate: G) -> Walker<'m, G> {
        Walker {
            predicate,
            _shapes: PhantomData,
        }
    }

    /// Iterates shapes reachable from `start` in depth-first preorder, starting with `start`.
    ///
    /// Traversal stays within `start`'s model.
    pub fn walk(&self, start: impl Into<ShapeRef<'m>>) -> Walk<'m, '_, F> {
        Walk {
            predicate: &self.predicate,
            stack: vec![start.into()],
            visited: HashSet::new(),
        }
    }
}

impl<F> fmt::Debug for Walker<'_, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Walker").finish_non_exhaustive()
    }
}

/// The iterator returned by [`Walker::walk`].
pub struct Walk<'m, 'w, F> {
    predicate: &'w F,
    stack: Vec<ShapeRef<'m>>,
    visited: HashSet<&'m ShapeId>,
}

impl<'m, F: Fn(&Relationship<'m>) -> bool> Iterator for Walk<'m, '_, F> {
    type Item = ShapeRef<'m>;

    fn next(&mut self) -> Option<ShapeRef<'m>> {
        while let Some(shape) = self.stack.pop() {
            if !self.visited.insert(shape.id()) {
                continue;
            }
            let neighbors = relationships(shape);
            // Push in reverse so the first relationship is visited first.
            for relationship in neighbors.iter().rev() {
                if !self.visited.contains(relationship.target_id())
                    && (self.predicate)(relationship)
                {
                    self.stack.push(relationship.target());
                }
            }
            return Some(shape);
        }
        None
    }
}

impl<F> fmt::Debug for Walk<'_, '_, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Walk")
            .field("pending", &self.stack.len())
            .field("visited", &self.visited.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use RelationshipType::*;

    const MODEL: &str = r#"{"smithy":"2.0","shapes":{
        "a#Svc":{"type":"service","operations":[{"target":"a#Op"}],"resources":[{"target":"a#R"}],
                 "errors":[{"target":"a#Err"}]},
        "a#Op":{"type":"operation","input":{"target":"a#In"},"errors":[{"target":"a#Err"}]},
        "a#In":{"type":"structure","members":{"next":{"target":"a#In"},"tags":{"target":"a#Tags"}}},
        "a#Tags":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"a#List"}},
        "a#List":{"type":"list","member":{"target":"smithy.api#Integer"}},
        "a#Err":{"type":"structure","traits":{"smithy.api#error":"client"}},
        "a#R":{"type":"resource","identifiers":{"id":{"target":"smithy.api#String"}},
               "properties":{"p":{"target":"a#List"}},"create":{"target":"a#Op"},"put":{"target":"a#Op"},
               "read":{"target":"a#Op"},"update":{"target":"a#Op"},"delete":{"target":"a#Op"},
               "list":{"target":"a#Op"},"operations":[{"target":"a#Op"}],
               "collectionOperations":[{"target":"a#Op"}],"resources":[{"target":"a#Child"}]},
        "a#Child":{"type":"resource","resources":[{"target":"a#Grandchild"}]},
        "a#Grandchild":{"type":"resource"},
        "a#Unused":{"type":"string"}
    }}"#;

    fn model() -> Model {
        Model::from_json_str("m", MODEL).unwrap()
    }

    fn edges(model: &Model, id: &str) -> Vec<(RelationshipType, String)> {
        model
            .expect_shape(id)
            .unwrap()
            .relationships()
            .map(|r| (r.kind(), r.target_id().to_string()))
            .collect()
    }

    fn e(kind: RelationshipType, id: &str) -> (RelationshipType, String) {
        (kind, id.to_owned())
    }

    #[test]
    fn relationships_per_kind() {
        let model = model();
        assert_eq!(
            edges(&model, "a#Svc"),
            [e(Operation, "a#Op"), e(Resource, "a#R"), e(Error, "a#Err")]
        );
        assert_eq!(
            edges(&model, "a#Op"),
            [
                e(Input, "a#In"),
                e(Output, "smithy.api#Unit"),
                e(Error, "a#Err")
            ]
        );
        assert_eq!(
            edges(&model, "a#R"),
            [
                e(Identifier, "smithy.api#String"),
                e(Property, "a#List"),
                e(Create, "a#Op"),
                e(Put, "a#Op"),
                e(Read, "a#Op"),
                e(Update, "a#Op"),
                e(Delete, "a#Op"),
                e(List, "a#Op"),
                e(Operation, "a#Op"),
                e(CollectionOperation, "a#Op"),
                e(Resource, "a#Child"),
            ]
        );
        assert_eq!(
            edges(&model, "a#Tags"),
            [e(Member, "a#Tags$key"), e(Member, "a#Tags$value")]
        );
        assert_eq!(
            edges(&model, "a#Tags$value"),
            [e(MemberContainer, "a#Tags"), e(MemberTarget, "a#List")]
        );
        assert_eq!(edges(&model, "a#Unused"), []);
        let relationship = model
            .expect_shape("a#Svc")
            .unwrap()
            .relationships()
            .next()
            .unwrap();
        assert_eq!(relationship.source().id(), "a#Svc");
        assert!(relationship.target().is_operation());
        assert_eq!(format!("{relationship:?}"), "a#Svc -[Operation]-> a#Op");
    }

    fn walk_ids<'m, F: Fn(&Relationship<'m>) -> bool>(
        model: &'m Model,
        walker: &Walker<'m, F>,
        start: &str,
    ) -> Vec<String> {
        walker
            .walk(model.expect_shape(start).unwrap())
            .map(|s| s.id().to_string())
            .collect()
    }

    #[test]
    fn walker_preorder_and_cycles() {
        let model = model();
        let walker = Walker::new();
        // `a#In$next` targets its own container: the cycle is visited once.
        assert_eq!(
            walk_ids(&model, &walker, "a#In"),
            [
                "a#In",
                "a#In$next",
                "a#In$tags",
                "a#Tags",
                "a#Tags$key",
                "smithy.api#String",
                "a#Tags$value",
                "a#List",
                "a#List$member",
                "smithy.api#Integer",
            ]
        );
        let closure = walk_ids(&model, &walker, "a#Svc");
        let unique: HashSet<_> = closure.iter().collect();
        assert_eq!(unique.len(), closure.len());
        assert_eq!(closure[0], "a#Svc");
        for id in [
            "a#Op",
            "a#Err",
            "a#Grandchild",
            "smithy.api#Unit",
            "a#List$member",
        ] {
            assert!(closure.iter().any(|c| c == id), "{id}");
        }
        assert!(!closure.iter().any(|c| c == "a#Unused"));
        // Walking from a member reaches its container.
        assert_eq!(walk_ids(&model, &walker, "a#List$member")[1], "a#List");
    }

    #[test]
    fn walker_predicate() {
        let model = model();
        let services_and_resources = Walker::new().filter(|r| matches!(r.kind(), Resource));
        assert_eq!(
            walk_ids(&model, &services_and_resources, "a#Svc"),
            ["a#Svc", "a#R", "a#Child", "a#Grandchild"]
        );
        let no_errors = Walker::new().filter(|r| r.kind() != Error);
        assert!(!walk_ids(&model, &no_errors, "a#Svc").contains(&"a#Err".to_owned()));
    }

    #[test]
    fn deep_chain_does_not_overflow() {
        let mut shapes = Vec::new();
        let depth = 20_000;
        for i in 0..depth {
            shapes.push(format!(
                r#""a#S{i}":{{"type":"structure","members":{{"m":{{"target":"a#S{}"}}}}}}"#,
                (i + 1) % depth
            ));
        }
        let json = format!(r#"{{"smithy":"2.0","shapes":{{{}}}}}"#, shapes.join(","));
        let model = Model::from_json_str("deep", &json).unwrap();
        let count = Walker::new()
            .walk(model.expect_shape("a#S0").unwrap())
            .count();
        assert_eq!(count, depth * 2);
    }

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync>(_: &T) {}
        let model = model();
        let walker = Walker::new();
        assert(&walker);
        assert(&walker.walk(model.expect_shape("a#Svc").unwrap()));
        assert(
            &model
                .expect_shape("a#Svc")
                .unwrap()
                .relationships()
                .next()
                .unwrap(),
        );
    }
}

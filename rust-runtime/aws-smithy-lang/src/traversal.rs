/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Outgoing shape relationships and forward traversal.
//!
//! Every ordinary reference in a [`Model`] resolves, so each [`Relationship`] always has a
//! target. Applied traits are not relationships: trait definitions may be absent.

use crate::model::{Model, ModelData};
use crate::shape::{MemberView, ShapeRef, ShapeView};
use crate::shape_id::ShapeId;
use std::collections::HashSet;
use std::fmt;

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

/// A directed edge from one shape or member to another.
#[derive(Clone, Copy)]
pub struct Relationship<'a> {
    source: ShapeRef<'a>,
    kind: RelationshipType,
    target: ShapeRef<'a>,
}

impl<'a> Relationship<'a> {
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
        push_all(Operation, &mut service.operations());
        push_all(Resource, &mut service.resources());
        push_all(Error, &mut service.errors());
    } else if let Some(operation) = view.as_operation() {
        push_all(Input, &mut std::iter::once(operation.input()));
        push_all(Output, &mut std::iter::once(operation.output()));
        push_all(Error, &mut operation.errors());
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
        push_all(Operation, &mut resource.operations());
        push_all(CollectionOperation, &mut resource.collection_operations());
        push_all(Resource, &mut resource.resources());
    }
    for member in view.members().iter() {
        push(Member, ShapeRef::Member(member));
    }
    out
}

impl<'a> ShapeRef<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    pub fn relationships(&self) -> impl ExactSizeIterator<Item = Relationship<'a>> + 'a {
        relationships(*self).into_iter()
    }
}

impl<'a> ShapeView<'a> {
    /// Outgoing relationships, in a deterministic order following the JSON AST.
    pub fn relationships(&self) -> impl ExactSizeIterator<Item = Relationship<'a>> + 'a {
        ShapeRef::Shape(*self).relationships()
    }
}

impl<'a> MemberView<'a> {
    /// Outgoing relationships: the container, then the target.
    pub fn relationships(&self) -> impl ExactSizeIterator<Item = Relationship<'a>> + 'a {
        ShapeRef::Member(*self).relationships()
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
/// let closure: Vec<_> = Walker::new(&model)
///     .filter(|rel| rel.kind() != RelationshipType::Output)
///     .walk(service)
///     .filter(|shape| !shape.is_member())
///     .map(|shape| shape.id().to_string())
///     .collect();
/// assert_eq!(closure, ["ex#Service", "ex#Ping", "ex#PingInput", "smithy.api#String"]);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
pub struct Walker<'m, F = fn(&Relationship<'m>) -> bool> {
    _model: &'m Model,
    predicate: F,
}

impl<'m> Walker<'m> {
    /// A walker that follows every relationship.
    pub fn new(model: &'m Model) -> Self {
        Self {
            _model: model,
            predicate: |_| true,
        }
    }
}

impl<'m, F: Fn(&Relationship<'m>) -> bool> Walker<'m, F> {
    /// Only follows relationships for which `predicate` returns `true`.
    pub fn filter<G: Fn(&Relationship<'m>) -> bool>(self, predicate: G) -> Walker<'m, G> {
        Walker {
            _model: self._model,
            predicate,
        }
    }

    /// Iterates shapes reachable from `start` in depth-first preorder, starting with `start`.
    ///
    /// `start` must come from the same model as this walker.
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
        let walker = Walker::new(&model);
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
        let services_and_resources = Walker::new(&model).filter(|r| matches!(r.kind(), Resource));
        assert_eq!(
            walk_ids(&model, &services_and_resources, "a#Svc"),
            ["a#Svc", "a#R", "a#Child", "a#Grandchild"]
        );
        let no_errors = Walker::new(&model).filter(|r| r.kind() != Error);
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
        let count = Walker::new(&model)
            .walk(model.expect_shape("a#S0").unwrap())
            .count();
        assert_eq!(count, depth * 2);
    }

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync>(_: &T) {}
        let model = model();
        let walker = Walker::new(&model);
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

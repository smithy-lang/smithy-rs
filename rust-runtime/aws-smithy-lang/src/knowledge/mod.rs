/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Derived, cached knowledge about a [`Model`].
//!
//! Many common questions cannot be answered correctly from a shape's declared fields alone. A
//! service's declared `operations`, for example, omit every operation bound through a
//! resource. The indexes in this module are the single implementation of those rules:
//!
//! | Index | Answers |
//! | ----- | ------- |
//! | [`TopDownIndex`] | Every operation and resource contained in a service or resource. |
//! | [`OperationIndex`] | Normalized input/output, effective errors within a service, and which operations bind a structure. |
//! | [`BottomUpIndex`] | The parents of an operation or resource within one service, reporting ambiguity instead of guessing. |
//! | [`PaginatedIndex`] | The effective pagination of an operation within one service, resolved to the input and output members it names. |
//! | [`RelationshipIndex`](crate::relationships::RelationshipIndex) | Outgoing and incoming relationships of any shape or member. |
//!
//! Acquire an index with [`Model::knowledge`](crate::Model::knowledge) by naming its type, as
//! in `model.knowledge::<TopDownIndex<'_>>()`. Every built-in index handle is a cheap, `Copy`
//! borrow of the model. The data behind them is computed on first use and shared by every clone
//! of the model and by every thread; a model that never asks for knowledge pays nothing.
//!
//! ```
//! use aws_smithy_lang::knowledge::{BottomUpIndex, OperationIndex, TopDownIndex};
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! const SERVICE: ShapeId = ShapeId::from_static("ex#Service");
//! const GET_THING: ShapeId = ShapeId::from_static("ex#GetThing");
//!
//! let model = Model::from_json_str("m", r#"{"smithy": "2.0", "shapes": {
//!     "ex#Service": {"type": "service", "resources": [{"target": "ex#Thing"}],
//!                    "errors": [{"target": "ex#Throttled"}]},
//!     "ex#Thing": {"type": "resource", "read": {"target": "ex#GetThing"}},
//!     "ex#GetThing": {"type": "operation", "errors": [{"target": "ex#NotFound"}]},
//!     "ex#Throttled": {"type": "structure", "traits": {"smithy.api#error": "client"}},
//!     "ex#NotFound": {"type": "structure", "traits": {"smithy.api#error": "client"}}
//! }}"#)?;
//!
//! // The operation is reached only through a resource.
//! let operations: Vec<_> = model
//!     .knowledge::<TopDownIndex<'_>>()
//!     .get_contained_operations(&SERVICE)
//!     .unwrap()
//!     .map(|op| op.id().to_string())
//!     .collect();
//! assert_eq!(operations, ["ex#GetThing"]);
//!
//! // Errors within the service: common service errors first.
//! let operations = model.knowledge::<OperationIndex<'_>>();
//! let operation = operations.get(&GET_THING).unwrap();
//! let errors: Vec<_> = operation
//!     .errors_in(&SERVICE)
//!     .unwrap()
//!     .map(|e| e.id().to_string())
//!     .collect();
//! assert_eq!(errors, ["ex#Throttled", "ex#NotFound"]);
//!
//! // The operation's parent chain within the service.
//! let bottom_up = model.knowledge::<BottomUpIndex<'_>>();
//! let chain = bottom_up.parent_chain(&SERVICE, &GET_THING)?.unwrap();
//! let chain: Vec<_> = chain.iter().map(|e| e.id().to_string()).collect();
//! assert_eq!(chain, ["ex#Thing", "ex#Service"]);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! Use [`Walker`](crate::traversal::Walker) when you intentionally need a custom traversal
//! over data shapes, members, or a hand-picked set of relationship kinds.
//!
//! # Direct versus contained
//!
//! Service and resource views have no unqualified `operations()` or `resources()` method, and
//! operation views have no unqualified `errors()`. Callers choose between a complete query,
//! such as [`ServiceView::contained_operations`](crate::shape::ServiceView::contained_operations),
//! and an explicitly *declaration-only* accessor, such as
//! [`ServiceView::direct_operations`](crate::shape::ServiceView::direct_operations). Code
//! written against the ambiguous name does not compile:
//!
//! ```compile_fail
//! # let model = aws_smithy_lang::Model::from_json_str("m", r#"{"smithy": "2.0", "shapes": {}}"#).unwrap();
//! for service in model.services() {
//!     for operation in service.operations() {}
//! }
//! ```
//!
//! Applied traits are not relationships: custom trait definitions may be absent from a model.
//! The loader also preserves trait values without validating them. An index that interprets
//! trait values, such as [`PaginatedIndex`], therefore decodes them when queried and returns an
//! error for a value it cannot interpret, rather than treating that value as absent.
//!
//! # Defining an index
//!
//! Any type that implements [`KnowledgeIndex`] can be acquired with
//! [`Model::knowledge`](crate::Model::knowledge), including a type defined in another crate.
//! There is nothing to register: naming the type selects its constructor. An index can build on
//! other indexes by acquiring them from the model, and can return [`Iter`] as they do:
//!
//! ```
//! use aws_smithy_lang::knowledge::{Iter, KnowledgeIndex, TopDownIndex};
//! use aws_smithy_lang::shape::OperationView;
//! use aws_smithy_lang::{Model, ShapeId};
//!
//! /// The `@readonly` operations of a service or resource.
//! struct ReadonlyIndex<'m> {
//!     top_down: TopDownIndex<'m>,
//! }
//!
//! impl<'m> KnowledgeIndex<'m> for ReadonlyIndex<'m> {
//!     fn from_model(model: &'m Model) -> Self {
//!         Self { top_down: model.knowledge::<TopDownIndex<'_>>() }
//!     }
//! }
//!
//! impl<'m> ReadonlyIndex<'m> {
//!     fn operations(&self, entity: &ShapeId) -> Option<Iter<OperationView<'m>>> {
//!         const READONLY: ShapeId = ShapeId::from_static("smithy.api#readonly");
//!         let operations = self.top_down.get_contained_operations(entity)?;
//!         Some(operations.filter(|op| op.has_trait(&READONLY)).collect())
//!     }
//! }
//!
//! let model = Model::from_idl_str("things.smithy", r#"
//! $version: "2"
//! namespace ex
//!
//! service Things { resources: [Thing] }
//! resource Thing { read: GetThing, delete: DeleteThing }
//!
//! @readonly
//! operation GetThing {}
//!
//! @idempotent
//! operation DeleteThing {}
//! "#)?;
//!
//! let readonly = model.knowledge::<ReadonlyIndex<'_>>();
//! let operations: Vec<_> = readonly
//!     .operations(&ShapeId::from_static("ex#Things"))
//!     .unwrap()
//!     .map(|op| op.id().to_string())
//!     .collect();
//! assert_eq!(operations, ["ex#GetThing"]);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```

mod bottom_up;
mod operations;
mod paginated;
mod top_down;

pub use bottom_up::{AmbiguousParentError, BottomUpIndex, EntityView, ParentBinding, ParentChain};
pub use operations::{OperationIndex, OperationKnowledge};
pub use paginated::{PaginatedIndex, PaginationIndexError, PaginationInfo};
pub use top_down::TopDownIndex;

use crate::model::{Model, ModelData};
use crate::relationships::RelationshipData;
use std::fmt;
use std::iter::FusedIterator;
use std::sync::OnceLock;

/// Lazily initialized knowledge storage owned by a model.
#[derive(Default)]
pub(crate) struct KnowledgeCache {
    relationships: OnceLock<RelationshipData>,
}

impl KnowledgeCache {
    pub(crate) fn relationships<'m>(&'m self, data: &ModelData) -> &'m RelationshipData {
        self.relationships
            .get_or_init(|| RelationshipData::build(data))
    }
}

/// An index of knowledge derived from a model, acquired with
/// [`Model::knowledge`](crate::Model::knowledge).
///
/// Implement this trait to make a type available as `model.knowledge::<MyIndex<'_>>()`. The
/// type may borrow the model for `'m` so its queries can return views such as
/// [`ShapeView<'m>`](crate::shape::ShapeView). See
/// [Defining an index](crate::knowledge#defining-an-index) for an example.
///
/// `from_model` runs on every acquisition and its result is not cached. The built-in indexes are
/// cheap handles over data that the model computes once and shares. An index that computes and
/// owns its data recomputes it each time it is acquired, so callers should keep the handle
/// instead of acquiring it repeatedly.
///
/// `from_model` must not fail. An index whose answers can be invalid, such as
/// [`PaginatedIndex`], returns errors from its query methods instead. It may acquire other
/// indexes, but those dependencies must not form a cycle.
pub trait KnowledgeIndex<'m>: Sized {
    /// Builds this index for `model`.
    fn from_model(model: &'m Model) -> Self;
}

/// An iterator over the results of a knowledge query.
///
/// An index defined in another crate can return the same type: build one from a `Vec` with
/// [`From`], or by collecting an iterator.
///
/// ```
/// use aws_smithy_lang::knowledge::Iter;
///
/// let sorted: Iter<u32> = vec![1, 2, 3].into();
/// assert_eq!(sorted.len(), 3);
/// let even: Iter<u32> = (1..=6).filter(|n| n % 2 == 0).collect();
/// assert_eq!(even.collect::<Vec<_>>(), [2, 4, 6]);
/// ```
#[derive(Clone)]
pub struct Iter<T> {
    inner: std::vec::IntoIter<T>,
}

impl<T> Iter<T> {
    fn new(items: Vec<T>) -> Self {
        Self {
            inner: items.into_iter(),
        }
    }
}

impl<T> From<Vec<T>> for Iter<T> {
    fn from(items: Vec<T>) -> Self {
        Self::new(items)
    }
}

impl<T> FromIterator<T> for Iter<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self::new(items.into_iter().collect())
    }
}

impl<T> Iterator for Iter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> DoubleEndedIterator for Iter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.inner.next_back()
    }
}

impl<T> ExactSizeIterator for Iter<T> {}

impl<T> FusedIterator for Iter<T> {}

impl<T: fmt::Debug> fmt::Debug for Iter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.inner.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests;

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Derived, cached knowledge about a [`Model`](crate::Model).
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
//! | [`RelationshipIndex`] | Outgoing and incoming relationships of any shape or member. |
//!
//! Enter through [`Model::knowledge`](crate::Model::knowledge). Every index handle is a cheap,
//! `Copy` borrow of the model. Indexes are computed on first use and shared by every clone of
//! the model and by every thread; a model that never asks for knowledge pays nothing.
//!
//! ```
//! use aws_smithy_lang::Model;
//!
//! let model = Model::from_json_str("m", r#"{"smithy": "2.0", "shapes": {
//!     "ex#Service": {"type": "service", "resources": [{"target": "ex#Thing"}],
//!                    "errors": [{"target": "ex#Throttled"}]},
//!     "ex#Thing": {"type": "resource", "read": {"target": "ex#GetThing"}},
//!     "ex#GetThing": {"type": "operation", "errors": [{"target": "ex#NotFound"}]},
//!     "ex#Throttled": {"type": "structure", "traits": {"smithy.api#error": "client"}},
//!     "ex#NotFound": {"type": "structure", "traits": {"smithy.api#error": "client"}}
//! }}"#)?;
//! let knowledge = model.knowledge();
//!
//! // The operation is reached only through a resource.
//! let operations: Vec<_> = knowledge
//!     .top_down()
//!     .get_contained_operations("ex#Service")
//!     .unwrap()
//!     .map(|op| op.id().to_string())
//!     .collect();
//! assert_eq!(operations, ["ex#GetThing"]);
//!
//! // Errors within the service: common service errors first.
//! let operation = knowledge.operations().get("ex#GetThing").unwrap();
//! let errors: Vec<_> = operation
//!     .errors_in("ex#Service")
//!     .unwrap()
//!     .map(|e| e.id().to_string())
//!     .collect();
//! assert_eq!(errors, ["ex#Throttled", "ex#NotFound"]);
//!
//! // The operation's parent chain within the service.
//! let chain = knowledge.bottom_up().parent_chain("ex#Service", "ex#GetThing")?.unwrap();
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
//! Indexes that interpret trait values (nullability, HTTP bindings, pagination, and so on) are
//! not provided, because the loader preserves trait values without validating them.

mod bottom_up;
mod operations;
mod top_down;

pub use bottom_up::{AmbiguousParentError, BottomUpIndex, EntityView, ParentBinding, ParentChain};
pub use operations::{OperationIndex, OperationKnowledge};
pub use top_down::TopDownIndex;

use crate::model::ModelData;
use crate::relationships::{RelationshipData, RelationshipIndex};
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

/// Entry point to a model's knowledge indexes. Returned by
/// [`Model::knowledge`](crate::Model::knowledge).
#[derive(Clone, Copy)]
pub struct Knowledge<'m> {
    data: &'m ModelData,
}

impl<'m> Knowledge<'m> {
    pub(crate) fn new(data: &'m ModelData) -> Self {
        Self { data }
    }

    /// Outgoing and incoming relationships of every shape and member.
    pub fn relationships(self) -> RelationshipIndex<'m> {
        RelationshipIndex::new(self.data)
    }

    /// Every operation and resource contained in a service or resource.
    pub fn top_down(self) -> TopDownIndex<'m> {
        TopDownIndex::new(self.relationships())
    }

    /// Operation input, output, errors, and reverse bindings.
    pub fn operations(self) -> OperationIndex<'m> {
        OperationIndex::new(self.relationships())
    }

    /// Service-scoped parents of operations and resources.
    pub fn bottom_up(self) -> BottomUpIndex<'m> {
        BottomUpIndex::new(self.relationships())
    }
}

impl fmt::Debug for Knowledge<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Knowledge").finish_non_exhaustive()
    }
}

/// An iterator over the results of a knowledge query.
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

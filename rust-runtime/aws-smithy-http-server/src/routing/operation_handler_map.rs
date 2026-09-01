/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{routing::operation_handler_bindings::OperationHandlerBinding, schema::OperationSchema};

/// Shared operation identity to handler map used by multi-protocol routing.
#[derive(Debug, Clone)]
pub struct OperationHandlerMap<S> {
    routes: Vec<(&'static OperationSchema<'static>, S)>,
}

impl<S> OperationHandlerMap<S> {
    /// Creates a handler map from operation handler bindings.
    pub fn new<I>(bindings: I) -> Self
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        let routes = bindings
            .into_iter()
            .map(|binding| (binding.operation, binding.handler))
            .collect();
        Self { routes }
    }

    /// Returns the handler for an operation.
    pub fn get(&self, operation: &'static OperationSchema<'static>) -> Option<S>
    where
        S: Clone,
    {
        let operation_id = operation.shape_id().as_str();
        self.routes
            .iter()
            .find(|(candidate, _)| candidate.shape_id().as_str() == operation_id)
            .map(|(_, handler)| handler.clone())
    }

    /// Maps every handler through a closure.
    pub fn map<SNew, F>(self, mut f: F) -> OperationHandlerMap<SNew>
    where
        F: FnMut(S) -> SNew,
    {
        OperationHandlerMap {
            routes: self
                .routes
                .into_iter()
                .map(|(operation, handler)| (operation, f(handler)))
                .collect(),
        }
    }
}

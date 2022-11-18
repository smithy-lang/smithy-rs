/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::operation::{Operation, OperationShape};

use super::Plugin;

/// A [`Plugin`] implemented by a closure over the operation name. See [`plugin_from_operation_name_fn`] for more details.
pub struct OperationNameFn<F> {
    f: F,
}

impl<P, Op, S, ExistingLayer, NewLayer, F> Plugin<P, Op, S, ExistingLayer> for OperationNameFn<F>
where
    F: Fn(&'static str) -> NewLayer,
    Op: OperationShape,
{
    type Service = S;
    type Layer = Stack<ExistingLayer, NewLayer>;

    fn map(&self, input: Operation<S, ExistingLayer>) -> Operation<Self::Service, Self::Layer> {
        input.layer((self.f)(Op::NAME))
    }
}

/// Constructs a [`Plugin`] using a closure over the operation name `F: Fn(&'static str) -> L` where `L` is a HTTP
/// [`Layer`](tower::Layer).
pub fn plugin_from_operation_name_fn<F>(f: F) -> OperationNameFn<F> {
    OperationNameFn { f }
}

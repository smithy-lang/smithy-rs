/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::util::Either;

use crate::operation::{Operation, OperationShape};

use super::Plugin;

/// A [`Plugin`] used to filter [`Plugin::map`] application using a predicate over the [`OperationShape::NAME`].
///
/// See [`PluginExt::filter_by_operation_name`](super::PluginExt::filter_by_operation_name) for more information.
pub struct FilterByOperationName<Inner, F> {
    inner: Inner,
    predicate: F,
}

impl<Inner, F> FilterByOperationName<Inner, F> {
    /// Creates a new [`FilterByOperationName`].
    pub(crate) fn new(inner: Inner, predicate: F) -> Self {
        Self { inner, predicate }
    }
}

impl<P, Op, S, L, Inner, F> Plugin<P, Op, S, L> for FilterByOperationName<Inner, F>
where
    F: Fn(&str) -> bool,
    Inner: Plugin<P, Op, S, L>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;
    type Layer = Either<Inner::Layer, L>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        if (self.predicate)(Op::NAME) {
            let Operation { inner, layer } = self.inner.map(input);
            Operation {
                inner: Either::A(inner),
                layer: Either::A(layer),
            }
        } else {
            Operation {
                inner: Either::B(input.inner),
                layer: Either::B(input.layer),
            }
        }
    }
}

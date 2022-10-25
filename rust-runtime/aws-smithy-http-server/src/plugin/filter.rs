/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::util::Either;

use crate::operation::OperationShape;

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

impl<P, Op, ModelLayer, HttpLayer, Inner, F> Plugin<P, Op, ModelLayer, HttpLayer> for FilterByOperationName<Inner, F>
where
    F: Fn(&str) -> bool,
    Inner: Plugin<P, Op, ModelLayer, HttpLayer>,
    Op: OperationShape,
{
    type ModelLayer = Either<Inner::ModelLayer, ModelLayer>;
    type HttpLayer = Either<Inner::HttpLayer, HttpLayer>;

    fn map(&self, model_layer: ModelLayer, http_layer: HttpLayer) -> (Self::ModelLayer, Self::HttpLayer) {
        if (self.predicate)(Op::NAME) {
            let (model_layer, http_layer) = self.inner.map(model_layer, http_layer);
            (Either::A(model_layer), Either::A(http_layer))
        } else {
            (Either::B(model_layer), Either::B(http_layer))
        }
    }
}

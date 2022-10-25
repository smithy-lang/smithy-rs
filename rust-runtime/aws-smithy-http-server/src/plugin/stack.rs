/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::Plugin;

/// A wrapper struct which composes an `Inner` and an `Outer` [`Plugin`].
///
/// The `Inner::map` is run _then_ the `Outer::map`.
pub struct PluginStack<Inner, Outer> {
    inner: Inner,
    outer: Outer,
}

impl<Inner, Outer> PluginStack<Inner, Outer> {
    /// Creates a new [`PluginStack`].
    pub fn new(inner: Inner, outer: Outer) -> Self {
        PluginStack { inner, outer }
    }
}

impl<P, Op, ModelLayer, HttpLayer, Inner, Outer> Plugin<P, Op, ModelLayer, HttpLayer> for PluginStack<Inner, Outer>
where
    Inner: Plugin<P, Op, ModelLayer, HttpLayer>,
    Outer: Plugin<P, Op, Inner::ModelLayer, Inner::HttpLayer>,
{
    type ModelLayer = Outer::ModelLayer;
    type HttpLayer = Outer::HttpLayer;

    fn map(&self, model_layer: ModelLayer, http_layer: HttpLayer) -> (Self::ModelLayer, Self::HttpLayer) {
        let (model_layer, http_layer) = self.inner.map(model_layer, http_layer);
        self.outer.map(model_layer, http_layer)
    }
}

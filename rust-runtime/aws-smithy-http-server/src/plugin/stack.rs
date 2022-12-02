/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::operation::Operation;

use super::Plugin;

/// A wrapper struct which composes an `Inner` and an `Outer` [`Plugin`].
///
/// The `Inner::map` is run _then_ the `Outer::map`.
///
/// Note that the primary tool for composing plugins is [`PluginPipeline`](crate::plugin::PluginPipeline).
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

impl<P, Op, S, L, Inner, Outer> Plugin<P, Op, S, L> for PluginStack<Inner, Outer>
where
    Inner: Plugin<P, Op, S, L>,
    Outer: Plugin<P, Op, Inner::Service, Inner::Layer>,
{
    type Service = Outer::Service;
    type Layer = Outer::Layer;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        let inner = self.inner.map(input);
        self.outer.map(inner)
    }
}

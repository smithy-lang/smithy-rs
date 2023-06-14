/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

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

impl<Ser, Op, S, Inner, Outer> Plugin<Ser, Op, S> for PluginStack<Inner, Outer>
where
    Inner: Plugin<Ser, Op, S>,
    Outer: Plugin<Ser, Op, Inner::Service>,
{
    type Service = Outer::Service;

    fn apply(&self, svc: S) -> Self::Service {
        let svc = self.inner.apply(svc);
        self.outer.apply(svc)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use tower::Layer;

use super::Plugin;

/// A [`Plugin`] which acts as a [`Layer`] `L`.
pub struct LayerPlugin<L>(pub L);

impl<P, Op, S, L> Plugin<P, Op, S> for LayerPlugin<L>
where
    L: Layer<S>,
{
    type Service = L::Service;

    fn apply(&self, svc: S) -> Self::Service {
        self.0.layer(svc)
    }
}

/// A [`Layer`] which acts as a [`Plugin`] `Pl` for specific protocol `P` and operation `Op`.
pub struct PluginLayer<P, Op, Pl> {
    plugin: Pl,
    _protocol: PhantomData<P>,
    _op: PhantomData<Op>,
}

impl<S, P, Op, Pl> Layer<S> for PluginLayer<P, Op, Pl>
where
    Pl: Plugin<P, Op, S>,
{
    type Service = Pl::Service;

    fn layer(&self, inner: S) -> Self::Service {
        self.plugin.apply(inner)
    }
}

impl<Pl> PluginLayer<(), (), Pl> {
    pub fn new<P, Op>(plugin: Pl) -> PluginLayer<P, Op, Pl> {
        PluginLayer {
            plugin,
            _protocol: PhantomData,
            _op: PhantomData,
        }
    }
}

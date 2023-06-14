/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use tower::Layer;

use super::Plugin;

/// A [`Plugin`] which acts as a [`Layer`] `L`.
pub struct LayerPlugin<L>(pub L);

impl<Ser, Op, S, L> Plugin<Ser, Op, S> for LayerPlugin<L>
where
    L: Layer<S>,
{
    type Service = L::Service;

    fn apply(&self, svc: S) -> Self::Service {
        self.0.layer(svc)
    }
}

/// A [`Layer`] which acts as a [`Plugin`] `Pl` for specific protocol `P` and operation `Op`.
pub struct PluginLayer<Ser, Op, Pl> {
    plugin: Pl,
    _ser: PhantomData<Ser>,
    _op: PhantomData<Op>,
}

impl<S, Ser, Op, Pl> Layer<S> for PluginLayer<Ser, Op, Pl>
where
    Pl: Plugin<Ser, Op, S>,
{
    type Service = Pl::Service;

    fn layer(&self, inner: S) -> Self::Service {
        self.plugin.apply(inner)
    }
}

impl<Pl> PluginLayer<(), (), Pl> {
    pub fn new<Ser, Op>(plugin: Pl) -> PluginLayer<Ser, Op, Pl> {
        PluginLayer {
            plugin,
            _ser: PhantomData,
            _op: PhantomData,
        }
    }
}

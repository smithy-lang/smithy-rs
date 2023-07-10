/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use tower::Layer;

use super::{HttpMarker, ModelMarker, Plugin};

/// A [`Plugin`] which acts as a [`Layer`] `L`.
pub struct LayerPlugin<L>(pub L);

impl<Ser, Op, S, L> Plugin<Ser, Op, S> for LayerPlugin<L>
where
    L: Layer<S>,
{
    type Output = L::Service;

    fn apply(&self, svc: S) -> Self::Output {
        self.0.layer(svc)
    }
}

// Without more information about what the layer `L` does, we can't know whether it's appropriate
// to run this plugin as a HTTP plugin or a model plugin, so we implement both marker traits.

impl<L> HttpMarker for LayerPlugin<L> {}
impl<L> ModelMarker for LayerPlugin<L> {}

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
    type Service = Pl::Output;

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

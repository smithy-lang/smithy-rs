/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::plugin::{PluginPipeline, PluginStack};
use crate::{
    operation::{Operation, OperationShape},
    plugin::Plugin,
};

use super::{layer::InstrumentLayer, sensitivity::Sensitivity};

/// A [`Plugin`] which applies [`InstrumentLayer`] to all operations in the builder.
#[derive(Debug)]
pub struct InstrumentPlugin;

impl<P, Op, S, L> Plugin<P, Op, S, L> for InstrumentPlugin
where
    Op: OperationShape,
    Op: Sensitivity,
{
    type Service = S;
    type Layer = Stack<L, InstrumentLayer<Op::RequestFmt, Op::ResponseFmt>>;

    fn map(&self, operation: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        let layer = InstrumentLayer::new(Op::NAME)
            .request_fmt(Op::request_fmt())
            .response_fmt(Op::response_fmt());
        operation.layer(layer)
    }
}

/// An extension trait for applying [`InstrumentLayer`] to all operations in a service.
pub trait InstrumentExt<CurrentPlugins> {
    /// Applies an [`InstrumentLayer`] to all operations which respects the [@sensitive] trait given on the input and
    /// output models. See [`InstrumentOperation`](super::InstrumentOperation) for more information.
    ///
    /// [@sensitive]: https://awslabs.github.io/smithy/2.0/spec/documentation-traits.html#sensitive-trait
    fn instrument(self) -> PluginPipeline<PluginStack<InstrumentPlugin, CurrentPlugins>>;
}

impl<CurrentPlugins> InstrumentExt<CurrentPlugins> for PluginPipeline<CurrentPlugins> {
    fn instrument(self) -> PluginPipeline<PluginStack<InstrumentPlugin, CurrentPlugins>> {
        self.push(InstrumentPlugin)
    }
}

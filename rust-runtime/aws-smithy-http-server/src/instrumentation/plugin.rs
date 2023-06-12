/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::plugin::{PluginPipeline, PluginStack};
use crate::{operation::OperationShape, plugin::Plugin};

use super::sensitivity::Sensitivity;
use super::InstrumentOperation;

/// A [`Plugin`] which applies [`InstrumentOperation`] to every operation.
#[derive(Debug)]
pub struct InstrumentPlugin;

impl<P, Op, S> Plugin<P, Op, S> for InstrumentPlugin
where
    Op: OperationShape,
    Op: Sensitivity,
{
    type Service = InstrumentOperation<S, Op::RequestFmt, Op::ResponseFmt>;

    fn apply(&self, svc: S) -> Self::Service {
        InstrumentOperation::new(svc, Op::ID)
            .request_fmt(Op::request_fmt())
            .response_fmt(Op::response_fmt())
    }
}

/// An extension trait for applying [`InstrumentPlugin`].
pub trait InstrumentExt<CurrentPlugin> {
    /// Applies an [`InstrumentOperation`] to every operation, respecting the [@sensitive] trait given on the input and
    /// output models. See [`InstrumentOperation`](super::InstrumentOperation) for more information.
    ///
    /// [@sensitive]: https://awslabs.github.io/smithy/2.0/spec/documentation-traits.html#sensitive-trait
    fn instrument(self) -> PluginPipeline<PluginStack<InstrumentPlugin, CurrentPlugin>>;
}

impl<CurrentPlugin> InstrumentExt<CurrentPlugin> for PluginPipeline<CurrentPlugin> {
    fn instrument(self) -> PluginPipeline<PluginStack<InstrumentPlugin, CurrentPlugin>> {
        self.push(InstrumentPlugin)
    }
}

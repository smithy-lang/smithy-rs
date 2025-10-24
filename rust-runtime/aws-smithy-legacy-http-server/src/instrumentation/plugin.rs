/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::plugin::{HttpMarker, HttpPlugins, PluginStack};
use crate::{operation::OperationShape, plugin::Plugin};

use super::sensitivity::Sensitivity;
use super::InstrumentOperation;

/// A [`Plugin`] which applies [`InstrumentOperation`] to every operation.
#[derive(Debug)]
pub struct InstrumentPlugin;

impl<Ser, Op, T> Plugin<Ser, Op, T> for InstrumentPlugin
where
    Op: OperationShape,
    Op: Sensitivity,
{
    type Output = InstrumentOperation<T, Op::RequestFmt, Op::ResponseFmt>;

    fn apply(&self, input: T) -> Self::Output {
        InstrumentOperation::new(input, Op::ID)
            .request_fmt(Op::request_fmt())
            .response_fmt(Op::response_fmt())
    }
}

impl HttpMarker for InstrumentPlugin {}

/// An extension trait for applying [`InstrumentPlugin`].
pub trait InstrumentExt<CurrentPlugin> {
    /// Applies an [`InstrumentOperation`] to every operation, respecting the [@sensitive] trait given on the input and
    /// output models. See [`InstrumentOperation`] for more information.
    ///
    /// [@sensitive]: https://smithy.io/2.0/spec/documentation-traits.html#sensitive-trait
    fn instrument(self) -> HttpPlugins<PluginStack<InstrumentPlugin, CurrentPlugin>>;
}

impl<CurrentPlugin> InstrumentExt<CurrentPlugin> for HttpPlugins<CurrentPlugin> {
    fn instrument(self) -> HttpPlugins<PluginStack<InstrumentPlugin, CurrentPlugin>> {
        self.push(InstrumentPlugin)
    }
}

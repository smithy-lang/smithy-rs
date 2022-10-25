/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::{
    operation::OperationShape,
    plugin::{Pluggable, Plugin},
};

use super::{layer::InstrumentLayer, sensitivity::Sensitivity};

/// An [`Plugin`] which applies [`InstrumentLayer`] to all operations in the builder.
#[derive(Debug)]
pub struct InstrumentPlugin;

impl<P, Op, ModelLayer, HttpLayer> Plugin<P, Op, ModelLayer, HttpLayer> for InstrumentPlugin
where
    Op: OperationShape,
    Op: Sensitivity,
{
    type ModelLayer = ModelLayer;
    type HttpLayer = Stack<HttpLayer, InstrumentLayer<Op::RequestFmt, Op::ResponseFmt>>;

    fn map(&self, model_layer: ModelLayer, http_layer: HttpLayer) -> (Self::ModelLayer, Self::HttpLayer) {
        let layer = InstrumentLayer::new(Op::NAME)
            .request_fmt(Op::request_fmt())
            .response_fmt(Op::response_fmt());
        (model_layer, Stack::new(http_layer, layer))
    }
}

/// An extension trait for applying [`InstrumentLayer`] to all operations.
pub trait InstrumentExt: Pluggable<InstrumentPlugin> {
    /// Applies an [`InstrumentLayer`] to all operations which respects the [@sensitive] trait given on the input and
    /// output models. See [`InstrumentOperation`](super::InstrumentOperation) for more information.
    ///
    /// [@sensitive]: https://awslabs.github.io/smithy/2.0/spec/documentation-traits.html#sensitive-trait
    fn instrument(self) -> Self::Output
    where
        Self: Sized,
    {
        self.apply(InstrumentPlugin)
    }
}

impl<Builder> InstrumentExt for Builder where Builder: Pluggable<InstrumentPlugin> {}

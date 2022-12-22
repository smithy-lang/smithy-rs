/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::operation::Operation;

use super::Plugin;

/// A [`Plugin`] which appends a HTTP [`Layer`](tower::Layer) `L` to the existing `Layer` in [`Operation<S, Layer>`](Operation).
pub struct HttpLayer<L>(pub L);

impl<P, Op, S, ExistingLayer, NewLayer> Plugin<P, Op, S, ExistingLayer> for HttpLayer<NewLayer>
where
    NewLayer: Clone,
{
    type Service = S;
    type Layer = Stack<ExistingLayer, NewLayer>;

    fn map(&self, input: Operation<S, ExistingLayer>) -> Operation<Self::Service, Self::Layer> {
        input.layer(self.0.clone())
    }
}

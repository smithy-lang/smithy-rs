/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::operation::Operation;

use super::Plugin;

/// A [`Plugin`] which appends a HTTP [`Layer`](tower::Layer) `L` to the existing `Layer` in [`Operation<S, Layer>`](Operation).
pub struct HttpLayer<'a, L>(pub &'a L);

impl<'a, P, Op, S, ExistingLayer, NewLayer> Plugin<P, Op, S, ExistingLayer> for HttpLayer<'a, NewLayer> {
    type Service = S;
    type Layer = Stack<ExistingLayer, &'a NewLayer>;

    fn map(&self, input: Operation<S, ExistingLayer>) -> Operation<Self::Service, Self::Layer> {
        input.layer(self.0)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::layer::util::Stack;

use crate::operation::{Operation, OperationShape};

use super::Plugin;

/// An adapter to convert a `Fn(&'static str) -> Layer` closure into a [`Plugin`]. See [`plugin_from_operation_name_fn`] for more details.
pub struct OperationNameFn<F> {
    f: F,
}

impl<P, Op, S, ExistingLayer, NewLayer, F> Plugin<P, Op, S, ExistingLayer> for OperationNameFn<F>
where
    F: Fn(&'static str) -> NewLayer,
    Op: OperationShape,
{
    type Service = S;
    type Layer = Stack<ExistingLayer, NewLayer>;

    fn map(&self, input: Operation<S, ExistingLayer>) -> Operation<Self::Service, Self::Layer> {
        input.layer((self.f)(Op::NAME))
    }
}

/// Constructs a [`Plugin`] using a closure over the operation name `F: Fn(&'static str) -> L` where `L` is a HTTP
/// [`Layer`](tower::Layer).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::plugin_from_operation_name_fn;
/// use tower::layer::layer_fn;
///
/// // A `Service` which prints the operation name before calling `S`.
/// struct PrintService<S> {
///     operation_name: &'static str,
///     inner: S
/// }
///
/// // A `Layer` applying `PrintService`.
/// struct PrintLayer {
///     operation_name: &'static str
/// }
///
/// // Defines a closure taking the operation name to `PrintLayer`.
/// let f = |operation_name| PrintLayer { operation_name };
///
/// // This plugin applies the `PrintService` middleware around every operation.
/// let plugin = plugin_from_operation_name_fn(f);
/// ```
pub fn plugin_from_operation_name_fn<L, F>(f: F) -> OperationNameFn<F>
where
    F: Fn(&'static str) -> L,
{
    OperationNameFn { f }
}

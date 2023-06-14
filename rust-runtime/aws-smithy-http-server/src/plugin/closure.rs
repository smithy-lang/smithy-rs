/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::operation::OperationShape;
use crate::shape_id::ShapeId;

use super::Plugin;

/// An adapter to convert a `Fn(ShapeId) -> Layer` closure into a [`Plugin`]. See [`plugin_from_operation_id_fn`] for more details.
pub struct OperationIdFn<F> {
    f: F,
}

impl<Ser, Op, S, NewService, F> Plugin<Ser, Op, S> for OperationIdFn<F>
where
    F: Fn(ShapeId, S) -> NewService,
    Op: OperationShape,
{
    type Service = NewService;

    fn apply(&self, svc: S) -> Self::Service {
        (self.f)(Op::ID, svc)
    }
}

/// Constructs a [`Plugin`] using a closure over the operation name `F: Fn(ShapeId) -> L` where `L` is a HTTP
/// [`Layer`](tower::Layer).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::plugin_from_operation_id_fn;
/// use aws_smithy_http_server::shape_id::ShapeId;
/// use tower::layer::layer_fn;
///
/// // A `Service` which prints the operation name before calling `S`.
/// struct PrintService<S> {
///     operation_name: ShapeId,
///     inner: S
/// }
///
/// // A `Layer` applying `PrintService`.
/// struct PrintLayer {
///     operation_name: ShapeId
/// }
///
/// // Defines a closure taking the operation name to `PrintLayer`.
/// let f = |operation_name| PrintLayer { operation_name };
///
/// // This plugin applies the `PrintService` middleware around every operation.
/// let plugin = plugin_from_operation_id_fn(f);
/// ```
pub fn plugin_from_operation_id_fn<NewService, F>(f: F) -> OperationIdFn<F>
where
    F: Fn(ShapeId) -> NewService,
{
    OperationIdFn { f }
}

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

impl<Ser, Op, T, NewService, F> Plugin<Ser, Op, T> for OperationIdFn<F>
where
    F: Fn(ShapeId, T) -> NewService,
    Op: OperationShape,
{
    type Output = NewService;

    fn apply(&self, input: T) -> Self::Output {
        (self.f)(Op::ID, input)
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
///     operation_id: ShapeId,
///     inner: S
/// }
///
/// impl<S> PrintService<S> {
///     pub fn new(operation_id: ShapeId, inner: S) -> Self {
///         Self {
///             operation_id,
///             inner
///         }
///     }
/// }
///
/// fn map<S>(operation_id: ShapeId, inner: S) -> PrintService<S> {
///     PrintService { operation_id, inner }
/// }
///
/// // This plugin applies the `PrintService` middleware around every operation.
/// let plugin = plugin_from_operation_id_fn(map);
/// # struct CheckHealth;
/// # impl aws_smithy_http_server::operation::OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # let _ = aws_smithy_http_server::plugin::Plugin::<(), CheckHealth, ()>::apply(&plugin, ());
/// ```
pub fn plugin_from_operation_id_fn<F>(f: F) -> OperationIdFn<F> {
    OperationIdFn { f }
}

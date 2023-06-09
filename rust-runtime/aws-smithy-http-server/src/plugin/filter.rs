/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{either::Either, IdentityPlugin};

use crate::operation::{Operation, OperationShape};
use crate::shape_id::ShapeId;

use super::Plugin;

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::NAME`](crate::operation::OperationShape).
///
/// See [`filter_by_operation_id`] for more details.
pub struct FilterByOperationId<Inner, F> {
    inner: Inner,
    predicate: F,
}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::NAME`](crate::operation::OperationShape).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::filter_by_operation_id;
/// use aws_smithy_http_server::shape_id::ShapeId;
/// # use aws_smithy_http_server::{plugin::Plugin, operation::{Operation, OperationShape}};
/// # struct Pl;
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const NAME: ShapeId = ShapeId::new("ns#CheckHealth", "ns", "CheckHealth"); type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<(), CheckHealth, (), ()> for Pl { type Service = (); type Layer = (); fn map(&self, input: Operation<(), ()>) -> Operation<(), ()> { input }}
/// # let plugin = Pl;
/// # let operation = Operation { inner: (), layer: () };
/// // Prevents `plugin` from being applied to the `CheckHealth` operation.
/// let filtered_plugin = filter_by_operation_id(plugin, |id| id.name() != CheckHealth::NAME.name());
/// let new_operation = filtered_plugin.map(operation);
/// ```
pub fn filter_by_operation_id<Inner, F>(plugins: Inner, predicate: F) -> FilterByOperationId<Inner, F>
where
    F: Fn(ShapeId) -> bool,
{
    FilterByOperationId::new(plugins, predicate)
}

impl<Inner, F> FilterByOperationId<Inner, F> {
    /// Creates a new [`FilterByOperationId`].
    fn new(inner: Inner, predicate: F) -> Self {
        Self { inner, predicate }
    }
}

impl<P, Op, S, L, Inner, F> Plugin<P, Op, S, L> for FilterByOperationId<Inner, F>
where
    F: Fn(ShapeId) -> bool,
    Inner: Plugin<P, Op, S, L>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;
    type Layer = Either<Inner::Layer, L>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        let either_plugin = if (self.predicate)(Op::NAME) {
            Either::Left { value: &self.inner }
        } else {
            Either::Right { value: IdentityPlugin }
        };
        either_plugin.map(input)
    }
}

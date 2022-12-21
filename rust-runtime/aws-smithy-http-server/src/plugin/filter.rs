/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::util::Either;

use crate::operation::{Operation, OperationShape};

use super::Plugin;

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::NAME`](crate::operation::OperationShape).
///
/// See [`filter_by_operation_name`] for more details.
pub struct FilterByOperationName<Inner, F> {
    inner: Inner,
    predicate: F,
}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::NAME`](crate::operation::OperationShape).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::filter_by_operation_name;
/// # use aws_smithy_http_server::{plugin::Plugin, operation::{Operation, OperationShape}};
/// # struct Pl;
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const NAME: &'static str = ""; type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<(), CheckHealth, (), ()> for Pl { type Service = (); type Layer = (); fn map(&self, input: Operation<(), ()>) -> Operation<(), ()> { input }}
/// # let plugin = Pl;
/// # let operation = Operation { inner: (), layer: () };
/// // Prevents `plugin` from being applied to the `CheckHealth` operation.
/// let filtered_plugin = filter_by_operation_name(plugin, |name| name != CheckHealth::NAME);
/// let new_operation = filtered_plugin.map(operation);
/// ```
pub fn filter_by_operation_name<Inner, F>(plugins: Inner, predicate: F) -> FilterByOperationName<Inner, F>
where
    F: Fn(&str) -> bool,
{
    FilterByOperationName::new(plugins, predicate)
}

impl<Inner, F> FilterByOperationName<Inner, F> {
    /// Creates a new [`FilterByOperationName`].
    fn new(inner: Inner, predicate: F) -> Self {
        Self { inner, predicate }
    }
}

impl<P, Op, S, L, Inner, F> Plugin<P, Op, S, L> for FilterByOperationName<Inner, F>
where
    F: Fn(&str) -> bool,
    Inner: Plugin<P, Op, S, L>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;
    type Layer = Either<Inner::Layer, L>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        if (self.predicate)(Op::NAME) {
            let Operation { inner, layer } = self.inner.map(input);
            Operation {
                inner: Either::A(inner),
                layer: Either::A(layer),
            }
        } else {
            Operation {
                inner: Either::B(input.inner),
                layer: Either::B(input.layer),
            }
        }
    }
}

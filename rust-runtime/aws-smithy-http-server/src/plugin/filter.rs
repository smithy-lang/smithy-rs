/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{either::Either, IdentityPlugin};

use crate::operation::OperationShape;

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
/// # use aws_smithy_http_server::{plugin::Plugin, operation::OperationShape};
/// # struct Pl;
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const NAME: &'static str = ""; type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<(), CheckHealth, ()> for Pl { type Service = (); fn apply(&self, input: ()) -> Self::Service { input }}
/// # let plugin = Pl;
/// # let svc = ();
/// // Prevents `plugin` from being applied to the `CheckHealth` operation.
/// let filtered_plugin = filter_by_operation_name(plugin, |name| name != CheckHealth::NAME);
/// let new_operation = filtered_plugin.apply(svc);
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

impl<P, Op, S, Inner, F> Plugin<P, Op, S> for FilterByOperationName<Inner, F>
where
    F: Fn(&str) -> bool,
    Inner: Plugin<P, Op, S>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;

    fn apply(&self, svc: S) -> Self::Service {
        let either_plugin = if (self.predicate)(Op::NAME) {
            Either::Left { value: &self.inner }
        } else {
            Either::Right { value: IdentityPlugin }
        };
        either_plugin.apply(svc)
    }
}

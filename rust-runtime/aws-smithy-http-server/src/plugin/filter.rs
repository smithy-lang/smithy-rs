/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{either::Either, IdentityPlugin};

use crate::operation::OperationShape;
use crate::service::ServiceShapeMember;
use crate::shape_id::ShapeId;

use super::Plugin;

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::ID`](crate::operation::OperationShape).
///
/// See [`filter_by_operation_id`] for more details.
pub struct FilterByOperationId<Inner, F> {
    inner: Inner,
    predicate: F,
}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`OperationShape::ID`](crate::operation::OperationShape).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::filter_by_operation_id;
/// # use aws_smithy_http_server::{plugin::Plugin, operation::OperationShape, shape_id::ShapeId};
/// # struct Pl;
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<(), CheckHealth, ()> for Pl { type Service = (); fn apply(&self, input: ()) -> Self::Service { input }}
/// # let plugin = Pl;
/// # let svc = ();
/// // Prevents `plugin` from being applied to the `CheckHealth` operation.
/// let filtered_plugin = filter_by_operation_id(plugin, |name| name != CheckHealth::ID);
/// let new_operation = filtered_plugin.apply(svc);
/// ```
pub fn filter_by_operation_id<Inner, F>(plugins: Inner, predicate: F) -> FilterByOperationId<Inner, F>
where
    F: Fn(ShapeId) -> bool,
{
    FilterByOperationId {
        inner: plugins,
        predicate,
    }
}

impl<Ser, Op, S, Inner, F> Plugin<Ser, Op, S> for FilterByOperationId<Inner, F>
where
    F: Fn(ShapeId) -> bool,
    Inner: Plugin<Ser, Op, S>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;

    fn apply(&self, svc: S) -> Self::Service {
        let either_plugin = if (self.predicate)(Op::ID) {
            Either::Left { value: &self.inner }
        } else {
            Either::Right { value: IdentityPlugin }
        };
        either_plugin.apply(svc)
    }
}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`ServiceShape::Operations`](crate::service::ServiceShape::Operations).
///
/// See [`filter_by_operation`] for more details.
pub struct FilterByOperation<Inner, F> {
    inner: Inner,
    predicate: F,
}

impl<Ser, Op, S, Inner, F> Plugin<Ser, Op, S> for FilterByOperation<Inner, F>
where
    Ser: ServiceShapeMember<Op>,
    F: Fn(Ser::Operations) -> bool,
    Inner: Plugin<Ser, Op, S>,
    Op: OperationShape,
{
    type Service = Either<Inner::Service, S>;

    fn apply(&self, svc: S) -> Self::Service {
        let either_plugin = if (self.predicate)(<Ser as ServiceShapeMember<Op>>::ENUM_VALUE) {
            Either::Left { value: &self.inner }
        } else {
            Either::Right { value: IdentityPlugin }
        };
        either_plugin.apply(svc)
    }
}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`ServiceShape::Operations`](crate::service::ServiceShape::Operations).
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::filter_by_operation;
/// # use aws_smithy_http_server::{plugin::Plugin, operation::OperationShape, shape_id::ShapeId, service::{ServiceShape, ServiceShapeMember}};
/// # struct Pl;
/// # struct PokemonService;
/// # #[derive(PartialEq, Eq)]
/// # enum Operation { CheckHealth }
/// # impl ServiceShape for PokemonService { const ID: ShapeId = ShapeId::new("", "", ""); type Operations = Operation; type Protocol = (); }
/// # impl ServiceShapeMember<CheckHealth> for PokemonService { const ENUM_VALUE: Operation = Operation::CheckHealth; }
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<PokemonService, CheckHealth, ()> for Pl { type Service = (); fn apply(&self, input: ()) -> Self::Service { input }}
/// # let plugin = Pl;
/// # let svc = ();
/// // Prevents `plugin` from being applied to the `CheckHealth` operation.
/// let filtered_plugin = filter_by_operation(plugin, |name| name != Operation::CheckHealth);
/// let new_operation = filtered_plugin.apply(svc);
/// ```
pub fn filter_by_operation<Inner, F>(plugins: Inner, predicate: F) -> FilterByOperation<Inner, F> {
    FilterByOperation {
        inner: plugins,
        predicate,
    }
}

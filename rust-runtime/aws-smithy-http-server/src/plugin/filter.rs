/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{either::Either, IdentityPlugin, ModelMarker};

use crate::operation::OperationShape;
use crate::service::ContainsOperation;

use super::{HttpMarker, Plugin};

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`ServiceShape::Operations`](crate::service::ServiceShape::Operations).
///
/// This contrasts with [`Scoped`](crate::plugin::Scoped) which can be used to selectively apply a [`Plugin`] to a
/// subset of operations at _compile time_.
///
/// See [`filter_by_operation`] for more details.
pub struct FilterByOperation<Inner, F> {
    inner: Inner,
    predicate: F,
}

impl<Ser, Op, T, Inner, F> Plugin<Ser, Op, T> for FilterByOperation<Inner, F>
where
    Ser: ContainsOperation<Op>,
    F: Fn(Ser::Operations) -> bool,
    Inner: Plugin<Ser, Op, T>,
    Op: OperationShape,
{
    type Output = Either<Inner::Output, T>;

    fn apply(&self, input: T) -> Self::Output {
        let either_plugin = if (self.predicate)(<Ser as ContainsOperation<Op>>::VALUE) {
            Either::Left { value: &self.inner }
        } else {
            Either::Right { value: IdentityPlugin }
        };
        either_plugin.apply(input)
    }
}

impl<Inner, F> HttpMarker for FilterByOperation<Inner, F> where Inner: HttpMarker {}
impl<Inner, F> ModelMarker for FilterByOperation<Inner, F> where Inner: ModelMarker {}

/// Filters the application of an inner [`Plugin`] using a predicate over the
/// [`ServiceShape::Operations`](crate::service::ServiceShape::Operations).
///
/// Users should prefer [`Scoped`](crate::plugin::Scoped) and fallback to [`filter_by_operation`]
/// in cases where [`Plugin`] application must be decided at runtime.
///
/// # Example
///
/// ```rust
/// use aws_smithy_http_server::plugin::filter_by_operation;
/// # use aws_smithy_http_server::{plugin::Plugin, operation::OperationShape, shape_id::ShapeId, service::{ServiceShape, ContainsOperation}};
/// # struct Pl;
/// # struct PokemonService;
/// # #[derive(PartialEq, Eq)]
/// # enum Operation { CheckHealth }
/// # impl ServiceShape for PokemonService { const VERSION: Option<&'static str> = None; const ID: ShapeId = ShapeId::new("", "", ""); type Operations = Operation; type Protocol = (); }
/// # impl ContainsOperation<CheckHealth> for PokemonService { const VALUE: Operation = Operation::CheckHealth; }
/// # struct CheckHealth;
/// # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # impl Plugin<PokemonService, CheckHealth, ()> for Pl { type Output = (); fn apply(&self, input: ()) -> Self::Output { input }}
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

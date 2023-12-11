/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::service::ContainsOperation;

use super::Plugin;

/// An adapter to convert a `Fn(ShapeId, T) -> Service` closure into a [`Plugin`]. See [`plugin_from_operation_fn`] for more details.
pub struct OperationFn<F> {
    f: F,
}

impl<Ser, Op, T, NewService, F> Plugin<Ser, Op, T> for OperationFn<F>
where
    Ser: ContainsOperation<Op>,
    F: Fn(Ser::Operations, T) -> NewService,
{
    type Output = NewService;

    fn apply(&self, input: T) -> Self::Output {
        (self.f)(Ser::VALUE, input)
    }
}

/// Constructs a [`Plugin`] using a closure over the [`ServiceShape::] `F: Fn(ShapeId, T) -> Service`.
///
/// # Example
///
/// ```rust
/// # use aws_smithy_http_server::{service::*, operation::OperationShape, plugin::Plugin, shape_id::ShapeId};
/// # pub enum Operation { CheckHealth, GetPokemonSpecies }
/// # impl Operation { fn shape_id(&self) -> ShapeId { ShapeId::new("", "", "") }}
/// # pub struct CheckHealth;
/// # pub struct GetPokemonSpecies;
/// # pub struct PokemonService;
/// # impl ServiceShape for PokemonService {
/// #   const ID: ShapeId = ShapeId::new("", "", "");
/// #   const VERSION: Option<&'static str> = None;
/// #   type Protocol = ();
/// #   type Operations = Operation;
/// # }
/// # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # impl OperationShape for GetPokemonSpecies { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
/// # impl ContainsOperation<CheckHealth> for PokemonService { const VALUE: Operation = Operation::CheckHealth; }
/// # impl ContainsOperation<GetPokemonSpecies> for PokemonService { const VALUE: Operation = Operation::GetPokemonSpecies; }
/// use aws_smithy_http_server::plugin::plugin_from_operation_fn;
/// use tower::layer::layer_fn;
///
/// struct FooService<S> {
///     info: String,
///     inner: S
/// }
///
/// fn map<S>(op: Operation, inner: S) -> FooService<S> {
///     match op {
///         Operation::CheckHealth => FooService { info: op.shape_id().name().to_string(), inner },
///         Operation::GetPokemonSpecies => FooService { info: "bar".to_string(), inner },
///         _ => todo!()
///     }
/// }
///
/// // This plugin applies the `FooService` middleware around every operation.
/// let plugin = plugin_from_operation_fn(map);
/// # let _ = Plugin::<PokemonService, CheckHealth, ()>::apply(&plugin, ());
/// # let _ = Plugin::<PokemonService, GetPokemonSpecies, ()>::apply(&plugin, ());
/// ```
pub fn plugin_from_operation_fn<F>(f: F) -> OperationFn<F> {
    OperationFn { f }
}

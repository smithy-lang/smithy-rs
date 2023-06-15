/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The plugin system allows you to build middleware with an awareness of the operation it is applied to.
//!
//! The system centers around the [`Plugin`] trait. In addition, this module provides helpers for composing and
//! combining [`Plugin`]s.
//!
//! # Filtered application of a HTTP [`Layer`](tower::Layer)
//!
//! ```
//! # use aws_smithy_http_server::plugin::*;
//! # use aws_smithy_http_server::shape_id::ShapeId;
//! # let layer = ();
//! # #[derive(PartialEq)]
//! # enum Operation { GetPokemonSpecies }
//! # struct GetPokemonSpecies;
//! # impl GetPokemonSpecies { const ID: ShapeId = ShapeId::new("namespace#name", "namespace", "name"); };
//! // Create a `Plugin` from a HTTP `Layer`
//! let plugin = LayerPlugin(layer);
//!
//! // Only apply the layer to operations with name "GetPokemonSpecies"
//! let plugin = filter_by_operation(plugin, |operation: Operation| operation == Operation::GetPokemonSpecies);
//! ```
//!
//! # Construct a [`Plugin`] from a closure that takes as input the operation name
//!
//! ```rust
//! # use aws_smithy_http_server::{service::*, operation::OperationShape, plugin::Plugin, shape_id::ShapeId};
//! # pub enum Operation { CheckHealth, GetPokemonSpecies }
//! # impl Operation { fn shape_id(&self) -> ShapeId { ShapeId::new("", "", "") }}
//! # pub struct CheckHealth;
//! # pub struct GetPokemonSpecies;
//! # pub struct PokemonService;
//! # impl ServiceShape for PokemonService {
//! #   const ID: ShapeId = ShapeId::new("", "", "");
//! #   const VERSION: Option<&'static str> = None;
//! #   type Protocol = ();
//! #   type Operations = Operation;
//! # }
//! # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
//! # impl OperationShape for GetPokemonSpecies { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
//! # impl ContainsOperation<CheckHealth> for PokemonService { const VALUE: Operation = Operation::CheckHealth; }
//! # impl ContainsOperation<GetPokemonSpecies> for PokemonService { const VALUE: Operation = Operation::GetPokemonSpecies; }
//! use aws_smithy_http_server::plugin::plugin_from_operation_fn;
//! use tower::layer::layer_fn;
//!
//! struct FooService<S> {
//!     info: String,
//!     inner: S
//! }
//!
//! fn map<S>(op: Operation, inner: S) -> FooService<S> {
//!     match op {
//!         Operation::CheckHealth => FooService { info: op.shape_id().name().to_string(), inner },
//!         Operation::GetPokemonSpecies => FooService { info: "bar".to_string(), inner },
//!         _ => todo!()
//!     }
//! }
//!
//! // This plugin applies the `FooService` middleware around every operation.
//! let plugin = plugin_from_operation_fn(map);
//! # let _ = Plugin::<PokemonService, CheckHealth, ()>::apply(&plugin, ());
//! # let _ = Plugin::<PokemonService, GetPokemonSpecies, ()>::apply(&plugin, ());
//! ```
//!
//! # Combine [`Plugin`]s
//!
//! ```
//! # use aws_smithy_http_server::plugin::*;
//! # let a = (); let b = ();
//! // Combine `Plugin`s `a` and `b`
//! let plugin = PluginPipeline::new()
//!     .push(a)
//!     .push(b);
//! ```
//!
//! As noted in the [`PluginPipeline`] documentation, the plugins' runtime logic is executed in registration order,
//! meaning that `a` is run _before_ `b` in the example above.
//!
//! # Example Implementation
//!
//! ```rust
//! use aws_smithy_http_server::{
//!     operation::OperationShape,
//!     service::ServiceShape,
//!     plugin::{Plugin, PluginPipeline, PluginStack},
//!     shape_id::ShapeId,
//! };
//! # use tower::{layer::util::Stack, Layer, Service};
//! # use std::task::{Context, Poll};
//!
//! /// A [`Service`] that adds a print log.
//! #[derive(Clone, Debug)]
//! pub struct PrintService<S> {
//!     inner: S,
//!     service_id: ShapeId,
//!     operation_id: ShapeId
//! }
//!
//! impl<R, S> Service<R> for PrintService<S>
//! where
//!     S: Service<R>,
//! {
//!     type Response = S::Response;
//!     type Error = S::Error;
//!     type Future = S::Future;
//!
//!     fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//!         self.inner.poll_ready(cx)
//!     }
//!
//!     fn call(&mut self, req: R) -> Self::Future {
//!         println!("Hi {} in {}", self.operation_id.absolute(), self.service_id.absolute());
//!         self.inner.call(req)
//!     }
//! }
//!
//! /// A [`Plugin`] for a service builder to add a [`PrintLayer`] over operations.
//! #[derive(Debug)]
//! pub struct PrintPlugin;
//!
//! impl<Ser, Op, T> Plugin<Ser, Op, T> for PrintPlugin
//! where
//!     Ser: ServiceShape,
//!     Op: OperationShape,
//! {
//!     type Output = PrintService<T>;
//!
//!     fn apply(&self, inner: T) -> Self::Output {
//!         PrintService {
//!             inner,
//!             service_id: Op::ID,
//!             operation_id: Ser::ID,
//!         }
//!     }
//! }
//! ```
//!

pub mod alb_health_check;
mod closure;
mod either;
mod filter;
mod identity;
mod layer;
mod pipeline;
#[doc(hidden)]
pub mod scoped;
mod stack;

pub use closure::{plugin_from_operation_fn, OperationFn};
pub use either::Either;
pub use filter::{filter_by_operation, FilterByOperation};
pub use identity::IdentityPlugin;
pub use layer::{LayerPlugin, PluginLayer};
pub use pipeline::PluginPipeline;
pub use scoped::Scoped;
pub use stack::PluginStack;

/// A mapping from one [`Service`](tower::Service) to another. This should be viewed as a
/// [`Layer`](tower::Layer) parameterized by the protocol and operation.
///
/// The generics `Ser` and `Op` allow the behavior to be parameterized by the [Smithy service] and
/// [operation] it's applied to.
///
/// See [module](crate::plugin) documentation for more information.
///
/// [Smithy service]: https://smithy.io/2.0/spec/service-types.html#service
/// [operation]: https://smithy.io/2.0/spec/service-types.html#operation
pub trait Plugin<Ser, Op, T> {
    /// The type of the new [`Service`](tower::Service).
    type Output;

    /// Maps a [`Service`](tower::Service) to another.
    fn apply(&self, input: T) -> Self::Output;
}

impl<'a, Ser, Op, T, Pl> Plugin<Ser, Op, T> for &'a Pl
where
    Pl: Plugin<Ser, Op, T>,
{
    type Output = Pl::Output;

    fn apply(&self, inner: T) -> Self::Output {
        <Pl as Plugin<Ser, Op, T>>::apply(self, inner)
    }
}

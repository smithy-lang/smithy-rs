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
//! # struct GetPokemonSpecies;
//! # impl GetPokemonSpecies { const ID: ShapeId = ShapeId::new("namespace#name", "namespace", "name"); };
//! // Create a `Plugin` from a HTTP `Layer`
//! let plugin = LayerPlugin(layer);
//!
//! // Only apply the layer to operations with name "GetPokemonSpecies"
//! let plugin = filter_by_operation_id(plugin, |id| id.name() == GetPokemonSpecies::ID.name());
//! ```
//!
//! # Construct a [`Plugin`] from a closure that takes as input the operation name
//!
//! ```
//! # use aws_smithy_http_server::plugin::*;
//! # use aws_smithy_http_server::shape_id::ShapeId;
//! // A `tower::Layer` which requires the operation name
//! struct PrintLayer {
//!     name: ShapeId,
//! }
//!
//! // Create a `Plugin` using `PrintLayer`
//! let plugin = plugin_from_operation_id_fn(|name| PrintLayer { name });
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
//!     ser_id: ShapeId,
//!     op_id: ShapeId
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
//!         println!("Hi {} in {}", self.op_id.absolute(), self.ser_id.absolute());
//!         self.inner.call(req)
//!     }
//! }
//!
//! /// A [`Plugin`] for a service builder to add a [`PrintLayer`] over operations.
//! #[derive(Debug)]
//! pub struct PrintPlugin;
//!
//! impl<Ser, Op, S> Plugin<Ser, Op, S> for PrintPlugin
//! where
//!     Ser: ServiceShape,
//!     Op: OperationShape,
//! {
//!     type Service = PrintService<S>;
//!
//!     fn apply(&self, inner: S) -> Self::Service {
//!         PrintService {
//!             inner,
//!             ser_id: Op::ID,
//!             op_id: Ser::ID,
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

pub use closure::{plugin_from_operation_id_fn, OperationIdFn};
pub use either::Either;
pub use filter::{filter_by_operation, filter_by_operation_id, FilterByOperation, FilterByOperationId};
pub use identity::IdentityPlugin;
pub use layer::{LayerPlugin, PluginLayer};
pub use pipeline::PluginPipeline;
pub use scoped::Scoped;
pub use stack::PluginStack;

/// A mapping from one [`Service`](tower::Service) to another. This should be viewed as a
/// [`Layer`](tower::Layer) parameterized by the protocol and operation.
///
/// The generics `Ser` and `Op` allow the behavior to be parameterized.
///
/// See [module](crate::plugin) documentation for more information.
pub trait Plugin<Ser, Op, S> {
    /// The type of the new [`Service`](tower::Service).
    type Service;

    /// Maps a [`Service`](tower::Service) to another.
    fn apply(&self, svc: S) -> Self::Service;
}

impl<'a, Ser, Op, S, Pl> Plugin<Ser, Op, S> for &'a Pl
where
    Pl: Plugin<Ser, Op, S>,
{
    type Service = Pl::Service;

    fn apply(&self, inner: S) -> Self::Service {
        <Pl as Plugin<Ser, Op, S>>::apply(self, inner)
    }
}

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
//! # let layer = ();
//! # struct GetPokemonSpecies;
//! # impl GetPokemonSpecies { const NAME: &'static str = ""; };
//! // Create a `Plugin` from a HTTP `Layer`
//! let plugin = HttpLayer(layer);
//!
//! // Only apply the layer to operations with name "GetPokemonSpecies"
//! let plugin = filter_by_operation_name(plugin, |name| name == GetPokemonSpecies::NAME);
//! ```
//!
//! # Construct a [`Plugin`] from a closure that takes as input the operation name
//!
//! ```
//! # use aws_smithy_http_server::plugin::*;
//! // A `tower::Layer` which requires the operation name
//! struct PrintLayer {
//!     name: &'static str
//! }
//!
//! // Create a `Plugin` using `PrintLayer`
//! let plugin = plugin_from_operation_name_fn(|name| PrintLayer { name });
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
//!     operation::{Operation, OperationShape},
//!     plugin::{Plugin, PluginPipeline, PluginStack},
//! };
//! # use tower::{layer::util::Stack, Layer, Service};
//! # use std::task::{Context, Poll};
//!
//! /// A [`Service`] that adds a print log.
//! #[derive(Clone, Debug)]
//! pub struct PrintService<S> {
//!     inner: S,
//!     name: &'static str,
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
//!         println!("Hi {}", self.name);
//!         self.inner.call(req)
//!     }
//! }
//!
//! /// A [`Layer`] which constructs the [`PrintService`].
//! #[derive(Debug)]
//! pub struct PrintLayer {
//!     name: &'static str,
//! }
//! impl<S> Layer<S> for PrintLayer {
//!     type Service = PrintService<S>;
//!
//!     fn layer(&self, service: S) -> Self::Service {
//!         PrintService {
//!             inner: service,
//!             name: self.name,
//!         }
//!     }
//! }
//!
//! /// A [`Plugin`] for a service builder to add a [`PrintLayer`] over operations.
//! #[derive(Debug)]
//! pub struct PrintPlugin;
//!
//! impl<P, Op, S, L> Plugin<P, Op, S, L> for PrintPlugin
//! where
//!     Op: OperationShape,
//! {
//!     type Service = S;
//!     type Layer = Stack<L, PrintLayer>;
//!
//!     fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
//!         input.layer(PrintLayer { name: Op::NAME })
//!     }
//! }
//! ```
//!

mod closure;
mod filter;
mod identity;
mod layer;
mod pipeline;
mod stack;

use crate::operation::Operation;

pub use closure::{plugin_from_operation_name_fn, OperationNameFn};
pub use filter::{filter_by_operation_name, FilterByOperationName};
pub use identity::IdentityPlugin;
pub use layer::HttpLayer;
pub use pipeline::PluginPipeline;
pub use stack::PluginStack;

/// A mapping from one [`Operation`] to another. Used to modify the behavior of
/// [`Upgradable`](crate::operation::Upgradable) and therefore the resulting service builder.
///
/// The generics `Protocol` and `Op` allow the behavior to be parameterized.
///
/// See [module](crate::plugin) documentation for more information.
pub trait Plugin<Protocol, Op, S, L> {
    /// The type of the new [`Service`](tower::Service).
    type Service;
    /// The type of the new [`Layer`](tower::Layer).
    type Layer;

    /// Maps an [`Operation`] to another.
    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}

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
//! // Create a `Plugin` from a HTTP `Layer`
//! let plugin = HttpLayer(layer);
//!
//! // Only apply the layer to operations with name "GetPokemonSpecies"
//! let plugin = filter_by_operation_name(plugin, |name| name == "GetPokemonSpecies");
//! ```
//!
//! # Construct [`Plugin`] from Operation name closure
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
pub trait Plugin<Protocol, Op, S, L> {
    type Service;
    type Layer;

    /// Maps an [`Operation`] to another.
    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}

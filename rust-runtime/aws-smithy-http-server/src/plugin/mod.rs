/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod filter;
mod identity;
mod pipeline;
mod stack;

use crate::operation::Operation;

pub use filter::FilterByOperationName;
pub use identity::IdentityPlugin;
pub use pipeline::PluginPipeline;
pub use stack::PluginStack;

/// A mapping from one [`Operation`] to another. Used to modify the behavior of
/// [`Upgradable`](crate::operation::Upgradable) and therefore the resulting service builder,
///
/// The generics `Protocol` and `Op` allow the behavior to be parameterized.
pub trait Plugin<Protocol, Op, S, L> {
    type Service;
    type Layer;

    /// Maps an [`Operation`] to another.
    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}

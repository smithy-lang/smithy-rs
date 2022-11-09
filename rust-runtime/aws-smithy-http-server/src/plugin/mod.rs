/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod composer;
mod filter;
mod identity;
mod stack;

use crate::operation::Operation;

pub use composer::PluginPipeline;
pub use filter::FilterByOperationName;
pub use identity::IdentityPlugin;
pub use stack::PluginStack;

/// Provides a standard interface for applying [`Plugin`]s to a service builder. This is implemented automatically for
/// all builders.
///
/// As [`Plugin`]s modify the way in which [`Operation`]s are [`upgraded`](crate::operation::Upgradable) we can use
/// [`Pluggable`] as a foundation to write extension traits which are implemented for all service builders.
///
/// # Example
///
/// ```
/// # struct PrintPlugin;
/// # use aws_smithy_http_server::plugin::Pluggable;
/// trait PrintExt: Pluggable<PrintPlugin> {
///     fn print(self) -> Self::Output where Self: Sized {
///         self.apply(PrintPlugin)
///     }
/// }
///
/// impl<Builder> PrintExt for Builder where Builder: Pluggable<PrintPlugin> {}
/// ```
pub trait Pluggable<NewPlugin> {
    type Output;

    /// Applies a [`Plugin`] to the service builder.
    fn apply(self, plugin: NewPlugin) -> Self::Output;
}

/// A mapping from one [`Operation`] to another. Used to modify the behavior of
/// [`Upgradable`](crate::operation::Upgradable) and therefore the resulting service builder,
///
/// The generics `Protocol` and `Op` allow the behavior to be parameterized.
///
/// Every service builder enjoys [`Pluggable`] and therefore can be provided with a [`Plugin`] using
/// [`Pluggable::apply`].
pub trait Plugin<Protocol, Op, S, L> {
    type Service;
    type Layer;

    /// Maps an [`Operation`] to another.
    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}

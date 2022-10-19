/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod filter;
mod identity;
mod stack;

use crate::operation::Operation;

pub use filter::*;
pub use identity::*;
pub use stack::*;

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

/// An extension trait for [`Plugin`].
pub trait PluginExt<P, Op, S, L>: Plugin<P, Op, S, L> {
    /// Stacks another [`Plugin`], running them sequentially.
    fn stack<Other>(self, other: Other) -> PluginStack<Self, Other>
    where
        Self: Sized,
    {
        PluginStack::new(self, other)
    }

    /// Filters the application of the [`Plugin`] using a predicate over the
    /// [`OperationShape::NAME`](crate::operation::OperationShape).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use aws_smithy_http_server::{plugin::{Plugin, PluginExt}, operation::{Operation, OperationShape}};
    /// # struct Pl;
    /// # struct CheckHealth;
    /// # impl OperationShape for CheckHealth { const NAME: &'static str = ""; type Input = (); type Output = (); type Error = (); }
    /// # impl Plugin<(), CheckHealth, (), ()> for Pl { type Service = (); type Layer = (); fn map(&self, input: Operation<(), ()>) -> Operation<(), ()> { input }}
    /// # let plugin = Pl;
    /// # let operation = Operation { inner: (), layer: () };
    /// // Prevents `plugin` from being applied to the `CheckHealth` operation.
    /// let filtered_plugin = plugin.filter_by_operation_name(|name| name != CheckHealth::NAME);
    /// let new_operation = filtered_plugin.map(operation);
    /// ```
    fn filter_by_operation_name<F>(self, predicate: F) -> FilterByOperationName<Self, F>
    where
        Self: Sized,
        F: Fn(&str) -> bool,
    {
        FilterByOperationName::new(self, predicate)
    }
}

impl<Pl, P, Op, S, L> PluginExt<P, Op, S, L> for Pl where Pl: Plugin<P, Op, S, L> {}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::operation::Operation;

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

    /// Applies a plugin to the service builder.
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

/// An [`Plugin`] that maps an `input` [`Operation`] to itself.
pub struct IdentityPlugin;

impl<P, Op, S, L> Plugin<P, Op, S, L> for IdentityPlugin {
    type Service = S;
    type Layer = L;

    fn map(&self, input: Operation<S, L>) -> Operation<S, L> {
        input
    }
}

/// A wrapper struct which composes an `Inner` and an `Outer` [`Plugin`].
pub struct PluginStack<Inner, Outer> {
    inner: Inner,
    outer: Outer,
}

impl<Inner, Outer> PluginStack<Inner, Outer> {
    /// Creates a new [`PluginStack`].
    pub fn new(inner: Inner, outer: Outer) -> Self {
        PluginStack { inner, outer }
    }
}

impl<P, Op, S, L, Inner, Outer> Plugin<P, Op, S, L> for PluginStack<Inner, Outer>
where
    Inner: Plugin<P, Op, S, L>,
    Outer: Plugin<P, Op, Inner::Service, Inner::Layer>,
{
    type Service = Outer::Service;
    type Layer = Outer::Layer;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        let inner = self.inner.map(input);
        self.outer.map(inner)
    }
}

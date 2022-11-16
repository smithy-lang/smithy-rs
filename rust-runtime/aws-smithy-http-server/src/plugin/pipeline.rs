/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::operation::Operation;
use crate::plugin::{IdentityPlugin, Plugin, PluginStack};

/// A wrapper struct for composing [`Plugin`]s.
/// It is used as input for the `builder_with_plugins` method on the generate service struct
/// (e.g. `PokemonService::builder_with_plugins`).
///
/// ## Applying plugins in a sequence
///
/// You can use the [`push`](PluginPipeline::push) method to apply a new plugin after the ones that
/// have already been registered.
///
/// ```rust
/// use aws_smithy_http_server::plugin::PluginPipeline;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
///
/// let pipeline = PluginPipeline::new().push(LoggingPlugin).push(MetricsPlugin);
/// ```
///
/// The plugins' runtime logic is executed in registration order.
/// In our example above, `LoggingPlugin` would run first, while `MetricsPlugin` is executed last.
///
/// ## Wrapping the current plugin pipeline
///
/// From time to time, you might have a need to transform the entire pipeline that has been built
/// so far - e.g. you only want to apply those plugins for a specific operation.
///
/// `PluginPipeline` is itself a [`Plugin`]: you can apply any transformation that expects a
/// [`Plugin`] to an entire pipeline. In this case, we want to use
/// [`filter_by_operation_name`](crate::plugin::filter_by_operation_name) to limit the scope of
/// the logging and metrics plugins to the `CheckHealth` operation:
///
/// ```rust
/// use aws_smithy_http_server::plugin::{filter_by_operation_name, PluginPipeline};
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;
/// # struct CheckHealth;
/// # impl CheckHealth { const NAME: &'static str = "MyName"; }
///
/// // The logging and metrics plugins will only be applied to the `CheckHealth` operation.
/// let operation_specific_pipeline = filter_by_operation_name(
///     PluginPipeline::new()
///         .push(LoggingPlugin)
///         .push(MetricsPlugin),
///     |name| name == CheckHealth::NAME
/// );
/// let pipeline = PluginPipeline::new()
///     .push(operation_specific_pipeline)
///     // The auth plugin will be applied to all operations
///     .push(AuthPlugin);
/// ```
///
/// ## Concatenating two plugin pipelines
///
/// `PluginPipeline` is a good way to bundle together multiple plugins, ensuring they are all
/// registered in the correct order.
///
/// Since `PluginPipeline` is itself a [`Plugin`], you can use the [`push`](PluginPipeline::push) to
/// append, at once, all the plugins in another pipeline to the current pipeline:
///
/// ```rust
/// use aws_smithy_http_server::plugin::{IdentityPlugin, PluginPipeline, PluginStack};
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;
///
/// pub fn get_bundled_pipeline() -> PluginPipeline<PluginStack<MetricsPlugin, PluginStack<LoggingPlugin, IdentityPlugin>>> {
///     PluginPipeline::new().push(LoggingPlugin).push(MetricsPlugin)
/// }
///
/// let pipeline = PluginPipeline::new()
///     .push(AuthPlugin)
///     .push(get_bundled_pipeline());
/// ```
///
/// ## Providing custom methods on `PluginPipeline`
///
/// You use an **extension trait** to add custom methods on `PluginPipeline`.
///
/// This is a simple example using `AuthPlugin`:
///
/// ```rust
/// use aws_smithy_http_server::plugin::{PluginPipeline, PluginStack};
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;
///
/// pub trait AuthPluginExt<CurrentPlugins> {
///     fn with_auth(self) -> PluginPipeline<PluginStack<AuthPlugin, CurrentPlugins>>;
/// }
///
/// impl<CurrentPlugins> AuthPluginExt<CurrentPlugins> for PluginPipeline<CurrentPlugins> {
///     fn with_auth(self) -> PluginPipeline<PluginStack<AuthPlugin, CurrentPlugins>> {
///         self.push(AuthPlugin)
///     }
/// }
///
/// let pipeline = PluginPipeline::new()
///     .push(LoggingPlugin)
///     // Our custom method!
///     .with_auth();
/// ```
pub struct PluginPipeline<P>(P);

impl Default for PluginPipeline<IdentityPlugin> {
    fn default() -> Self {
        Self(IdentityPlugin)
    }
}

impl PluginPipeline<IdentityPlugin> {
    /// Create an empty [`PluginPipeline`].
    ///
    /// You can use [`PluginPipeline::push`] to add plugins to it.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<P> PluginPipeline<P> {
    /// Apply a new plugin after the ones that have already been registered.
    ///
    /// ```rust
    /// use aws_smithy_http_server::plugin::PluginPipeline;
    /// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
    /// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
    ///
    /// let pipeline = PluginPipeline::new().push(LoggingPlugin).push(MetricsPlugin);
    /// ```
    ///
    /// The plugins' runtime logic is executed in registration order.
    /// In our example above, `LoggingPlugin` would run first, while `MetricsPlugin` is executed last.
    ///
    /// ## Implementation notes
    ///
    /// Plugins are applied to the underlying [`Operation`] in opposite order compared
    /// to their registration order.
    /// But most [`Plugin::map`] implementations desugar to appending a layer to [`Operation`],
    /// usually via [`Operation::layer`].
    /// As an example:
    ///
    /// ```rust,compile_fail
    /// #[derive(Debug)]
    /// pub struct PrintPlugin;
    ///
    /// impl<P, Op, S, L> Plugin<P, Op, S, L> for PrintPlugin
    /// // [...]
    /// {
    ///     // [...]
    ///     fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
    ///         input.layer(PrintLayer { name: Op::NAME })
    ///     }
    /// }
    /// ```
    ///
    /// The layer that is registered **last** via [`Operation::layer`] is the one that gets executed
    /// **first** at runtime when a new request comes in, since it _wraps_ the underlying service.
    ///
    /// This is why plugins in [`PluginPipeline`] are applied in opposite order compared to their
    /// registration order: this ensures that, _at runtime_, their logic is executed
    /// in registration order.
    pub fn push<NewPlugin>(self, new_plugin: NewPlugin) -> PluginPipeline<PluginStack<NewPlugin, P>> {
        PluginPipeline(PluginStack::new(new_plugin, self.0))
    }
}

impl<P, Op, S, L, InnerPlugin> Plugin<P, Op, S, L> for PluginPipeline<InnerPlugin>
where
    InnerPlugin: Plugin<P, Op, S, L>,
{
    type Service = InnerPlugin::Service;
    type Layer = InnerPlugin::Layer;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        self.0.map(input)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// If you make any updates to this file (including Rust docs), make sure you make them to
// `model_plugins.rs` too!

use crate::plugin::{IdentityPlugin, Plugin, PluginStack};

use super::{HttpMarker, LayerPlugin};

/// A wrapper struct for composing HTTP plugins.
///
/// ## Applying plugins in a sequence
///
/// You can use the [`push`](HttpPlugins::push) method to apply a new HTTP plugin after the ones that
/// have already been registered.
///
/// ```rust
/// use aws_smithy_legacy_http_server::plugin::HttpPlugins;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as MetricsPlugin;
///
/// let http_plugins = HttpPlugins::new().push(LoggingPlugin).push(MetricsPlugin);
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
/// `HttpPlugins` is itself a [`Plugin`]: you can apply any transformation that expects a
/// [`Plugin`] to an entire pipeline. In this case, we could use a [scoped
/// plugin](crate::plugin::Scoped) to limit the scope of the logging and metrics plugins to the
/// `CheckHealth` operation:
///
/// ```rust
/// use aws_smithy_legacy_http_server::scope;
/// use aws_smithy_legacy_http_server::plugin::{HttpPlugins, Scoped};
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as AuthPlugin;
/// use aws_smithy_legacy_http_server::shape_id::ShapeId;
/// # #[derive(PartialEq)]
/// # enum Operation { CheckHealth }
/// # struct CheckHealth;
/// # impl CheckHealth { const ID: ShapeId = ShapeId::new("namespace#MyName", "namespace", "MyName"); }
///
/// // The logging and metrics plugins will only be applied to the `CheckHealth` operation.
/// let plugin = HttpPlugins::new()
///     .push(LoggingPlugin)
///     .push(MetricsPlugin);
///
/// scope! {
///     struct OnlyCheckHealth {
///         includes: [CheckHealth],
///         excludes: [/* The rest of the operations go here */]
///     }
/// }
///
/// let filtered_plugin = Scoped::new::<OnlyCheckHealth>(&plugin);
/// let http_plugins = HttpPlugins::new()
///     .push(filtered_plugin)
///     // The auth plugin will be applied to all operations.
///     .push(AuthPlugin);
/// ```
///
/// ## Concatenating two collections of HTTP plugins
///
/// `HttpPlugins` is a good way to bundle together multiple plugins, ensuring they are all
/// registered in the correct order.
///
/// Since `HttpPlugins` is itself a HTTP plugin (it implements the `HttpMarker` trait), you can use
/// the [`push`](HttpPlugins::push) to append, at once, all the HTTP plugins in another
/// `HttpPlugins` to the current `HttpPlugins`:
///
/// ```rust
/// use aws_smithy_legacy_http_server::plugin::{IdentityPlugin, HttpPlugins, PluginStack};
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as AuthPlugin;
///
/// pub fn get_bundled_http_plugins() -> HttpPlugins<PluginStack<MetricsPlugin, PluginStack<LoggingPlugin, IdentityPlugin>>> {
///     HttpPlugins::new().push(LoggingPlugin).push(MetricsPlugin)
/// }
///
/// let http_plugins = HttpPlugins::new()
///     .push(AuthPlugin)
///     .push(get_bundled_http_plugins());
/// ```
///
/// ## Providing custom methods on `HttpPlugins`
///
/// You use an **extension trait** to add custom methods on `HttpPlugins`.
///
/// This is a simple example using `AuthPlugin`:
///
/// ```rust
/// use aws_smithy_legacy_http_server::plugin::{HttpPlugins, PluginStack};
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as AuthPlugin;
///
/// pub trait AuthPluginExt<CurrentPlugins> {
///     fn with_auth(self) -> HttpPlugins<PluginStack<AuthPlugin, CurrentPlugins>>;
/// }
///
/// impl<CurrentPlugins> AuthPluginExt<CurrentPlugins> for HttpPlugins<CurrentPlugins> {
///     fn with_auth(self) -> HttpPlugins<PluginStack<AuthPlugin, CurrentPlugins>> {
///         self.push(AuthPlugin)
///     }
/// }
///
/// let http_plugins = HttpPlugins::new()
///     .push(LoggingPlugin)
///     // Our custom method!
///     .with_auth();
/// ```
#[derive(Debug)]
pub struct HttpPlugins<P>(pub(crate) P);

impl Default for HttpPlugins<IdentityPlugin> {
    fn default() -> Self {
        Self(IdentityPlugin)
    }
}

impl HttpPlugins<IdentityPlugin> {
    /// Create an empty [`HttpPlugins`].
    ///
    /// You can use [`HttpPlugins::push`] to add plugins to it.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<P> HttpPlugins<P> {
    /// Apply a new HTTP plugin after the ones that have already been registered.
    ///
    /// ```rust
    /// use aws_smithy_legacy_http_server::plugin::HttpPlugins;
    /// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as LoggingPlugin;
    /// # use aws_smithy_legacy_http_server::plugin::IdentityPlugin as MetricsPlugin;
    ///
    /// let http_plugins = HttpPlugins::new().push(LoggingPlugin).push(MetricsPlugin);
    /// ```
    ///
    /// The plugins' runtime logic is executed in registration order.
    /// In our example above, `LoggingPlugin` would run first, while `MetricsPlugin` is executed last.
    ///
    /// ## Implementation notes
    ///
    /// Plugins are applied to the underlying [`Service`](tower::Service) in opposite order compared
    /// to their registration order.
    ///
    /// As an example:
    ///
    /// ```rust,compile_fail
    /// #[derive(Debug)]
    /// pub struct PrintPlugin;
    ///
    /// impl<Ser, Op, S> Plugin<Ser, Op, T> for PrintPlugin
    /// // [...]
    /// {
    ///     // [...]
    ///     fn apply(&self, inner: T) -> Self::Service {
    ///         PrintService {
    ///             inner,
    ///             service_id: Ser::ID,
    ///             operation_id: Op::ID
    ///         }
    ///     }
    /// }
    /// ```
    // We eagerly require `NewPlugin: HttpMarker`, despite not really needing it, because compiler
    // errors get _substantially_ better if the user makes a mistake.
    pub fn push<NewPlugin: HttpMarker>(self, new_plugin: NewPlugin) -> HttpPlugins<PluginStack<NewPlugin, P>> {
        HttpPlugins(PluginStack::new(new_plugin, self.0))
    }

    /// Applies a single [`tower::Layer`] to all operations _before_ they are deserialized.
    pub fn layer<L>(self, layer: L) -> HttpPlugins<PluginStack<LayerPlugin<L>, P>> {
        HttpPlugins(PluginStack::new(LayerPlugin(layer), self.0))
    }
}

impl<Ser, Op, T, InnerPlugin> Plugin<Ser, Op, T> for HttpPlugins<InnerPlugin>
where
    InnerPlugin: Plugin<Ser, Op, T>,
{
    type Output = InnerPlugin::Output;

    fn apply(&self, input: T) -> Self::Output {
        self.0.apply(input)
    }
}

impl<InnerPlugin> HttpMarker for HttpPlugins<InnerPlugin> where InnerPlugin: HttpMarker {}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// If you make any updates to this file (including Rust docs), make sure you make them to
// `http_plugins.rs` too!

use crate::plugin::{IdentityPlugin, Plugin, PluginStack};

use super::{LayerPlugin, ModelMarker};

/// A wrapper struct for composing model plugins.
/// It operates identically to [`HttpPlugins`](crate::plugin::HttpPlugins); see its documentation.
#[derive(Debug)]
pub struct ModelPlugins<P>(pub(crate) P);

impl Default for ModelPlugins<IdentityPlugin> {
    fn default() -> Self {
        Self(IdentityPlugin)
    }
}

impl ModelPlugins<IdentityPlugin> {
    /// Create an empty [`ModelPlugins`].
    ///
    /// You can use [`ModelPlugins::push`] to add plugins to it.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<P> ModelPlugins<P> {
    /// Apply a new model plugin after the ones that have already been registered.
    ///
    /// ```rust
    /// use aws_smithy_http_server::plugin::ModelPlugins;
    /// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
    /// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
    ///
    /// let model_plugins = ModelPlugins::new().push(LoggingPlugin).push(MetricsPlugin);
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
    // We eagerly require `NewPlugin: ModelMarker`, despite not really needing it, because compiler
    // errors get _substantially_ better if the user makes a mistake.
    pub fn push<NewPlugin: ModelMarker>(self, new_plugin: NewPlugin) -> ModelPlugins<PluginStack<NewPlugin, P>> {
        ModelPlugins(PluginStack::new(new_plugin, self.0))
    }

    /// Applies a single [`tower::Layer`] to all operations _before_ they are deserialized.
    pub fn layer<L>(self, layer: L) -> ModelPlugins<PluginStack<LayerPlugin<L>, P>> {
        ModelPlugins(PluginStack::new(LayerPlugin(layer), self.0))
    }
}

impl<Ser, Op, T, InnerPlugin> Plugin<Ser, Op, T> for ModelPlugins<InnerPlugin>
where
    InnerPlugin: Plugin<Ser, Op, T>,
{
    type Output = InnerPlugin::Output;

    fn apply(&self, input: T) -> Self::Output {
        self.0.apply(input)
    }
}

impl<InnerPlugin> ModelMarker for ModelPlugins<InnerPlugin> where InnerPlugin: ModelMarker {}

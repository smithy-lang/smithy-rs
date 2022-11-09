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
/// let composer = PluginPipeline::empty().push(LoggingPlugin).push(MetricsPlugin);
/// ```
///
/// ## Wrapping the current plugin pipeline
///
/// From time to time, you might have a need to transform the entire pipeline that has been built
/// so far - e.g. you only want to apply those plugins for a specified operation.
/// You can use the [`map`](PluginPipeline::map) method to grab the current pipeline and transform it:
///
/// ```rust
/// use aws_smithy_http_server::plugin::{FilterByOperationName, PluginPipeline};
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;
/// # struct CheckHealth;
/// # impl CheckHealth { const NAME: &'static str = "MyName"; }
///
/// let composer = PluginPipeline::new(LoggingPlugin)
///     .push(MetricsPlugin)
///     .map(|current_pipeline| {
///         // The logging and metrics plugins will not be applied to the `CheckHealth` operation.
///         FilterByOperationName::new(current_pipeline, |name| name != CheckHealth::NAME)
///     })
///     // The auth plugin will be applied to all operations
///     .push(AuthPlugin);
/// ```
///
/// ## Concatenating two plugin pipelines
///
/// `PluginPipeline` is a good way to bundle together multiple plugins, ensuring they are all
/// registered in the correct order.
///
/// You can use the [`concat`](PluginPipeline::concat) to append, at once, all the plugins
/// in another pipeline to the current pipeline:
///
/// ```rust
/// use aws_smithy_http_server::plugin::{PluginPipeline, PluginStack};
/// # use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;
/// # use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;
///
/// pub fn get_bundled_pipeline() -> PluginPipeline<PluginStack<LoggingPlugin, MetricsPlugin>> {
///     PluginPipeline::new(LoggingPlugin).push(MetricsPlugin)
/// }
///
/// let composer = PluginPipeline::new(AuthPlugin)
///     .concat(get_bundled_pipeline());
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
///     fn with_auth(self) -> PluginPipeline<PluginStack<CurrentPlugins, AuthPlugin>>;
/// }
///
/// impl<CurrentPlugins> AuthPluginExt<CurrentPlugins> for PluginPipeline<CurrentPlugins> {
///     fn with_auth(self) -> PluginPipeline<PluginStack<CurrentPlugins, AuthPlugin>> {
///         self.push(AuthPlugin)
///     }
/// }
///
/// let composer = PluginPipeline::new(LoggingPlugin)
///     // Our custom method!
///     .with_auth();
/// ```
pub struct PluginPipeline<P>(P);

impl PluginPipeline<IdentityPlugin> {
    pub fn empty() -> Self {
        Self(IdentityPlugin)
    }
}

impl<P> PluginPipeline<P> {
    pub fn new(new_plugin: P) -> PluginPipeline<P> {
        PluginPipeline(new_plugin)
    }

    pub fn push<NewPlugin>(self, new_plugin: NewPlugin) -> PluginPipeline<PluginStack<P, NewPlugin>> {
        PluginPipeline(PluginStack::new(self.0, new_plugin))
    }

    pub fn concat<OtherPlugin>(
        self,
        other_pipeline: PluginPipeline<OtherPlugin>,
    ) -> PluginPipeline<PluginStack<P, OtherPlugin>> {
        PluginPipeline(PluginStack::new(self.0, other_pipeline.0))
    }

    pub fn map<NewPlugin, F>(self, f: F) -> PluginPipeline<NewPlugin>
    where
        F: FnOnce(P) -> NewPlugin,
    {
        PluginPipeline(f(self.0))
    }

    pub fn inner(self) -> P {
        self.0
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

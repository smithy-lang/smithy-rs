/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::config_bag::ConfigBag;
use std::sync::Arc;

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait RuntimePlugin: Send + Sync + std::fmt::Debug {
    fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError>;
}

/// Sharable runtime plugin that implements `Clone` using an internal
#[derive(Clone, Debug)]
pub struct SharedRuntimePlugin(Arc<dyn RuntimePlugin>);

impl SharedRuntimePlugin {
    /// Create a new `SharedRuntimePlugin` from `RuntimePlugin`
    pub fn new(plugin: impl RuntimePlugin + 'static) -> Self {
        Self(Arc::new(plugin))
    }
}

impl<T> From<T> for SharedRuntimePlugin
where
    T: RuntimePlugin + 'static,
{
    fn from(t: T) -> Self {
        SharedRuntimePlugin::new(t) as _
    }
}

/// `RuntiemPlugins` stores collections of runtime plugins of different scope
///
/// Plugins have different scope. Plugins for an operation take precedence over those for its
/// service client as the former is more specific. `RuntimePlugins` does not enforce the specificity
/// as it is meant to be simple collections of plugins. It is up to the user of `RuntimePlugins` to
/// ensure that specificity by first calling `apply_client_configuration` and then
/// `apply_operation_configuration`.
#[derive(Clone, Debug, Default)]
pub struct RuntimePlugins {
    // In each collection, a plugin that appears later overwrites that appearing earlier in it
    // because `apply_client_configuration` (or `apply_operation_configuration`) builds
    // configuration layers into the bag as it traverses the collection from the beginning to the
    // end.
    // Furthermore, each collection stores `Arc`ed plugins as opposed to `Box`ed plugins to make
    // cloning `RuntimePlugins` more efficient.
    client_plugins: Vec<SharedRuntimePlugin>,
    operation_plugins: Vec<SharedRuntimePlugin>,
}

impl RuntimePlugins {
    /// Create a default constructed `RuntimePlugins`
    pub fn new() -> Self {
        Default::default()
    }

    /// Modify `self` by appending `plugin` to the end of client plugins
    pub fn with_client_plugin(&mut self, plugin: impl Into<SharedRuntimePlugin>) -> &mut Self {
        self.client_plugins.push(plugin.into());
        self
    }

    /// Modify `self` by appending `plugin` to the end of operation plugins
    pub fn with_operation_plugin(&mut self, plugin: impl Into<SharedRuntimePlugin>) -> &mut Self {
        self.operation_plugins.push(plugin.into());
        self
    }

    /// Apply to `cfg` each plugin in client plugins from the beginning, with the last plugin
    /// resulting in the top layer of [`ConfigBag`](crate::config_bag::ConfigBag).
    pub fn apply_client_configuration(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
        for plugin in self.client_plugins.iter() {
            plugin.0.configure(cfg)?;
        }

        Ok(())
    }

    /// Apply to `cfg` each plugin in operation plugins from the beginning, with the last plugin
    /// resulting in the top layer of [`ConfigBag`](crate::config_bag::ConfigBag).
    pub fn apply_operation_configuration(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
        for plugin in self.operation_plugins.iter() {
            plugin.0.configure(cfg)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BoxError, RuntimePlugin, RuntimePlugins};
    use crate::config_bag::ConfigBag;

    #[derive(Debug)]
    struct SomeStruct;

    impl RuntimePlugin for SomeStruct {
        fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
            todo!()
        }
    }

    #[test]
    fn can_add_runtime_plugin_implementors_to_runtime_plugins() {
        let mut rps = RuntimePlugins::new();
        rps.with_client_plugin(SomeStruct);
    }
}

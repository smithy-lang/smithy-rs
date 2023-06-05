/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::interceptors::InterceptorRegistrar;
use crate::config_bag::{ConfigBag, FrozenLayer};
use std::fmt::Debug;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxRuntimePlugin = Box<dyn RuntimePlugin + Send + Sync>;

pub trait RuntimePlugin: Debug {
    fn config(&self, current: &ConfigBag) -> Option<FrozenLayer> {
        None
    }

    fn interceptors(&self, interceptors: &mut InterceptorRegistrar) -> Result<(), BoxError> {
        Ok(())
    }
}

impl RuntimePlugin for BoxRuntimePlugin {
    fn configure(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut InterceptorRegistrar,
    ) -> Result<(), BoxError> {
        self.as_ref().configure(cfg, interceptors)
    }

    fn config(&self, current: &ConfigBag) -> Option<FrozenLayer> {
        self.as_ref().config(current)
    }

    fn interceptors(&self, interceptors: &mut InterceptorRegistrar) -> Result<(), BoxError> {
        self.as_ref().interceptors(interceptors)
    }
}

#[derive(Default)]
pub struct RuntimePlugins {
    client_plugins: Vec<BoxRuntimePlugin>,
    operation_plugins: Vec<BoxRuntimePlugin>,
}

impl RuntimePlugins {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn for_operation(operation: Box<dyn RuntimePlugin + Send + Sync>) -> Self {
        let mut plugins = Self::new();
        plugins.operation_plugins.push(operation);
        plugins
    }

    pub fn with_client_plugin(
        mut self,
        plugin: impl RuntimePlugin + Send + Sync + 'static,
    ) -> Self {
        self.client_plugins.push(Box::new(plugin));
        self
    }

    pub fn with_operation_plugin(
        mut self,
        plugin: impl RuntimePlugin + Send + Sync + 'static,
    ) -> Self {
        self.operation_plugins.push(Box::new(plugin));
        self
    }

    pub fn apply_client_configuration(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut InterceptorRegistrar,
    ) -> Result<(), BoxError> {
        for plugin in self.client_plugins.iter() {
            if let Some(layer) = plugin.config(cfg) {
                cfg.push(layer);
            }
            plugin.interceptors(interceptors)?;
        }

        Ok(())
    }

    pub fn apply_operation_configuration(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut InterceptorRegistrar,
    ) -> Result<(), BoxError> {
        for plugin in self.operation_plugins.iter() {
            if let Some(layer) = plugin.config(cfg) {
                cfg.push(layer);
            }
            plugin.interceptors(interceptors)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BoxError, RuntimePlugin, RuntimePlugins};
    use crate::client::interceptors::InterceptorRegistrar;
    use crate::config_bag::{ConfigBag, FrozenLayer};

    #[derive(Debug)]
    struct SomeStruct;

    impl RuntimePlugin for SomeStruct {
        fn configure(
            &self,
            _cfg: &mut ConfigBag,
            _inters: &mut InterceptorRegistrar,
        ) -> Result<(), BoxError> {
            todo!()
        }

        fn config(&self, current: &ConfigBag) -> Option<FrozenLayer> {
            todo!()
        }
    }

    #[test]
    fn can_add_runtime_plugin_implementors_to_runtime_plugins() {
        RuntimePlugins::new().with_client_plugin(SomeStruct);
    }
}

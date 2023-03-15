/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::config_bag::ConfigBag;

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait RuntimePlugin {
    fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError>;
}

pub struct RuntimePlugins {
    client_plugins: Vec<Box<dyn RuntimePlugin>>,
    operation_plugins: Vec<Box<dyn RuntimePlugin>>,
}

impl RuntimePlugins {
    pub fn with_client_plugin(&mut self, plugin: impl Into<Box<dyn RuntimePlugin>>) -> &mut Self {
        self.client_plugins.push(plugin.into());
        self
    }

    pub fn with_operation_plugin(
        &mut self,
        plugin: impl Into<Box<dyn RuntimePlugin>>,
    ) -> &mut Self {
        self.operation_plugins.push(plugin.into());
        self
    }

    pub fn apply_client_configuration(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
        for plugin in self.client_plugins.iter() {
            plugin.configure(cfg)?;
        }

        Ok(())
    }

    pub fn apply_operation_configuration(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
        for plugin in self.operation_plugins.iter() {
            plugin.configure(cfg)?;
        }

        Ok(())
    }
}

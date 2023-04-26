/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::interceptors::Interceptors;
use crate::client::orchestrator::{HttpRequest, HttpResponse};
use crate::config_bag::ConfigBag;

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait RuntimePlugin<Tx = HttpRequest, Rx = HttpResponse> {
    fn configure(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut Interceptors<Tx, Rx>,
    ) -> Result<(), BoxError>;
}

impl<T> From<T> for Box<dyn RuntimePlugin>
where
    T: RuntimePlugin + 'static,
{
    fn from(t: T) -> Self {
        Box::new(t) as _
    }
}

pub struct RuntimePlugins<Tx = HttpRequest, Rx = HttpResponse> {
    client_plugins: Vec<Box<dyn RuntimePlugin<Tx, Rx>>>,
    operation_plugins: Vec<Box<dyn RuntimePlugin<Tx, Rx>>>,
}

impl<Tx, Rx> Default for RuntimePlugins<Tx, Rx> {
    fn default() -> Self {
        Self {
            client_plugins: vec![],
            operation_plugins: vec![],
        }
    }
}

impl<Tx, Rx> RuntimePlugins<Tx, Rx> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_client_plugin(mut self, plugin: impl RuntimePlugin<Tx, Rx> + 'static) -> Self {
        self.client_plugins.push(Box::new(plugin));
        self
    }

    pub fn with_operation_plugin(mut self, plugin: impl RuntimePlugin<Tx, Rx> + 'static) -> Self {
        self.operation_plugins.push(Box::new(plugin));
        self
    }

    pub fn apply_client_configuration(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut Interceptors<Tx, Rx>,
    ) -> Result<(), BoxError> {
        for plugin in self.client_plugins.iter() {
            plugin.configure(cfg, interceptors)?;
        }

        Ok(())
    }

    pub fn apply_operation_configuration(
        &self,
        cfg: &mut ConfigBag,
        interceptors: &mut Interceptors<Tx, Rx>,
    ) -> Result<(), BoxError> {
        for plugin in self.operation_plugins.iter() {
            plugin.configure(cfg, interceptors)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BoxError, RuntimePlugin, RuntimePlugins};
    use crate::client::interceptors::Interceptors;
    use crate::config_bag::ConfigBag;

    #[derive(Debug)]
    struct SomeStruct;

    impl RuntimePlugin for SomeStruct {
        fn configure(
            &self,
            _cfg: &mut ConfigBag,
            _inters: &mut Interceptors,
        ) -> Result<(), BoxError> {
            todo!()
        }
    }

    #[test]
    fn can_add_runtime_plugin_implementors_to_runtime_plugins() {
        RuntimePlugins::new().with_client_plugin(SomeStruct);
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins;

mod s3 {
    pub(crate) mod config {
        use aws_smithy_async::time::TimeSource;
        use aws_smithy_runtime_api::client::interceptors::InterceptorRegistrar;
        use aws_smithy_runtime_api::client::runtime_plugin::{
            BoxError, RuntimePlugin, RuntimePlugins,
        };
        use aws_smithy_runtime_api::config_bag::{ConfigBag, FrozenLayer, Layer};
        use aws_types::region::Region;
        use aws_types::SdkConfig;

        #[derive(Debug)]
        pub struct Config {
            plugins: RuntimePlugins,
            layer: FrozenLayer,
        }

        impl RuntimePlugin for Config {
            fn config(&self, current: &ConfigBag) -> Option<FrozenLayer> {
                Some(self.layer.clone())
            }

            fn interceptors(
                &self,
                interceptors: &mut InterceptorRegistrar,
            ) -> Result<(), BoxError> {
                todo!()
            }
        }

        pub struct Builder {
            plugins: RuntimePlugins,
            layer: Layer,
        }

        impl Builder {
            pub fn region(&mut self, region: Region) {
                self.layer.store_put(region);
            }

            pub fn build(self) -> Config {
                Config {
                    plugins: self.plugins,
                    layer: self.layer.freeze(),
                }
            }
        }

        impl<'a> From<&'a SdkConfig> for Builder {
            fn from(value: &'a SdkConfig) -> Self {
                let plugins = RuntimePlugins::new()
                    // SdkConfig _is_ a RuntimePlugin
                    .with_client_plugin(value.clone());
                Builder {
                    plugins,
                    layer: Layer::new("S3ClientConfig"),
                }
            }
        }
    }
}

// elsewhere:
pub(crate) fn setup_runtime_plugins(
    config: s3::config::Config,
    runtime_plugins: &mut RuntimePlugins,
) -> RuntimePlugins {
    runtime_plugins.subregister(config.plugins());
    runtime_plugins.with_client_plugin(config)
}

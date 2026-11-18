/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The dynamic client and its builder.

use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
#[cfg(test)]
use aws_smithy_lang::ShapeId;
#[cfg(test)]
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins;
#[cfg(test)]
use aws_smithy_types::config_bag::ConfigBag;
use std::sync::Arc;

mod builder;

pub use builder::DynamicClientBuilder;

/// A client for a Smithy service, built from a model loaded at runtime.
///
/// Create one with [`DynamicClient::builder`].
///
/// ```no_run
/// use aws_smithy_dynamic_client::config::{BehaviorVersion, Config};
/// use aws_smithy_dynamic_client::DynamicClient;
///
/// let client = DynamicClient::builder()
///     .model_file("weather.json")
///     .config(Config::builder().behavior_version(BehaviorVersion::latest()))
///     .build()?;
/// # Ok::<(), aws_smithy_dynamic_client::BuildError>(())
/// ```
#[derive(Clone, Debug)]
pub struct DynamicClient {
    // The schemas of the selected service, built on first use. The set also holds the model
    // and the service ID.
    // TODO(dynamic-client): Remove this `allow` once calls read the schemas.
    #[allow(dead_code)]
    schemas: Arc<RuntimeSchemaSet>,
    // The default plugins, then the plugin for the client's config, in the order generated
    // clients apply them. The protocol and the service facts are stored in the config plugin's
    // layer.
    // TODO(dynamic-client): Remove this `allow` once calls read the runtime plugins.
    #[allow(dead_code)]
    runtime_plugins: RuntimePlugins,
}

impl DynamicClient {
    /// Returns a builder for a dynamic client.
    pub fn builder() -> DynamicClientBuilder {
        DynamicClientBuilder::default()
    }

    #[cfg(test)]
    fn service(&self) -> &ShapeId {
        self.schemas
            .service()
            .expect("the set is built for a service")
    }

    #[cfg(test)]
    fn client_config(&self) -> (ConfigBag, RuntimeComponentsBuilder) {
        let mut cfg = ConfigBag::base();
        let components = self
            .runtime_plugins
            .apply_client_configuration(&mut cfg)
            .expect("build() applied the same plugins");
        (cfg, components)
    }

    #[cfg(test)]
    fn protocol_id(&self) -> String {
        let (cfg, _) = self.client_config();
        cfg.load::<aws_smithy_schema::protocol::SharedClientProtocol>()
            .expect("build() stores the protocol")
            .protocol_id()
            .to_string()
    }
}

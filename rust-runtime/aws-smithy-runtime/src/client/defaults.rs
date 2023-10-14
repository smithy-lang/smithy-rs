/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime plugins that provide defaults for clients.
//!
//! Note: these are the absolute base-level defaults. They may not be the defaults
//! for _your_ client, since many things can change these defaults on the way to
//! code generating and constructing a full client.

use crate::client::retries::strategy::StandardRetryStrategy;
use crate::client::retries::RetryPartition;
use aws_smithy_async::rt::sleep::default_async_sleep;
use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::{
    Order, SharedRuntimePlugin, StaticRuntimePlugin,
};
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::config_bag::Layer;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use std::borrow::Cow;

/// Runtime plugin that provides a default connector.
pub fn default_http_client_plugin() -> SharedRuntimePlugin {
    let _default: Option<SharedHttpClient> = None;
    #[cfg(feature = "connector-hyper-0-14-x")]
    let _default = crate::client::http::hyper_014::default_client();

    StaticRuntimePlugin::new()
        .with_order(Order::Defaults)
        .with_runtime_components(
            RuntimeComponentsBuilder::new("default_http_client_plugin").with_http_client(_default),
        )
        .into_shared()
}

/// Runtime plugin that provides a default async sleep implementation.
pub fn default_sleep_impl_plugin() -> SharedRuntimePlugin {
    StaticRuntimePlugin::new()
        .with_order(Order::Defaults)
        .with_runtime_components(
            RuntimeComponentsBuilder::new("default_sleep_impl_plugin")
                .with_sleep_impl(default_async_sleep()),
        )
        .into_shared()
}

/// Runtime plugin that provides a default time source.
pub fn default_time_source_plugin() -> SharedRuntimePlugin {
    StaticRuntimePlugin::new()
        .with_order(Order::Defaults)
        .with_runtime_components(
            RuntimeComponentsBuilder::new("default_time_source_plugin")
                .with_time_source(Some(SystemTimeSource::new())),
        )
        .into_shared()
}

/// Runtime plugin that sets the default retry strategy, config (disabled), and partition.
pub fn default_retry_config_plugin(
    default_partition_name: impl Into<Cow<'static, str>>,
) -> SharedRuntimePlugin {
    StaticRuntimePlugin::new()
        .with_order(Order::Defaults)
        .with_runtime_components(
            RuntimeComponentsBuilder::new("default_retry_config_plugin")
                .with_retry_strategy(Some(StandardRetryStrategy::new())),
        )
        .with_config({
            let mut layer = Layer::new("default_retry_config");
            layer.store_put(RetryConfig::disabled());
            layer.store_put(RetryPartition::new(default_partition_name));
            layer.freeze()
        })
        .into_shared()
}

/// Runtime plugin that sets the default timeout config (no timeouts).
pub fn default_timeout_config_plugin() -> SharedRuntimePlugin {
    StaticRuntimePlugin::new()
        .with_order(Order::Defaults)
        .with_config({
            let mut layer = Layer::new("default_timeout_config");
            layer.store_put(TimeoutConfig::disabled());
            layer.freeze()
        })
        .into_shared()
}

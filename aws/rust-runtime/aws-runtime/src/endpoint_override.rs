/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Endpoint override detection for business metrics tracking

use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::{FrozenLayer, Layer};

use crate::sdk_feature::AwsSdkFeature;

/// Runtime plugin that tracks when a custom endpoint URL has been configured.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct EndpointOverrideRuntimePlugin {
    config: Option<FrozenLayer>,
}

impl EndpointOverrideRuntimePlugin {
    /// Creates a new `EndpointOverrideRuntimePlugin` with the endpoint override feature flag set
    pub fn new_with_feature_flag() -> Self {
        let mut layer = Layer::new("endpoint_override");
        layer.store_append(AwsSdkFeature::EndpointOverride);
        Self {
            config: Some(layer.freeze()),
        }
    }
}

impl RuntimePlugin for EndpointOverrideRuntimePlugin {
    fn config(&self) -> Option<FrozenLayer> {
        self.config.clone()
    }
}

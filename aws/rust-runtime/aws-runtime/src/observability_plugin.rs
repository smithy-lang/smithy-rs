/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime plugin for detecting observability features

use aws_smithy_observability::global::get_telemetry_provider;
use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::{FrozenLayer, Layer};
use std::borrow::Cow;

use crate::sdk_feature::AwsSdkFeature;

/// Runtime plugin that detects observability features and stores them in the config bag
#[derive(Debug, Default)]
pub struct ObservabilityRuntimePlugin;

impl ObservabilityRuntimePlugin {
    /// Create a new ObservabilityRuntimePlugin
    pub fn new() -> Self {
        Self
    }
}

impl RuntimePlugin for ObservabilityRuntimePlugin {
    fn config(&self) -> Option<FrozenLayer> {
        // Try to get the global telemetry provider
        if let Ok(telemetry_provider) = get_telemetry_provider() {
            // Check if it's a non-noop provider
            if !telemetry_provider.is_noop() {
                let mut cfg = Layer::new("ObservabilityFeatures");

                // Store ObservabilityMetrics feature (AWS-level)
                cfg.interceptor_state()
                    .store_append(AwsSdkFeature::ObservabilityMetrics);

                // PRAGMATIC ASSUMPTION:
                // If someone configured a meter provider, they likely also configured tracing
                // (both are part of observability setup). We track ObservabilityTracing based
                // on the presence of a non-noop meter provider.
                cfg.interceptor_state()
                    .store_append(SmithySdkFeature::ObservabilityTracing);

                // Check if it's OpenTelemetry
                if telemetry_provider.is_otel() {
                    cfg.interceptor_state()
                        .store_append(AwsSdkFeature::ObservabilityOtelTracing);
                    cfg.interceptor_state()
                        .store_append(AwsSdkFeature::ObservabilityOtelMetrics);
                }

                return Some(cfg.freeze());
            }
        }

        None
    }
}

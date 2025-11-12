/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Endpoint override detection for business metrics tracking

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextRef;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer};

use crate::sdk_feature::AwsSdkFeature;

/// Interceptor that detects custom endpoint URLs for business metrics
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct EndpointOverrideInterceptor;

impl EndpointOverrideInterceptor {
    /// Creates a new EndpointOverrideInterceptor
    pub fn new() -> Self {
        Self
    }
}

impl Intercept for EndpointOverrideInterceptor {
    fn name(&self) -> &'static str {
        "EndpointOverrideInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Check if endpoint_url was set in config
        if cfg
            .load::<aws_types::endpoint_config::EndpointUrl>()
            .is_some()
        {
            cfg.interceptor_state()
                .store_append(AwsSdkFeature::EndpointOverride);
        }
        Ok(())
    }
}

/// Runtime plugin that detects when a custom endpoint URL has been configured
/// and tracks it for business metrics.
///
/// This plugin is created by the codegen decorator when a user explicitly
/// sets an endpoint URL via `.endpoint_url()`. It stores the
/// `AwsSdkFeature::EndpointOverride` feature flag in the ConfigBag for
/// business metrics tracking.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct EndpointOverrideRuntimePlugin {
    config: Option<FrozenLayer>,
}

impl EndpointOverrideRuntimePlugin {
    /// Creates a new `EndpointOverrideRuntimePlugin` with the given config layer
    pub fn new(config: Option<FrozenLayer>) -> Self {
        Self { config }
    }
}

impl RuntimePlugin for EndpointOverrideRuntimePlugin {
    fn config(&self) -> Option<FrozenLayer> {
        self.config.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk_feature::AwsSdkFeature;
    use aws_smithy_runtime_api::client::interceptors::context::{
        BeforeSerializationInterceptorContextRef, Input, InterceptorContext,
    };
    use aws_smithy_types::config_bag::ConfigBag;

    #[test]
    fn test_plugin_with_no_config() {
        let plugin = EndpointOverrideRuntimePlugin::default();
        assert!(plugin.config().is_none());
    }

    #[test]
    fn test_interceptor_detects_endpoint_url_when_present() {
        let interceptor = EndpointOverrideInterceptor::new();
        let mut cfg = ConfigBag::base();

        // Set endpoint URL in config
        let endpoint_url =
            aws_types::endpoint_config::EndpointUrl("https://custom.example.com".to_string());
        cfg.interceptor_state().store_put(endpoint_url);

        // Create a dummy context
        let input = Input::doesnt_matter();
        let ctx = InterceptorContext::new(input);
        let context = BeforeSerializationInterceptorContextRef::from(&ctx);

        // Run the interceptor
        interceptor
            .read_before_execution(&context, &mut cfg)
            .unwrap();

        // Verify feature flag was set in interceptor_state
        let features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0], AwsSdkFeature::EndpointOverride);
    }

    #[test]
    fn test_interceptor_does_not_set_flag_when_endpoint_url_absent() {
        let interceptor = EndpointOverrideInterceptor::new();
        let mut cfg = ConfigBag::base();

        // Create a dummy context
        let input = Input::doesnt_matter();
        let ctx = InterceptorContext::new(input);
        let context = BeforeSerializationInterceptorContextRef::from(&ctx);

        // Run the interceptor without setting endpoint URL
        interceptor
            .read_before_execution(&context, &mut cfg)
            .unwrap();

        // Verify no feature flag was set
        let features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert_eq!(features.len(), 0);
    }
}

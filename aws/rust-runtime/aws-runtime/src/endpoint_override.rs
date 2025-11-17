/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Endpoint override detection for business metrics tracking

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Layer};

use crate::sdk_feature::AwsSdkFeature;

/// Interceptor that detects custom endpoint URLs for business metrics
///
/// This interceptor checks at runtime if a `StaticUriEndpointResolver` is configured,
/// which indicates that `.endpoint_url()` was called. When detected, it stores the
/// `AwsSdkFeature::EndpointOverride` feature flag for business metrics tracking.
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

    fn read_after_serialization(
        &self,
        _context: &BeforeTransmitInterceptorContextRef<'_>,
        runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Check if the endpoint resolver is a StaticUriEndpointResolver
        // This indicates that .endpoint_url() was called
        let resolver = runtime_components.endpoint_resolver();

        // Check the resolver's debug string to see if it's StaticUriEndpointResolver
        let debug_str = format!("{resolver:?}");

        if debug_str.contains("StaticUriEndpointResolver") {
            // Store in interceptor_state
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

    /// Creates a new `EndpointOverrideRuntimePlugin` and marks that endpoint override is enabled
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk_feature::AwsSdkFeature;

    #[test]
    fn test_plugin_with_no_config() {
        let plugin = EndpointOverrideRuntimePlugin::default();
        assert!(plugin.config().is_none());
    }

    #[test]
    fn test_plugin_with_feature_flag() {
        let plugin = EndpointOverrideRuntimePlugin::new_with_feature_flag();
        let config = plugin.config().expect("config should be set");

        // Verify the feature flag is present in the config
        let features: Vec<_> = config.load::<AwsSdkFeature>().cloned().collect();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0], AwsSdkFeature::EndpointOverride);
    }

    #[test]
    fn test_interceptor_detects_static_uri_resolver() {
        use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
        use aws_smithy_runtime_api::client::endpoint::SharedEndpointResolver;
        use aws_smithy_runtime_api::client::interceptors::context::{Input, InterceptorContext};
        use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
        use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
        use aws_smithy_types::config_bag::ConfigBag;

        // Create a StaticUriEndpointResolver
        let endpoint_resolver = SharedEndpointResolver::new(StaticUriEndpointResolver::uri(
            "https://custom.example.com",
        ));

        let mut context = InterceptorContext::new(Input::doesnt_matter());
        context.enter_serialization_phase();
        context.set_request(HttpRequest::empty());
        let _ = context.take_input();
        context.enter_before_transmit_phase();

        let rc = RuntimeComponentsBuilder::for_tests()
            .with_endpoint_resolver(Some(endpoint_resolver))
            .build()
            .unwrap();
        let mut cfg = ConfigBag::base();

        let interceptor = EndpointOverrideInterceptor::new();
        let ctx = Into::into(&context);
        interceptor
            .read_after_serialization(&ctx, &rc, &mut cfg)
            .unwrap();

        // Verify the feature flag was set
        let features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert_eq!(features.len(), 1, "Expected 1 feature, got: {features:?}");
        assert_eq!(features[0], AwsSdkFeature::EndpointOverride);
    }
}

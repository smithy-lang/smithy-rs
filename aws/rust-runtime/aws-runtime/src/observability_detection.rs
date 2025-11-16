/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Observability feature detection for business metrics tracking
//!
//! This module provides an interceptor for detecting observability features in the AWS SDK:
//!
//! - [`crate::observability_detection::ObservabilityDetectionInterceptor`]: Detects observability features during
//!   request processing and tracks them for business metrics in the User-Agent header.

use aws_smithy_observability_otel::meter::OtelMeterProvider;
use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;

/// Interceptor that detects when observability features are being used
/// and tracks them for business metrics.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ObservabilityDetectionInterceptor;

impl ObservabilityDetectionInterceptor {
    /// Creates a new `ObservabilityDetectionInterceptor`
    pub fn new() -> Self {
        Self
    }
}

impl Intercept for ObservabilityDetectionInterceptor {
    fn name(&self) -> &'static str {
        "ObservabilityDetectionInterceptor"
    }

    fn read_before_signing(
        &self,
        _context: &BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Try to get the global telemetry provider
        if let Ok(provider) = aws_smithy_observability::global::get_telemetry_provider() {
            // Use type-safe downcasting to detect OpenTelemetry meter provider
            // This works with any ProvideMeter implementation and doesn't require
            // implementation-specific boolean flags
            if provider
                .meter_provider()
                .as_any()
                .downcast_ref::<OtelMeterProvider>()
                .is_some()
            {
                // Track that observability metrics are enabled
                cfg.interceptor_state()
                    .store_append(SmithySdkFeature::ObservabilityMetrics);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk_feature::AwsSdkFeature;
    use aws_smithy_observability::TelemetryProvider;
    use aws_smithy_runtime_api::client::interceptors::context::{Input, InterceptorContext};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_types::config_bag::ConfigBag;

    #[test]
    fn test_detects_noop_provider() {
        let mut context = InterceptorContext::new(Input::doesnt_matter());
        context.enter_serialization_phase();
        context.set_request(HttpRequest::empty());
        let _ = context.take_input();
        context.enter_before_transmit_phase();

        let rc = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let mut cfg = ConfigBag::base();

        // Set a noop provider (ignore error if already set by another test)
        let _ = aws_smithy_observability::global::set_telemetry_provider(TelemetryProvider::noop());

        let interceptor = ObservabilityDetectionInterceptor::new();
        let ctx = Into::into(&context);
        interceptor
            .read_before_signing(&ctx, &rc, &mut cfg)
            .unwrap();

        // Should not track any features for noop provider since it doesn't downcast to OtelMeterProvider
        let smithy_features: Vec<_> = cfg.load::<SmithySdkFeature>().collect();
        assert_eq!(smithy_features.len(), 0);

        let aws_features: Vec<_> = cfg.load::<AwsSdkFeature>().collect();
        assert_eq!(aws_features.len(), 0);
    }

    #[test]
    fn test_custom_provider_not_detected_as_otel() {
        use aws_smithy_observability::meter::{Meter, ProvideMeter};
        use aws_smithy_observability::noop::NoopMeterProvider;
        use aws_smithy_observability::Attributes;
        use std::sync::Arc;

        // Create a custom (non-OTel, non-noop) meter provider
        // This simulates a user implementing their own metrics provider
        #[derive(Debug)]
        struct CustomMeterProvider {
            inner: NoopMeterProvider,
        }

        impl ProvideMeter for CustomMeterProvider {
            fn get_meter(&self, scope: &'static str, attributes: Option<&Attributes>) -> Meter {
                // Delegate to noop for simplicity, but this is a distinct type
                self.inner.get_meter(scope, attributes)
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let mut context = InterceptorContext::new(Input::doesnt_matter());
        context.enter_serialization_phase();
        context.set_request(HttpRequest::empty());
        let _ = context.take_input();
        context.enter_before_transmit_phase();

        let rc = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let mut cfg = ConfigBag::base();

        // Set the custom provider
        let custom_provider = Arc::new(CustomMeterProvider {
            inner: NoopMeterProvider,
        });
        let telemetry_provider = TelemetryProvider::builder()
            .meter_provider(custom_provider)
            .build();
        let _ = aws_smithy_observability::global::set_telemetry_provider(telemetry_provider);

        let interceptor = ObservabilityDetectionInterceptor::new();
        let ctx = Into::into(&context);
        interceptor
            .read_before_signing(&ctx, &rc, &mut cfg)
            .unwrap();

        // Should NOT track any features for custom provider since it doesn't downcast to OtelMeterProvider
        // The new implementation only emits metrics when OTel is detected
        let smithy_features: Vec<_> = cfg
            .interceptor_state()
            .load::<SmithySdkFeature>()
            .cloned()
            .collect();
        assert!(
            !smithy_features.iter().any(|f| *f == SmithySdkFeature::ObservabilityMetrics),
            "Should not detect custom provider as having observability metrics (only OTel is tracked)"
        );

        // Verify no AWS-specific features are tracked for custom provider
        let aws_features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert_eq!(
            aws_features.len(),
            0,
            "Should not track any AWS-specific features for custom provider"
        );
    }
}

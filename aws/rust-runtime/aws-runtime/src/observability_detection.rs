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

#[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
use crate::sdk_feature::AwsSdkFeature;
#[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
use aws_smithy_observability_otel::meter::OtelMeterProvider;
#[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
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
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        #[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
        {
            // Try to get the global telemetry provider
            if let Ok(provider) = aws_smithy_observability::global::get_telemetry_provider() {
                let meter_provider = provider.meter_provider();

                // Check if this is an OpenTelemetry meter provider
                let is_otel = meter_provider
                    .as_any()
                    .downcast_ref::<OtelMeterProvider>()
                    .is_some();

                // Check if this is a noop provider (we don't want to track noop)
                let is_noop = meter_provider
                    .as_any()
                    .downcast_ref::<aws_smithy_observability::noop::NoopMeterProvider>()
                    .is_some();

                if !is_noop {
                    // Track generic observability metrics (for any non-noop provider)
                    _cfg.interceptor_state()
                        .store_append(SmithySdkFeature::ObservabilityMetrics);

                    // If it's specifically OpenTelemetry, track that too
                    if is_otel {
                        _cfg.interceptor_state()
                            .store_append(AwsSdkFeature::ObservabilityOtelMetrics);
                    }
                }
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

    #[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
    #[test]
    #[serial_test::serial]
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

        // Should not track any features for noop provider
        let smithy_features: Vec<_> = cfg
            .interceptor_state()
            .load::<SmithySdkFeature>()
            .cloned()
            .collect();
        assert_eq!(
            smithy_features.len(),
            0,
            "Should not track Smithy features for noop provider"
        );

        let aws_features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert_eq!(
            aws_features.len(),
            0,
            "Should not track AWS features for noop provider"
        );
    }

    #[cfg(all(not(target_arch = "powerpc"), not(target_family = "wasm")))]
    #[test]
    #[serial_test::serial]
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

        // Should track generic observability metrics for custom provider
        let smithy_features: Vec<_> = cfg
            .interceptor_state()
            .load::<SmithySdkFeature>()
            .cloned()
            .collect();
        assert!(
            smithy_features.contains(&SmithySdkFeature::ObservabilityMetrics),
            "Should detect custom provider as having observability metrics"
        );

        // Should NOT track AWS-specific observability metrics for custom provider
        let aws_features: Vec<_> = cfg
            .interceptor_state()
            .load::<AwsSdkFeature>()
            .cloned()
            .collect();
        assert!(
            !aws_features.contains(&AwsSdkFeature::ObservabilityOtelMetrics),
            "Should NOT track OTel-specific metrics for custom provider"
        );
        assert_eq!(
            aws_features.len(),
            0,
            "Should not track any AWS-specific features for custom provider"
        );
    }
}

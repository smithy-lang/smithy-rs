/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_runtime::sdk_feature::AwsSdkFeature;
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::interceptors::{context::BeforeSerializationInterceptorContextRef, Intercept},
};
use aws_smithy_types::config_bag::ConfigBag;

// Interceptor that tracks AWS SDK features for observability (tracing/metrics).
#[derive(Debug, Default)]
pub(crate) struct ObservabilityFeatureTrackerInterceptor;

impl Intercept for ObservabilityFeatureTrackerInterceptor {
    fn name(&self) -> &'static str {
        "ObservabilityFeatureTrackerInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Check if an OpenTelemetry meter provider is configured via the global provider
        if let Ok(telemetry_provider) = aws_smithy_observability::global::get_telemetry_provider() {
            let meter_provider = telemetry_provider.meter_provider();

            // Use downcast to check if it's specifically the OTel implementation
            // This is more reliable than string matching on type names
            if let Some(_otel_provider) = meter_provider
                .as_any()
                .downcast_ref::<aws_smithy_observability_otel::meter::OtelMeterProvider>(
            ) {
                cfg.interceptor_state()
                    .store_append(AwsSdkFeature::ObservabilityOtelMetrics);
            }
        }

        Ok(())
    }
}

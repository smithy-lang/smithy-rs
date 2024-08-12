/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Definitions of high level Telemetry Providers.

use std::sync::Arc;

use crate::{meter::MeterProvider, noop::NoopMeterProvider};

/// A struct to hold the various types of telemetry providers.
#[non_exhaustive]
pub struct TelemetryProvider {
    meter_provider: &'static (dyn MeterProvider + Send + Sync),
}

impl TelemetryProvider {
    /// Returns a builder struct for [TelemetryProvider]
    pub fn builder() -> TelemetryProviderBuilder {
        TelemetryProviderBuilder {
            meter_provider: &NoopMeterProvider,
        }
    }

    /// Get the set [MeterProvider]
    pub fn meter_provider(&self) -> &(dyn MeterProvider + Send + Sync) {
        self.meter_provider
    }
}

impl Default for TelemetryProvider {
    fn default() -> Self {
        Self {
            meter_provider: &NoopMeterProvider,
        }
    }
}

// If we choose to expand our Telemetry provider and make Logging and Tracing
// configurable at some point in the future we can do that by adding default
// logger_provider and tracer_providers based on `tracing` to maintain backwards
// compatibilty with what we have today.
/// A builder for [TelemetryProvider].
#[non_exhaustive]
pub struct TelemetryProviderBuilder {
    meter_provider: &'static (dyn MeterProvider + Send + Sync),
}

impl TelemetryProviderBuilder {
    /// Set the [MeterProvider].
    pub fn meter_provider(&mut self, meter_provider: &'static (dyn MeterProvider + Send + Sync)) {
        self.meter_provider = meter_provider;
    }

    /// Build the [TelemetryProvider].
    pub fn build(self) -> TelemetryProvider {
        TelemetryProvider {
            meter_provider: self.meter_provider,
        }
    }
}

/// Wrapper type to hold a implementer of TelemetryProvider in an Arc so that
/// it can be safely used across threads.
#[non_exhaustive]
pub(crate) struct GlobalTelemetryProvider {
    pub(crate) telemetry_provider: Arc<TelemetryProvider>,
}

impl GlobalTelemetryProvider {
    pub(crate) fn new(telemetry_provider: TelemetryProvider) -> Self {
        Self {
            telemetry_provider: Arc::new(telemetry_provider),
        }
    }

    pub(crate) fn telemetry_provider(&self) -> &Arc<TelemetryProvider> {
        &self.telemetry_provider
    }
}

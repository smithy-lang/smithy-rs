/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Definitions of high level Telemetry Providers.

use std::sync::Arc;

use crate::{meter::ProvideMeter, noop::NoopMeterProvider};

/// A struct to hold the various types of telemetry providers.
#[non_exhaustive]
pub struct TelemetryProvider {
    meter_provider: Arc<dyn ProvideMeter + Send + Sync>,
    is_otel: bool,
}

impl TelemetryProvider {
    /// Returns a builder struct for [TelemetryProvider]
    pub fn builder() -> TelemetryProviderBuilder {
        TelemetryProviderBuilder {
            meter_provider: Arc::new(NoopMeterProvider),
            is_otel: false,
        }
    }

    /// Returns a noop [TelemetryProvider]
    pub fn noop() -> TelemetryProvider {
        Self {
            meter_provider: Arc::new(NoopMeterProvider),
            is_otel: false,
        }
    }

    /// Get the set [ProvideMeter]
    pub fn meter_provider(&self) -> &(dyn ProvideMeter + Send + Sync) {
        self.meter_provider.as_ref()
    }

    /// Returns true if this provider is using OpenTelemetry
    pub fn is_otel(&self) -> bool {
        self.is_otel
    }

    /// Returns true if this provider is a noop provider
    pub fn is_noop(&self) -> bool {
        // Check if the meter provider is the NoopMeterProvider by attempting to downcast
        self.meter_provider
            .as_any()
            .downcast_ref::<NoopMeterProvider>()
            .is_some()
    }
}

// If we choose to expand our Telemetry provider and make Logging and Tracing
// configurable at some point in the future we can do that by adding default
// logger_provider and tracer_providers based on `tracing` to maintain backwards
// compatibilty with what we have today.
impl Default for TelemetryProvider {
    fn default() -> Self {
        Self {
            meter_provider: Arc::new(NoopMeterProvider),
            is_otel: false,
        }
    }
}

/// A builder for [TelemetryProvider].
#[non_exhaustive]
pub struct TelemetryProviderBuilder {
    meter_provider: Arc<dyn ProvideMeter + Send + Sync>,
    is_otel: bool,
}

impl TelemetryProviderBuilder {
    /// Set the [ProvideMeter].
    pub fn meter_provider(mut self, meter_provider: Arc<impl ProvideMeter + 'static>) -> Self {
        self.meter_provider = meter_provider;
        self
    }

    /// Mark this provider as using OpenTelemetry.
    pub fn with_otel(mut self, is_otel: bool) -> Self {
        self.is_otel = is_otel;
        self
    }

    /// Build the [TelemetryProvider].
    pub fn build(self) -> TelemetryProvider {
        TelemetryProvider {
            meter_provider: self.meter_provider,
            is_otel: self.is_otel,
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

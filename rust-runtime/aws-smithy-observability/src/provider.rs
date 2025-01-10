/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Definitions of high level Telemetry Providers.

use std::sync::Arc;

use crate::{meter::ProvideMeter, noop::NoopMeterProvider};

/// A struct to hold the various types of telemetry providers.
#[non_exhaustive]
pub struct TelemetryProvider<PM: ProvideMeter> {
    meter_provider: PM,
}

impl<PM: ProvideMeter> TelemetryProvider<PM> {
    /// Returns a builder struct for [TelemetryProvider]
    pub fn builder() -> TelemetryProviderBuilder<PM> {
        TelemetryProviderBuilder {
            meter_provider: None,
        }
    }

    /// Returns a [TelemetryProvider] with a noop `meter_provider`
    pub fn noop() -> TelemetryProvider<NoopMeterProvider> {
        TelemetryProvider {
            meter_provider: NoopMeterProvider,
        }
    }

    /// Get a reference to the set [ProvideMeter]
    pub fn meter_provider(&self) -> &PM {
        &self.meter_provider
    }
}

// If we choose to expand our Telemetry provider and make Logging and Tracing
// configurable at some point in the future we can do that by adding default
// logger_provider and tracer_providers based on `tracing` to maintain backwards
// compatibilty with what we have today.
impl Default for TelemetryProvider<NoopMeterProvider> {
    fn default() -> Self {
        Self {
            meter_provider: NoopMeterProvider,
        }
    }
}

/// A builder for [TelemetryProvider].
// Implementation note: The meter_provider is wrapped in an Option to allow us to maintain
// the generic nature of the struct when first constructing it from TelemetryProvider::builder.
// Without the Option we would have to provide a properly typed instance of the meter_provider
// when calling the builder function.
#[non_exhaustive]
pub struct TelemetryProviderBuilder<PM: ProvideMeter> {
    meter_provider: Option<PM>,
}

impl<PM: ProvideMeter> TelemetryProviderBuilder<PM> {
    /// Set the [ProvideMeter].
    pub fn meter_provider(mut self, meter_provider: PM) -> Self {
        self.meter_provider = Some(meter_provider);
        self
    }

    /// Build the [TelemetryProvider]. Returns [None] if no `meter_provider` has been set.
    pub fn build(self) -> Option<TelemetryProvider<PM>> {
        self.meter_provider
            .map(|mp| TelemetryProvider { meter_provider: mp })
    }
}

/// Wrapper type to hold a implementer of TelemetryProvider in an Arc so that
/// it can be safely used across threads.
#[non_exhaustive]
pub(crate) struct GlobalTelemetryProvider<PM: ProvideMeter> {
    pub(crate) telemetry_provider: Arc<TelemetryProvider<PM>>,
}

impl<PM: ProvideMeter> GlobalTelemetryProvider<PM> {
    pub(crate) fn new(telemetry_provider: TelemetryProvider<PM>) -> Self {
        Self {
            telemetry_provider: Arc::new(telemetry_provider),
        }
    }

    pub(crate) fn telemetry_provider(&self) -> &Arc<TelemetryProvider<PM>> {
        &self.telemetry_provider
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::Arc;

use crate::{
    meter::{Meter, MeterProvider},
    noop::NoopMeterProvider,
    // noop::NoopMeterProvider,
};

pub trait TelemetryProvider {
    // fn tracer_provider() -> &impl TracerProvider;

    fn meter_provider(&self) -> &dyn MeterProvider;

    // fn logger_provider() -> &impl LoggerProvider;

    // fn context_manager() -> &impl ContextManager;
}

// Wrapper type to hold a implementer of TelemetryProvider in an Arc.
// The builder for GlobalTelemetryProvider will replace any not specified
// Providers with a no-op provider.
#[non_exhaustive]
pub struct GlobalTelemetryProvider {
    provider: Arc<dyn TelemetryProvider + Send + Sync>,
}

impl GlobalTelemetryProvider {
    pub(crate) fn new(telemetry_provider: impl TelemetryProvider + Send + Sync + 'static) -> Self {
        Self {
            provider: Arc::new(telemetry_provider),
        }
    }
}

#[non_exhaustive]
pub struct GlobalTelemetryProviderBuilder {
    meter_provider: &'static dyn MeterProvider,
}

impl GlobalTelemetryProviderBuilder {
    pub fn meter_provider(&mut self, meter_provider: &'static dyn MeterProvider) {
        self.meter_provider = meter_provider
    }
}

#[non_exhaustive]
pub(crate) struct DefaultTelemetryProvider {}

impl DefaultTelemetryProvider {
    pub fn new() -> Self {
        DefaultTelemetryProvider {}
    }
}

impl TelemetryProvider for DefaultTelemetryProvider {
    fn meter_provider(&self) -> &dyn MeterProvider {
        &NoopMeterProvider
    }
}

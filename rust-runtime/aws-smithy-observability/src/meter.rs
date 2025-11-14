/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics are used to gain insight into the operational performance and health of a system in
//! real time.

use crate::instruments::{
    AsyncInstrumentBuilder, AsyncMeasure, Histogram, InstrumentBuilder, MonotonicCounter,
    UpDownCounter,
};
use crate::{attributes::Attributes, instruments::ProvideInstrument};
use std::{borrow::Cow, fmt::Debug, sync::Arc};

/// Provides named instances of [Meter].
pub trait ProvideMeter: Send + Sync + Debug + 'static {
    /// Get or create a named [Meter].
    fn get_meter(&self, scope: &'static str, attributes: Option<&Attributes>) -> Meter;

    /// Downcast to `Any` for type inspection.
    ///
    /// This method enables runtime type checking via downcasting to concrete types.
    ///
    /// Implementations must return `self` to enable proper downcasting:
    ///
    /// ```ignore
    /// impl ProvideMeter for MyProvider {
    ///     fn as_any(&self) -> &dyn std::any::Any {
    ///         self
    ///     }
    /// }
    /// ```
    ///
    /// # Example Usage
    ///
    /// ```ignore
    /// use aws_smithy_observability::meter::ProvideMeter;
    /// use aws_smithy_observability_otel::meter::OtelMeterProvider;
    ///
    /// fn check_provider_type(provider: &dyn ProvideMeter) {
    ///     // Downcast to check if this is an OpenTelemetry provider
    ///     if provider.as_any().downcast_ref::<OtelMeterProvider>().is_some() {
    ///         println!("This is an OpenTelemetry provider");
    ///     }
    /// }
    /// ```
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The entry point to creating instruments. A grouping of related metrics.
#[derive(Clone)]
pub struct Meter {
    pub(crate) instrument_provider: Arc<dyn ProvideInstrument + Send + Sync>,
}

impl Meter {
    /// Create a new [Meter] from an [ProvideInstrument]
    pub fn new(instrument_provider: Arc<dyn ProvideInstrument + Send + Sync>) -> Self {
        Meter {
            instrument_provider,
        }
    }

    /// Create a new Gauge.
    #[allow(clippy::type_complexity)]
    pub fn create_gauge<F>(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: F,
    ) -> AsyncInstrumentBuilder<'_, Arc<dyn AsyncMeasure<Value = f64>>, f64>
    where
        F: Fn(&dyn AsyncMeasure<Value = f64>) + Send + Sync + 'static,
    {
        AsyncInstrumentBuilder::new(self, name.into(), Arc::new(callback))
    }

    /// Create a new [UpDownCounter].
    pub fn create_up_down_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
    ) -> InstrumentBuilder<'_, Arc<dyn UpDownCounter>> {
        InstrumentBuilder::new(self, name.into())
    }

    /// Create a new AsyncUpDownCounter.
    #[allow(clippy::type_complexity)]
    pub fn create_async_up_down_counter<F>(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: F,
    ) -> AsyncInstrumentBuilder<'_, Arc<dyn AsyncMeasure<Value = i64>>, i64>
    where
        F: Fn(&dyn AsyncMeasure<Value = i64>) + Send + Sync + 'static,
    {
        AsyncInstrumentBuilder::new(self, name.into(), Arc::new(callback))
    }

    /// Create a new [MonotonicCounter].
    pub fn create_monotonic_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
    ) -> InstrumentBuilder<'_, Arc<dyn MonotonicCounter>> {
        InstrumentBuilder::new(self, name.into())
    }

    /// Create a new AsyncMonotonicCounter.
    #[allow(clippy::type_complexity)]
    pub fn create_async_monotonic_counter<F>(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: F,
    ) -> AsyncInstrumentBuilder<'_, Arc<dyn AsyncMeasure<Value = u64>>, u64>
    where
        F: Fn(&dyn AsyncMeasure<Value = u64>) + Send + Sync + 'static,
    {
        AsyncInstrumentBuilder::new(self, name.into(), Arc::new(callback))
    }

    /// Create a new [Histogram].
    pub fn create_histogram(
        &self,
        name: impl Into<Cow<'static, str>>,
    ) -> InstrumentBuilder<'_, Arc<dyn Histogram>> {
        InstrumentBuilder::new(self, name.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noop::NoopMeterProvider;

    #[test]
    fn test_as_any_downcasting_works() {
        // Create a noop provider
        let provider = NoopMeterProvider;

        // Convert to trait object
        let provider_dyn: &dyn ProvideMeter = &provider;

        // Test that downcasting works when as_any() is properly implemented
        let downcast_result = provider_dyn.as_any().downcast_ref::<NoopMeterProvider>();
        assert!(
            downcast_result.is_some(),
            "Downcasting should succeed when as_any() returns self"
        );
    }

    /// Custom meter provider for testing extensibility.
    ///
    /// This demonstrates that users can create their own meter provider implementations
    /// and that all required types are publicly accessible.
    #[derive(Debug)]
    struct CustomMeterProvider;

    impl ProvideMeter for CustomMeterProvider {
        fn get_meter(&self, _scope: &'static str, _attributes: Option<&Attributes>) -> Meter {
            // Create a meter using NoopMeterProvider's implementation
            // This demonstrates that users can compose their own providers
            let noop_provider = NoopMeterProvider;
            noop_provider.get_meter(_scope, _attributes)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_custom_provider_extensibility() {
        // Create a custom provider
        let custom = CustomMeterProvider;
        let provider_ref: &dyn ProvideMeter = &custom;

        // Verify custom provider doesn't downcast to NoopMeterProvider
        assert!(
            provider_ref
                .as_any()
                .downcast_ref::<NoopMeterProvider>()
                .is_none(),
            "Custom provider should not downcast to NoopMeterProvider"
        );

        // Verify custom provider downcasts to its own type
        assert!(
            provider_ref
                .as_any()
                .downcast_ref::<CustomMeterProvider>()
                .is_some(),
            "Custom provider should downcast to CustomMeterProvider"
        );

        // Verify the provider can create meters (demonstrates all required types are accessible)
        let meter = custom.get_meter("test_scope", None);

        // Verify we can create instruments from the meter
        let _counter = meter.create_monotonic_counter("test_counter").build();
        let _histogram = meter.create_histogram("test_histogram").build();
        let _up_down = meter.create_up_down_counter("test_up_down").build();

        // If we got here, all required types are publicly accessible
    }
}

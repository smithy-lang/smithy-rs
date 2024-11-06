/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! OpenTelemetry based implementations of the Smithy Observability Meter traits.

use std::ops::Deref;

use crate::attributes::kv_from_option_attr;
use aws_smithy_observability::attributes::{Attributes, Context};
use aws_smithy_observability::error::{ErrorKind, ObservabilityError};
pub use aws_smithy_observability::meter::{
    AsyncMeasurement, Histogram, Meter, MeterProvider, MonotonicCounter, UpDownCounter,
};
pub use aws_smithy_observability::provider::TelemetryProvider;
use opentelemetry::metrics::{
    AsyncInstrument as OtelAsyncInstrument, Counter as OtelCounter, Histogram as OtelHistogram,
    Meter as OtelMeter, MeterProvider as OtelMeterProvider,
    ObservableCounter as OtelObservableCounter, ObservableGauge as OtelObservableGauge,
    ObservableUpDownCounter as OtelObservableUpDownCounter, UpDownCounter as OtelUpDownCounter,
};
use opentelemetry_sdk::metrics::SdkMeterProvider as OtelSdkMeterProvider;

struct UpDownCounterWrap(OtelUpDownCounter<i64>);
impl UpDownCounter for UpDownCounterWrap {
    fn add(&self, value: i64, attributes: Option<&Attributes>, _context: Option<&dyn Context>) {
        self.0.add(value, &kv_from_option_attr(attributes));
    }
}

struct HistogramWrap(OtelHistogram<f64>);
impl Histogram for HistogramWrap {
    fn record(&self, value: f64, attributes: Option<&Attributes>, _context: Option<&dyn Context>) {
        self.0.record(value, &kv_from_option_attr(attributes));
    }
}

struct MonotonicCounterWrap(OtelCounter<u64>);
impl MonotonicCounter for MonotonicCounterWrap {
    fn add(&self, value: u64, attributes: Option<&Attributes>, _context: Option<&dyn Context>) {
        self.0.add(value, &kv_from_option_attr(attributes));
    }
}

struct GaugeWrap(OtelObservableGauge<f64>);
impl AsyncMeasurement for GaugeWrap {
    type Value = f64;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        self.0.observe(value, &kv_from_option_attr(attributes));
    }

    // OTel rust does not currently support unregistering callbacks
    // https://github.com/open-telemetry/opentelemetry-rust/issues/2245
    fn stop(&self) {}
}

struct AsyncUpDownCounterWrap(OtelObservableUpDownCounter<i64>);
impl AsyncMeasurement for AsyncUpDownCounterWrap {
    type Value = i64;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        self.0.observe(value, &kv_from_option_attr(attributes));
    }

    // OTel rust does not currently support unregistering callbacks
    // https://github.com/open-telemetry/opentelemetry-rust/issues/2245
    fn stop(&self) {}
}

struct AsyncMonotonicCounterWrap(OtelObservableCounter<u64>);
impl AsyncMeasurement for AsyncMonotonicCounterWrap {
    type Value = u64;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        self.0.observe(value, &kv_from_option_attr(attributes));
    }

    // OTel rust does not currently support unregistering callbacks
    // https://github.com/open-telemetry/opentelemetry-rust/issues/2245
    fn stop(&self) {}
}

struct AsyncInstrumentWrap<'a, T>(&'a (dyn OtelAsyncInstrument<T> + Send + Sync));
impl<T> AsyncMeasurement for AsyncInstrumentWrap<'_, T> {
    type Value = T;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        self.0.observe(value, &kv_from_option_attr(attributes));
    }

    // OTel rust does not currently support unregistering callbacks
    // https://github.com/open-telemetry/opentelemetry-rust/issues/2245
    fn stop(&self) {}
}

struct MeterWrap(OtelMeter);
impl Deref for MeterWrap {
    type Target = OtelMeter;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Meter for MeterWrap {
    fn create_gauge(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = f64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn AsyncMeasurement<Value = f64>> {
        let mut builder = self.f64_observable_gauge(name).with_callback(
            move |input: &dyn OtelAsyncInstrument<f64>| {
                callback(&AsyncInstrumentWrap(input));
            },
        );

        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(GaugeWrap(builder.init()))
    }

    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn UpDownCounter> {
        let mut builder = self.i64_up_down_counter(name);
        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(UpDownCounterWrap(builder.init()))
    }

    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = i64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn AsyncMeasurement<Value = i64>> {
        let mut builder = self.i64_observable_up_down_counter(name).with_callback(
            move |input: &dyn OtelAsyncInstrument<i64>| {
                callback(&AsyncInstrumentWrap(input));
            },
        );

        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(AsyncUpDownCounterWrap(builder.init()))
    }

    fn create_monotonic_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn MonotonicCounter> {
        let mut builder = self.u64_counter(name);
        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(MonotonicCounterWrap(builder.init()))
    }

    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = u64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn AsyncMeasurement<Value = u64>> {
        let mut builder = self.u64_observable_counter(name).with_callback(
            move |input: &dyn OtelAsyncInstrument<u64>| {
                callback(&AsyncInstrumentWrap(input));
            },
        );

        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(AsyncMonotonicCounterWrap(builder.init()))
    }

    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn Histogram> {
        let mut builder = self.f64_histogram(name);
        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u);
        }

        Box::new(HistogramWrap(builder.init()))
    }
}

/// An OpenTelemetry based implementation of the AWS SDK's [MeterProvider] trait
#[non_exhaustive]
#[derive(Debug)]
pub struct AwsSdkOtelMeterProvider {
    meter_provider: OtelSdkMeterProvider,
}

impl AwsSdkOtelMeterProvider {
    /// Create a new [AwsSdkOtelMeterProvider] from an [OtelSdkMeterProvider].
    pub fn new(otel_meter_provider: OtelSdkMeterProvider) -> Self {
        Self {
            meter_provider: otel_meter_provider,
        }
    }
}

impl MeterProvider for AwsSdkOtelMeterProvider {
    fn get_meter(&self, scope: &'static str, _attributes: Option<&Attributes>) -> Box<dyn Meter> {
        Box::new(MeterWrap(self.meter_provider.meter(scope)))
    }

    fn flush(&self) -> Result<(), ObservabilityError> {
        match self.meter_provider.force_flush() {
            Ok(_) => Ok(()),
            Err(err) => Err(ObservabilityError::new(ErrorKind::MetricsFlush, err)),
        }
    }

    fn shutdown(&self) -> Result<(), ObservabilityError> {
        match self.meter_provider.force_flush() {
            Ok(_) => Ok(()),
            Err(err) => Err(ObservabilityError::new(ErrorKind::MetricsShutdown, err)),
        }
    }
}

#[cfg(test)]
mod tests {

    use aws_smithy_observability::attributes::{AttributeValue, Attributes};
    use aws_smithy_observability::meter::AsyncMeasurement;
    use aws_smithy_observability::provider::TelemetryProvider;
    use opentelemetry_sdk::metrics::{
        data::{Gauge, Histogram, Sum},
        PeriodicReader, SdkMeterProvider,
    };
    use opentelemetry_sdk::runtime::Tokio;
    use opentelemetry_sdk::testing::metrics::InMemoryMetricsExporter;

    use super::AwsSdkOtelMeterProvider;

    // Without these tokio settings this test just stalls forever on flushing the metrics pipeline
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn sync_instruments_work() {
        // Create the OTel metrics objects
        let exporter = InMemoryMetricsExporter::default();
        let reader = PeriodicReader::builder(exporter.clone(), Tokio).build();
        let otel_mp = SdkMeterProvider::builder().with_reader(reader).build();

        // Create the SDK metrics types from the OTel objects
        let sdk_mp = AwsSdkOtelMeterProvider::new(otel_mp);
        let sdk_tp = TelemetryProvider::builder()
            .meter_provider(Box::new(sdk_mp))
            .build();

        // Get the dyn versions of the SDK metrics objects
        let dyn_sdk_mp = sdk_tp.meter_provider();
        let dyn_sdk_meter = dyn_sdk_mp.get_meter("TestMeter", None);

        //Create all 3 sync instruments and record some data for each
        let mono_counter =
            dyn_sdk_meter.create_monotonic_counter("TestMonoCounter".to_string(), None, None);
        mono_counter.add(4, None, None);
        let ud_counter =
            dyn_sdk_meter.create_up_down_counter("TestUpDownCounter".to_string(), None, None);
        ud_counter.add(-6, None, None);
        let histogram = dyn_sdk_meter.create_histogram("TestHistogram".to_string(), None, None);
        histogram.record(1.234, None, None);

        // Gracefully shutdown the metrics provider so all metrics are flushed through the pipeline
        dyn_sdk_mp.shutdown().unwrap();

        // Extract the metrics from the exporter and assert that they are what we expect
        let finished_metrics = exporter.get_finished_metrics().unwrap();
        let extracted_mono_counter_data = &finished_metrics[0].scope_metrics[0].metrics[0]
            .data
            .as_any()
            .downcast_ref::<Sum<u64>>()
            .unwrap()
            .data_points[0]
            .value;
        assert_eq!(extracted_mono_counter_data, &4);

        let extracted_ud_counter_data = &finished_metrics[0].scope_metrics[0].metrics[1]
            .data
            .as_any()
            .downcast_ref::<Sum<i64>>()
            .unwrap()
            .data_points[0]
            .value;
        assert_eq!(extracted_ud_counter_data, &-6);

        let extracted_histogram_data = &finished_metrics[0].scope_metrics[0].metrics[2]
            .data
            .as_any()
            .downcast_ref::<Histogram<f64>>()
            .unwrap()
            .data_points[0]
            .sum;
        assert_eq!(extracted_histogram_data, &1.234);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn async_instrument_work() {
        // Create the OTel metrics objects
        let exporter = InMemoryMetricsExporter::default();
        let reader = PeriodicReader::builder(exporter.clone(), Tokio).build();
        let otel_mp = SdkMeterProvider::builder().with_reader(reader).build();

        // Create the SDK metrics types from the OTel objects
        let sdk_mp = AwsSdkOtelMeterProvider::new(otel_mp);
        let sdk_tp = TelemetryProvider::builder()
            .meter_provider(Box::new(sdk_mp))
            .build();

        // Get the dyn versions of the SDK metrics objects
        let dyn_sdk_mp = sdk_tp.meter_provider();
        let dyn_sdk_meter = dyn_sdk_mp.get_meter("TestMeter", None);

        //Create all async instruments and record some data
        let gauge = dyn_sdk_meter.create_gauge(
            "TestGauge".to_string(),
            // Callback function records another value with different attributes so it is deduped
            Box::new(|measurement: &dyn AsyncMeasurement<Value = f64>| {
                let mut attrs = Attributes::new();
                attrs.set(
                    "TestGaugeAttr".into(),
                    AttributeValue::String("TestGaugeAttr".into()),
                );
                measurement.record(6.789, Some(&attrs), None);
            }),
            None,
            None,
        );
        gauge.record(1.234, None, None);

        let async_ud_counter = dyn_sdk_meter.create_async_up_down_counter(
            "TestAsyncUpDownCounter".to_string(),
            Box::new(|measurement: &dyn AsyncMeasurement<Value = i64>| {
                let mut attrs = Attributes::new();
                attrs.set(
                    "TestAsyncUpDownCounterAttr".into(),
                    AttributeValue::String("TestAsyncUpDownCounterAttr".into()),
                );
                measurement.record(12, Some(&attrs), None);
            }),
            None,
            None,
        );
        async_ud_counter.record(-6, None, None);

        let async_mono_counter = dyn_sdk_meter.create_async_monotonic_counter(
            "TestAsyncMonoCounter".to_string(),
            Box::new(|measurement: &dyn AsyncMeasurement<Value = u64>| {
                let mut attrs = Attributes::new();
                attrs.set(
                    "TestAsyncMonoCounterAttr".into(),
                    AttributeValue::String("TestAsyncMonoCounterAttr".into()),
                );
                measurement.record(123, Some(&attrs), None);
            }),
            None,
            None,
        );
        async_mono_counter.record(4, None, None);

        // Gracefully shutdown the metrics provider so all metrics are flushed through the pipeline
        dyn_sdk_mp.flush().unwrap();
        dyn_sdk_mp.shutdown().unwrap();

        // Extract the metrics from the exporter
        let finished_metrics = exporter.get_finished_metrics().unwrap();

        // Assert that the reported metrics are what we expect
        let extracted_gauge_data = &finished_metrics[0].scope_metrics[0].metrics[0]
            .data
            .as_any()
            .downcast_ref::<Gauge<f64>>()
            .unwrap()
            .data_points[0]
            .value;
        assert_eq!(extracted_gauge_data, &1.234);

        let extracted_async_ud_counter_data = &finished_metrics[0].scope_metrics[0].metrics[1]
            .data
            .as_any()
            .downcast_ref::<Sum<i64>>()
            .unwrap()
            .data_points[0]
            .value;
        assert_eq!(extracted_async_ud_counter_data, &-6);

        let extracted_async_mono_data = &finished_metrics[0].scope_metrics[0].metrics[2]
            .data
            .as_any()
            .downcast_ref::<Sum<u64>>()
            .unwrap()
            .data_points[0]
            .value;
        assert_eq!(extracted_async_mono_data, &4);

        // Assert that the async callbacks ran
        let finished_metrics = exporter.get_finished_metrics().unwrap();
        let extracted_gauge_data = &finished_metrics[0].scope_metrics[0].metrics[0]
            .data
            .as_any()
            .downcast_ref::<Gauge<f64>>()
            .unwrap()
            .data_points[1]
            .value;
        assert_eq!(extracted_gauge_data, &6.789);

        let extracted_async_ud_counter_data = &finished_metrics[0].scope_metrics[0].metrics[1]
            .data
            .as_any()
            .downcast_ref::<Sum<i64>>()
            .unwrap()
            .data_points[1]
            .value;
        assert_eq!(extracted_async_ud_counter_data, &12);

        let extracted_async_mono_data = &finished_metrics[0].scope_metrics[0].metrics[2]
            .data
            .as_any()
            .downcast_ref::<Sum<u64>>()
            .unwrap()
            .data_points[1]
            .value;
        assert_eq!(extracted_async_mono_data, &123);
    }
}

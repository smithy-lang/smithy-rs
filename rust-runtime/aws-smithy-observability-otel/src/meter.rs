/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! OpenTelemetry based implementations of the Smithy Observability Meter traits.

use std::{ops::Deref, sync::Arc};

use crate::attributes::{kv_from_option_attr, option_attr_from_kv};
use aws_smithy_observability::attributes::{Attributes, Context, Double, Long, UnsignedLong};
pub use aws_smithy_observability::meter::{
    AsyncMeasurement, AsyncMeasurementHandle, Histogram, Meter, MeterProvider, MonotonicCounter,
    UpDownCounter,
};
use opentelemetry::metrics::{
    AsyncInstrument, Counter as OtelCounter, Histogram as OtelHistogram, Meter as OtelMeter,
    ObservableCounter, ObservableGauge, ObservableUpDownCounter,
    UpDownCounter as OtelUpDownCounter,
};
// use opentelemetry_sdk::metrics::{
//     SdkMeter as OtelSdkMeter, SdkMeterProvider as OtelSdkMeterProvider,
// };

struct UpDownCounterWrap(OtelUpDownCounter<i64>);
impl Deref for UpDownCounterWrap {
    type Target = OtelUpDownCounter<i64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl UpDownCounter for UpDownCounterWrap {
    fn add(&self, value: Long, attributes: Option<&Attributes>, _context: Option<&dyn Context>) {
        let _ = &self.0.add(value, &kv_from_option_attr(attributes));
    }
}

struct HistogramWrap(OtelHistogram<f64>);
impl Deref for HistogramWrap {
    type Target = OtelHistogram<f64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Histogram for HistogramWrap {
    fn record(
        &self,
        value: Double,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        let _ = &self.0.record(value, &kv_from_option_attr(attributes));
    }
}

struct MonotonicCounterWrap(OtelCounter<u64>);
impl Deref for MonotonicCounterWrap {
    type Target = OtelCounter<u64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl MonotonicCounter for MonotonicCounterWrap {
    fn add(
        &self,
        value: UnsignedLong,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        let _ = &self.0.add(value, &kv_from_option_attr(attributes));
    }
}

struct GaugeWrap(ObservableGauge<f64>);
impl Deref for GaugeWrap {
    type Target = ObservableGauge<f64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsyncMeasurement for GaugeWrap {
    type Value = Double;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        let _ = &self.0.observe(value, &kv_from_option_attr(attributes));
    }
}

struct AsyncUpDownCounterWrap(ObservableUpDownCounter<i64>);
impl Deref for AsyncUpDownCounterWrap {
    type Target = ObservableUpDownCounter<i64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsyncMeasurement for AsyncUpDownCounterWrap {
    type Value = Long;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        let _ = &self.0.observe(value, &kv_from_option_attr(attributes));
    }
}

struct AsyncMonotonicCounterWrap(ObservableCounter<u64>);
impl Deref for AsyncMonotonicCounterWrap {
    type Target = ObservableCounter<u64>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsyncMeasurement for AsyncMonotonicCounterWrap {
    type Value = UnsignedLong;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        let _ = &self.0.observe(value, &kv_from_option_attr(attributes));
    }
}

struct MeterWrap(OtelMeter);
impl Deref for MeterWrap {
    type Target = OtelMeter;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct OtelAsyncInstrument<'a, T>(&'a (dyn AsyncInstrument<T> + Send + Sync));
impl<T> AsyncMeasurement for OtelAsyncInstrument<'_, T> {
    type Value = T;

    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
        &self.0.observe(value, &kv_from_option_attr(attributes));
    }
}

struct OtelObservableGauge<T>(ObservableGauge<T>);
impl<T> AsyncMeasurementHandle for OtelObservableGauge<T> {
    // Otel Rust doesn't appear to support unregistering callbacks yet so this is a noop for now
    // https://github.com/open-telemetry/opentelemetry-rust/issues/2245
    fn stop(&self) {}
}

impl Meter for MeterWrap {
    fn create_gauge(
        &self,
        name: String,
        // TODO(smithyObservability): compare this definition to the Boxed version below
        // callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Double>>)>,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Double>>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn AsyncMeasurementHandle> {
        // let help = move |foo: &dyn AsyncInstrument<f64>| {
        //     let blah = Box::new(OtelAsyncInstrument(foo));
        //     callback(blah);
        // };

        //TODO(smithyObservability): figure out the typing for the callback here
        let mut builder = self.0.f64_observable_gauge(name).with_callback(
            move |foo: &dyn AsyncInstrument<f64>| {
                let blah = Box::new(OtelAsyncInstrument(foo));
                callback(blah);
            },
        );

        if let Some(desc) = description {
            builder = builder.with_description(desc);
        }

        if let Some(u) = units {
            builder = builder.with_unit(u)
        }

        let gauge = builder.init();

        Box::new(OtelObservableGauge(gauge))
    }

    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn UpDownCounter {
        todo!()
    }

    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Long>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle {
        todo!()
    }

    fn create_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn MonotonicCounter {
        todo!()
    }

    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = UnsignedLong>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle {
        todo!()
    }

    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn Histogram {
        todo!()
    }
}

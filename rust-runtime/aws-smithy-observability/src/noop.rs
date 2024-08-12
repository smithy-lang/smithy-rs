/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! An noop implementation of the Meter traits

use crate::{
    attributes::{Attributes, Context, Double, Long, UnsignedLong},
    meter::{
        AsyncMeasurement, AsyncMeasurementHandle, Histogram, Meter, MeterProvider,
        MonotonicCounter, UpDownCounter,
    },
};

pub(crate) struct NoopMeterProvider;
impl MeterProvider for NoopMeterProvider {
    fn get_meter(&self, _scope: String, _attributes: Option<&Attributes>) -> &dyn Meter {
        &NoopMeter
    }
}

pub(crate) struct NoopMeter;
impl Meter for NoopMeter {
    fn create_gauge(
        &self,
        _name: String,
        _callback: &dyn Fn(&dyn AsyncMeasurement<Value = Double>),
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle {
        &NoopAsyncMeasurementHandle
    }

    fn create_up_down_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn UpDownCounter {
        &NoopUpDownCounter
    }

    fn create_async_up_down_counter(
        &self,
        _name: String,
        _callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Long>>)>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle {
        &NoopAsyncMeasurementHandle
    }

    fn create_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn MonotonicCounter {
        &NoopMonotonicCounter
    }

    fn create_async_monotonic_counter(
        &self,
        _name: String,
        _callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = UnsignedLong>>)>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle {
        &NoopAsyncMeasurementHandle
    }

    fn create_histogram(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> &dyn Histogram {
        &NoopHistogram
    }
}

struct NoopAsyncMeasurementHandle;
impl AsyncMeasurementHandle for NoopAsyncMeasurementHandle {
    fn stop(&self) {}
}

struct NoopUpDownCounter;
impl UpDownCounter for NoopUpDownCounter {
    fn add(&self, _value: Long, _attributes: Option<&Attributes>, _context: Option<&dyn Context>) {}
}

struct NoopMonotonicCounter;
impl MonotonicCounter for NoopMonotonicCounter {
    fn add(
        &self,
        _value: UnsignedLong,
        _attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
    }
}

struct NoopHistogram;
impl Histogram for NoopHistogram {
    fn record(
        &self,
        _value: Double,
        _attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
    }
}

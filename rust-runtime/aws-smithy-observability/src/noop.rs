/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! An noop implementation of the Meter traits

use std::any::Any;

use crate::{
    attributes::{Attributes, Context, Double, Long, UnsignedLong},
    meter::{AsyncMeasurement, Histogram, Meter, MeterProvider, MonotonicCounter, UpDownCounter},
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
        _callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = Double>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasurement<Value = Double>> {
        Box::new(NoopAsyncMeasurement)
    }

    fn create_up_down_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn UpDownCounter> {
        Box::new(NoopUpDownCounter)
    }

    fn create_async_up_down_counter(
        &self,
        _name: String,
        _callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = Long>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasurement> {
        Box::new(NoopAsyncMeasurement)
    }

    fn create_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn MonotonicCounter> {
        Box::new(NoopMonotonicCounter)
    }

    fn create_async_monotonic_counter(
        &self,
        _name: String,
        _callback: Box<dyn Fn(&dyn AsyncMeasurement<Value = UnsignedLong>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasurement> {
        Box::new(NoopAsyncMeasurement)
    }

    fn create_histogram(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn Histogram> {
        Box::new(NoopHistogram)
    }
}

struct NoopAsyncMeasurement;
impl<T> AsyncMeasurement for NoopAsyncMeasurement {
    type Value = T;

    fn record(&self, _value: T, _attributes: Option<&Attributes>, _context: Option<&dyn Context>) {}

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

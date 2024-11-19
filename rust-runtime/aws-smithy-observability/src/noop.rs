/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! An noop implementation of the Meter traits

use std::fmt::Debug;
use std::marker::PhantomData;

use crate::{
    attributes::{Attributes, Context},
    meter::{AsyncMeasure, Histogram, Meter, MonotonicCounter, ProvideMeter, UpDownCounter},
};

#[derive(Debug)]
pub(crate) struct NoopMeterProvider;
impl ProvideMeter for NoopMeterProvider {
    fn get_meter(&self, _scope: &'static str, _attributes: Option<&Attributes>) -> Box<dyn Meter> {
        Box::new(NoopMeter)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
pub(crate) struct NoopMeter;
impl Meter for NoopMeter {
    fn create_gauge(
        &self,
        _name: String,
        _callback: Box<dyn Fn(&dyn AsyncMeasure<Value = f64>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasure<Value = f64>> {
        Box::new(NoopAsyncMeasurement(PhantomData::<f64>))
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
        _callback: Box<dyn Fn(&dyn AsyncMeasure<Value = i64>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasure<Value = i64>> {
        Box::new(NoopAsyncMeasurement(PhantomData::<i64>))
    }

    fn create_monotonic_counter(
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
        _callback: Box<dyn Fn(&dyn AsyncMeasure<Value = u64>) + Send + Sync>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Box<dyn AsyncMeasure<Value = u64>> {
        Box::new(NoopAsyncMeasurement(PhantomData::<u64>))
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

#[derive(Debug)]
struct NoopAsyncMeasurement<T: Send + Sync + Debug>(PhantomData<T>);
impl<T: Send + Sync + Debug> AsyncMeasure for NoopAsyncMeasurement<T> {
    type Value = T;

    fn record(&self, _value: T, _attributes: Option<&Attributes>, _context: Option<&dyn Context>) {}

    fn stop(&self) {}
}

#[derive(Debug)]
struct NoopUpDownCounter;
impl UpDownCounter for NoopUpDownCounter {
    fn add(&self, _value: i64, _attributes: Option<&Attributes>, _context: Option<&dyn Context>) {}
}

#[derive(Debug)]
struct NoopMonotonicCounter;
impl MonotonicCounter for NoopMonotonicCounter {
    fn add(&self, _value: u64, _attributes: Option<&Attributes>, _context: Option<&dyn Context>) {}
}

#[derive(Debug)]
struct NoopHistogram;
impl Histogram for NoopHistogram {
    fn record(
        &self,
        _value: f64,
        _attributes: Option<&Attributes>,
        _context: Option<&dyn Context>,
    ) {
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! An noop implementation of the Meter traits

use std::marker::PhantomData;
use std::{fmt::Debug, sync::Arc};

use crate::{
    attributes::{Attributes, Context, Scope},
    meter::{AsyncMeasure, Histogram, Meter, MonotonicCounter, ProvideMeter, UpDownCounter},
};

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopMeterProvider;
impl ProvideMeter for NoopMeterProvider {
    type Meter = NoopMeter;

    fn get_meter(
        &self,
        _scope: &'static str,
        _attributes: Option<&Attributes>,
    ) -> Arc<Self::Meter> {
        Arc::new(NoopMeter)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopMeter;
impl Meter for NoopMeter {
    type Context = NoopContext;

    type Gauge = NoopAsyncMeasurement<f64>;

    type GaugeCallback<'a> = fn(&NoopAsyncMeasurement<f64>);

    type GaugeCallbackInput<'a> = NoopAsyncMeasurement<f64>;

    type UpDownCounter = NoopUpDownCounter;

    type AsyncUDC = NoopAsyncMeasurement<i64>;

    type AsyncUDCCallback<'a> = fn(&NoopAsyncMeasurement<i64>);

    type AsyncUDCCallbackInput<'a> = NoopAsyncMeasurement<i64>;

    type MonotonicCounter = NoopMonotonicCounter;

    type AsyncMC = NoopAsyncMeasurement<u64>;

    type AsyncMCCallback<'a> = fn(&NoopAsyncMeasurement<u64>);

    type AsyncMCCallbackInput<'a> = NoopAsyncMeasurement<u64>;

    type Histogram = NoopHistogram;

    fn create_gauge(
        &self,
        _name: String,
        _callback: Self::GaugeCallback<'_>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopAsyncMeasurement<f64>> {
        Arc::new(NoopAsyncMeasurement(PhantomData::<f64>))
    }

    fn create_up_down_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopUpDownCounter> {
        Arc::new(NoopUpDownCounter)
    }

    fn create_async_up_down_counter(
        &self,
        _name: String,
        _callback: Self::AsyncUDCCallback<'_>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopAsyncMeasurement<i64>> {
        Arc::new(NoopAsyncMeasurement(PhantomData::<i64>))
    }

    fn create_monotonic_counter(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopMonotonicCounter> {
        Arc::new(NoopMonotonicCounter)
    }

    fn create_async_monotonic_counter(
        &self,
        _name: String,
        _callback: Self::AsyncMCCallback<'_>,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopAsyncMeasurement<u64>> {
        Arc::new(NoopAsyncMeasurement(PhantomData::<u64>))
    }

    fn create_histogram(
        &self,
        _name: String,
        _units: Option<String>,
        _description: Option<String>,
    ) -> Arc<NoopHistogram> {
        Arc::new(NoopHistogram)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopAsyncMeasurement<T: Send + Sync + Debug>(PhantomData<T>);
impl<T: Send + Sync + Debug> AsyncMeasure for NoopAsyncMeasurement<T> {
    type Value = T;
    type Context = NoopContext;

    fn record(&self, _value: T, _attributes: Option<&Attributes>, _context: Option<&NoopContext>) {}

    fn stop(&self) {}
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopUpDownCounter;
impl UpDownCounter for NoopUpDownCounter {
    type Context = NoopContext;

    fn add(&self, _value: i64, _attributes: Option<&Attributes>, _context: Option<&NoopContext>) {}
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopMonotonicCounter;
impl MonotonicCounter for NoopMonotonicCounter {
    type Context = NoopContext;

    fn add(&self, _value: u64, _attributes: Option<&Attributes>, _context: Option<&NoopContext>) {}
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopHistogram;
impl Histogram for NoopHistogram {
    type Context = NoopContext;

    fn record(
        &self,
        _value: f64,
        _attributes: Option<&Attributes>,
        _context: Option<&NoopContext>,
    ) {
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopContext;
impl Context for NoopContext {
    type Scope = NoopScope;
    fn make_current(&self) -> &NoopScope {
        &NoopScope
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct NoopScope;
impl Scope for NoopScope {
    fn end(&self) {}
}

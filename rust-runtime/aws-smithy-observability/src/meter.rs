/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics are used to gain insight into the operational performance and health of a system in
//! real time.

use crate::attributes::{Attributes, Context, ContextGeneric};
use std::{fmt::Debug, sync::Arc};

/// Provides named instances of [Meter].
pub trait ProvideMeter: Send + Sync + Debug {
    /// Get or create a named [Meter].
    fn get_meter(&self, scope: &'static str, attributes: Option<&Attributes>) -> Arc<dyn Meter>;

    /// Cast to [std::any::Any]
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The entry point to creating instruments. A grouping of related metrics.
pub trait Meter: Send + Sync + Debug {
    /// Create a new Gauge.
    #[allow(clippy::type_complexity)]
    fn create_gauge(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = f64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn AsyncMeasure<Value = f64>>;

    /// Create a new [UpDownCounter].
    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn UpDownCounter>;

    /// Create a new AsyncUpDownCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = i64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn AsyncMeasure<Value = i64>>;

    /// Create a new [MonotonicCounter].
    fn create_monotonic_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn MonotonicCounter>;

    /// Create a new AsyncMonotonicCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = u64>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn AsyncMeasure<Value = u64>>;

    /// Create a new [Histogram].
    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<dyn Histogram>;
}

/// Collects a set of events with an event count and sum for all events.
pub trait Histogram: Send + Sync + Debug {
    /// Record a value.
    fn record(&self, value: f64, attributes: Option<&Attributes>, context: Option<&dyn Context>);
}

/// A counter that monotonically increases.
pub trait MonotonicCounter: Send + Sync + Debug {
    /// Increment a counter by a fixed amount.
    fn add(&self, value: u64, attributes: Option<&Attributes>, context: Option<&dyn Context>);
}

/// A counter that can increase or decrease.
pub trait UpDownCounter: Send + Sync + Debug {
    /// Increment or decrement a counter by a fixed amount.
    fn add(&self, value: i64, attributes: Option<&Attributes>, context: Option<&dyn Context>);
}

/// A measurement that can be taken asynchronously.
pub trait AsyncMeasure: Send + Sync + Debug {
    /// The type recorded by the measurement.
    type Value;

    /// Record a value
    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        context: Option<&dyn Context>,
    );

    /// Stop recording, unregister callback.
    fn stop(&self);
}

/// Foo Histogram
pub trait HistogramGeneric<C>: Send + Sync + Debug
where
    C: ContextGeneric,
{
    /// Record a value.
    fn record(&self, value: f64, attributes: Option<&Attributes>, context: Option<&C>);
}

/// A counter that monotonically increases.
pub trait MonotonicCounterGeneric<C>: Send + Sync + Debug
where
    C: ContextGeneric,
{
    /// Increment a counter by a fixed amount.
    fn add(&self, value: u64, attributes: Option<&Attributes>, context: Option<&C>);
}

/// A counter that can increase or decrease.
pub trait UpDownCounterGeneric<C>: Send + Sync + Debug
where
    C: ContextGeneric,
{
    /// Increment or decrement a counter by a fixed amount.
    fn add(&self, value: i64, attributes: Option<&Attributes>, context: Option<&C>);
}

/// Foo
pub trait AsyncMeasureGeneric<C, T>: Send + Sync + Debug
where
    C: ContextGeneric,
{
    /// The type recorded by the measurement.
    // type Value = T;

    /// Record a value
    fn record(&self, value: T, attributes: Option<&Attributes>, context: Option<&C>);

    /// Stop recording, unregister callback.
    fn stop(&self);
}

/// Worlds longest where clause
pub trait MeterGeneric: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: ContextGeneric;

    /// A type implementing [AsyncMeasureGeneric] for [f64]
    type Gauge: AsyncMeasureGeneric<Self::Context, f64>;
    /// The type of the callback function passed when creating a [MeterGeneric::Gauge]
    type GaugeCallback<'a>: Fn(&Self::GaugeCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [MeterGeneric::GaugeCallback]
    type GaugeCallbackInput<'a>: AsyncMeasureGeneric<Self::Context, f64>;

    /// A type implementing [UpDownCounterGeneric]
    type UpDownCounter: UpDownCounterGeneric<Self::Context>;

    /// A type implementing [AsyncMeasureGeneric] for [i64]
    type AsyncUDC: AsyncMeasureGeneric<Self::Context, i64>;
    /// The type of the callback function passed when creating a [MeterGeneric::AsyncUDC]
    type AsyncUDCCallback<'a>: Fn(&Self::AsyncUDCCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [MeterGeneric::AsyncUDCCallback]
    type AsyncUDCCallbackInput<'a>: AsyncMeasureGeneric<Self::Context, i64>;

    /// A type implementing [MonotonicCounterGeneric]
    type MonotonicCounter: MonotonicCounterGeneric<Self::Context>;

    /// A type implementing [AsyncMeasureGeneric] for [u64]
    type AsyncMC: AsyncMeasureGeneric<Self::Context, u64>;
    /// The type of the callback function passed when creating a [MeterGeneric::AsyncMC]
    type AsyncMCCallback<'a>: Fn(&Self::AsyncMCCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [MeterGeneric::AsyncMCCallback]
    type AsyncMCCallbackInput<'a>: AsyncMeasureGeneric<Self::Context, u64>;

    /// A type implementing [HistogramGeneric]
    type Histogram: HistogramGeneric<Self::Context>;

    /// Create a new Gauge.
    #[allow(clippy::type_complexity)]
    fn create_gauge(
        &self,
        name: String,
        callback: Self::GaugeCallback<'_>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::Gauge>;

    /// Create a new [UpDownCounter].
    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::UpDownCounter>;

    /// Create a new AsyncUpDownCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Self::AsyncUDCCallback<'_>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::AsyncUDC>;

    /// Create a new [MonotonicCounter].
    fn create_monotonic_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::MonotonicCounter>;

    /// Create a new AsyncMonotonicCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Self::AsyncMCCallback<'_>,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::AsyncMC>;

    /// Create a new [Histogram].
    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> Arc<Self::Histogram>;
}

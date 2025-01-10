/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics are used to gain insight into the operational performance and health of a system in
//! real time.

use crate::attributes::{Attributes, Context};
use std::{fmt::Debug, sync::Arc};

/// Provides named instances of [Meter].
pub trait ProvideMeter: Send + Sync + Debug {
    /// A type implementing the [Meter] trait.
    type Meter: Meter;
    /// Get or create a named [Meter].
    fn get_meter(&self, scope: &'static str, attributes: Option<&Attributes>) -> Arc<Self::Meter>;

    /// Cast to [std::any::Any]
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Collects a set of events with an event count and sum for all events.
pub trait Histogram: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: Context;
    /// Record a value.
    fn record(&self, value: f64, attributes: Option<&Attributes>, context: Option<&Self::Context>);
}

/// A counter that monotonically increases.
pub trait MonotonicCounter: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: Context;
    /// Increment a counter by a fixed amount.
    fn add(&self, value: u64, attributes: Option<&Attributes>, context: Option<&Self::Context>);
}

/// A counter that can increase or decrease.
pub trait UpDownCounter: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: Context;
    /// Increment or decrement a counter by a fixed amount.
    fn add(&self, value: i64, attributes: Option<&Attributes>, context: Option<&Self::Context>);
}

/// Foo
pub trait AsyncMeasure: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: Context;

    /// The type of the value recorded by this instrument
    type Value;

    /// Record a value
    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        context: Option<&Self::Context>,
    );

    /// Stop recording, unregister callback.
    fn stop(&self);
}

/// The entry point to creating instruments. A grouping of related metrics.
pub trait Meter: Send + Sync + Debug {
    /// A type implementing [crate::attributes::Context]
    type Context: Context;

    /// A type implementing [AsyncMeasure] for [f64]
    type Gauge: AsyncMeasure<Context = Self::Context, Value = f64>;
    /// The type of the callback function passed when creating a [Meter::Gauge]
    type GaugeCallback<'a>: Fn(&Self::GaugeCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [Meter::GaugeCallback]
    type GaugeCallbackInput<'a>: AsyncMeasure<Context = Self::Context, Value = f64>;

    /// A type implementing [UpDownCounter]
    type UpDownCounter: UpDownCounter<Context = Self::Context>;

    /// A type implementing [AsyncMeasure] for [i64]
    type AsyncUDC: AsyncMeasure<Context = Self::Context, Value = i64>;
    /// The type of the callback function passed when creating a [Meter::AsyncUDC]
    type AsyncUDCCallback<'a>: Fn(&Self::AsyncUDCCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [Meter::AsyncUDCCallback]
    type AsyncUDCCallbackInput<'a>: AsyncMeasure<Context = Self::Context, Value = i64>;

    /// A type implementing [MonotonicCounter]
    type MonotonicCounter: MonotonicCounter<Context = Self::Context>;

    /// A type implementing [AsyncMeasure] for [u64]
    type AsyncMC: AsyncMeasure<Context = Self::Context, Value = u64>;
    /// The type of the callback function passed when creating a [Meter::AsyncMC]
    type AsyncMCCallback<'a>: Fn(&Self::AsyncMCCallbackInput<'a>) + Send + Sync;
    /// The type of the input to [Meter::AsyncMCCallback]
    type AsyncMCCallbackInput<'a>: AsyncMeasure<Context = Self::Context, Value = u64>;

    /// A type implementing [Histogram]
    type Histogram: Histogram<Context = Self::Context>;

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

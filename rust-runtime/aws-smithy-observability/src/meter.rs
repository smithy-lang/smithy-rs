/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics are used to gain insight into the operational performance and health of a system in
//! real time.

use crate::attributes::Attributes;
use crate::context::Context;
use std::{borrow::Cow, fmt::Debug, sync::Arc};

/// Provides named instances of [Meter].
pub trait ProvideMeter: Send + Sync + Debug {
    /// Get or create a named [Meter].
    fn get_meter(&self, scope: &'static str, attributes: Option<&Attributes>) -> Meter;

    /// Cast to [std::any::Any]
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The entry point to creating instruments. A grouping of related metrics.
#[derive(Clone)]
pub struct Meter {
    instrument_provider: Arc<dyn ProvideInstrument + Send + Sync>,
}

impl Meter {
    /// Create a new [Meter] from an [InstrumentProvider]
    pub fn new(instrument_provider: Arc<dyn ProvideInstrument + Send + Sync>) -> Self {
        Meter {
            instrument_provider,
        }
    }

    /// Create a new Gauge.
    #[allow(clippy::type_complexity)]
    pub fn create_gauge(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = f64>) + Send + Sync>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn AsyncMeasure<Value = f64>> {
        self.instrument_provider.create_gauge(
            name.into(),
            callback,
            units.map(Into::into),
            description.map(Into::into),
        )
    }

    /// Create a new [UpDownCounter].
    pub fn create_up_down_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn UpDownCounter> {
        self.instrument_provider.create_up_down_counter(
            name.into(),
            units.map(Into::into),
            description.map(Into::into),
        )
    }

    /// Create a new AsyncUpDownCounter.
    #[allow(clippy::type_complexity)]
    pub fn create_async_up_down_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = i64>) + Send + Sync>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn AsyncMeasure<Value = i64>> {
        self.instrument_provider.create_async_up_down_counter(
            name.into(),
            callback,
            units.map(Into::into),
            description.map(Into::into),
        )
    }

    /// Create a new [MonotonicCounter].
    pub fn create_monotonic_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn MonotonicCounter> {
        self.instrument_provider.create_monotonic_counter(
            name.into(),
            units.map(Into::into),
            description.map(Into::into),
        )
    }

    /// Create a new AsyncMonotonicCounter.
    #[allow(clippy::type_complexity)]
    pub fn create_async_monotonic_counter(
        &self,
        name: impl Into<Cow<'static, str>>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = u64>) + Send + Sync>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn AsyncMeasure<Value = u64>> {
        self.instrument_provider.create_async_monotonic_counter(
            name.into(),
            callback,
            units.map(Into::into),
            description.map(Into::into),
        )
    }

    /// Create a new [Histogram].
    pub fn create_histogram(
        &self,
        name: impl Into<Cow<'static, str>>,
        units: Option<impl Into<Cow<'static, str>>>,
        description: Option<impl Into<Cow<'static, str>>>,
    ) -> Arc<dyn Histogram> {
        self.instrument_provider.create_histogram(
            name.into(),
            units.map(Into::into),
            description.map(Into::into),
        )
    }
}
/// The entry point to creating instruments. A grouping of related metrics.
pub trait ProvideInstrument: Send + Sync + Debug {
    /// Create a new Gauge.
    #[allow(clippy::type_complexity)]
    fn create_gauge(
        &self,
        name: Cow<'static, str>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = f64>) + Send + Sync>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
    ) -> Arc<dyn AsyncMeasure<Value = f64>>;

    /// Create a new [UpDownCounter].
    fn create_up_down_counter(
        &self,
        name: Cow<'static, str>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
    ) -> Arc<dyn UpDownCounter>;

    /// Create a new AsyncUpDownCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_up_down_counter(
        &self,
        name: Cow<'static, str>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = i64>) + Send + Sync>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
    ) -> Arc<dyn AsyncMeasure<Value = i64>>;

    /// Create a new [MonotonicCounter].
    fn create_monotonic_counter(
        &self,
        name: Cow<'static, str>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
    ) -> Arc<dyn MonotonicCounter>;

    /// Create a new AsyncMonotonicCounter.
    #[allow(clippy::type_complexity)]
    fn create_async_monotonic_counter(
        &self,
        name: Cow<'static, str>,
        callback: Box<dyn Fn(&dyn AsyncMeasure<Value = u64>) + Send + Sync>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
    ) -> Arc<dyn AsyncMeasure<Value = u64>>;

    /// Create a new [Histogram].
    fn create_histogram(
        &self,
        name: Cow<'static, str>,
        units: Option<Cow<'static, str>>,
        description: Option<Cow<'static, str>>,
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

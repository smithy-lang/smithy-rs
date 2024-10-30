/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics are used to gain insight into the operational performance and health of a system in
//! real time.

use crate::attributes::{Attributes, Context, Double, Long, UnsignedLong};

/// Provides named instances of [Meter].
pub trait MeterProvider {
    /// Get or create a named [Meter].
    fn get_meter(&self, scope: String, attributes: Option<&Attributes>) -> &dyn Meter;
}

/// The entry point to creating instruments. A grouping of related metrics.
pub trait Meter {
    /// Create a new Gauge.
    fn create_gauge(
        &self,
        name: String,
        // TODO(smithyObservability): compare this definition to the Boxed version below
        // callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Double>>)>,
        // callback: &(dyn Fn(&dyn AsyncMeasurement<Value = Double>) + Send + Sync),
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Double>>) + Send + Sync>,
        units: Option<String>,
        description: Option<String>,
    ) -> Box<dyn AsyncMeasurementHandle>;

    /// Create a new [UpDownCounter].
    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn UpDownCounter;

    /// Create a new AsyncUpDownCounter.
    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Long>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle;

    /// Create a new [MonotonicCounter].
    fn create_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn MonotonicCounter;

    /// Create a new AsyncMonotonicCounter.
    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = UnsignedLong>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle;

    /// Create a new [Histogram].
    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn Histogram;
}

/// Collects a set of events with an event count and sum for all events.
pub trait Histogram {
    /// Record a value.
    fn record(&self, value: Double, attributes: Option<&Attributes>, context: Option<&dyn Context>);
}

/// A counter that monotonically increases.
pub trait MonotonicCounter {
    /// Increment a counter by a fixed amount.
    fn add(
        &self,
        value: UnsignedLong,
        attributes: Option<&Attributes>,
        context: Option<&dyn Context>,
    );
}

/// A counter that can increase or decrease.
pub trait UpDownCounter {
    /// Increment or decrement a counter by a fixed amount.
    fn add(&self, value: Long, attributes: Option<&Attributes>, context: Option<&dyn Context>);
}

/// A measurement that can be taken asynchronously.
pub trait AsyncMeasurement {
    /// The type recorded by the measurement.
    type Value;

    /// Record a value
    fn record(
        &self,
        value: Self::Value,
        attributes: Option<&Attributes>,
        context: Option<&dyn Context>,
    );
}

/// A handle to an [AsyncMeasurement].
pub trait AsyncMeasurementHandle {
    /// Stop recording , unregister callback.
    fn stop(&self);
}

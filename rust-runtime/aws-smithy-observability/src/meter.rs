/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::attributes::{self, AttributeMap, Attributes, Context, Double, Long};
use std::{any::Any, collections::HashMap, marker::PhantomData};

pub(crate) trait MeterProvider {
    fn get_meter(&self, scope: String, attributes: Option<AttributeMap>) -> &dyn Meter;
}

//TODO(smithyObservability): Are instruments globally unique by name?
pub(crate) trait Meter {
    fn create_gauge(
        &self,
        name: String,
        //TODO(smithyObservability): compare this definition to the Boxed version below
        // callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Double>>)>,
        callback: &dyn Fn(&dyn AsyncMeasurement<Value = Double>),
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle;

    fn create_up_down_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn UpDownCounter;

    fn create_async_up_down_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Long>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle;

    fn create_counter(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn MonotonicCounter;

    fn create_async_monotonic_counter(
        &self,
        name: String,
        callback: Box<dyn Fn(Box<dyn AsyncMeasurement<Value = Long>>)>,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn AsyncMeasurementHandle;

    fn create_histogram(
        &self,
        name: String,
        units: Option<String>,
        description: Option<String>,
    ) -> &dyn Histogram;
}

pub(crate) trait Histogram {
    /// Record a value
    fn record(
        &self,
        value: Double,
        attributes: Option<AttributeMap>,
        context: Option<&dyn Context>,
    );
}

pub(crate) trait MonotonicCounter {
    /// Increment a counter by a fixed amount
    fn add(&self, value: Long, attributes: Option<AttributeMap>, context: Option<&dyn Context>);
}

pub(crate) trait UpDownCounter {
    /// Increment or decrement a counter by a fixed amount
    fn add(&self, value: Long, attributes: Option<AttributeMap>, context: Option<&dyn Context>);
}

pub(crate) trait AsyncMeasurement {
    type Value;

    /// Record a value
    fn record(
        &self,
        value: Self::Value,
        attributes: Option<AttributeMap>,
        context: Option<&dyn Context>,
    );
}

pub(crate) trait AsyncMeasurementHandle {
    /// stop recording , unregister callback
    fn stop(&self);
}

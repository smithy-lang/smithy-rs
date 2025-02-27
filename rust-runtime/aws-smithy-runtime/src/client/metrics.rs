/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_observability::{
    global::get_telemetry_provider, instruments::Histogram, AttributeValue, Attributes,
    ObservabilityError,
};
use aws_smithy_runtime_api::client::{
    interceptors::Intercept, orchestrator::Metadata, runtime_components::RuntimeComponentsBuilder,
    runtime_plugin::RuntimePlugin,
};
use aws_smithy_types::config_bag::{Layer, Storable, StoreReplace};
use std::{borrow::Cow, sync::Arc, time::SystemTime};

/// Struct to hold metric data in the ConfigBag
#[derive(Debug, Clone)]
struct MeasurementsContainer {
    call_start: SystemTime,
    attempts: u32,
    attempt_start: SystemTime,
}

impl Storable for MeasurementsContainer {
    type Storer = StoreReplace<Self>;
}

/// Instruments for recording a single operation
#[derive(Debug, Clone)]
pub(crate) struct MetricsInterceptorInstruments {
    pub(crate) operation_duration: Arc<dyn Histogram>,
    pub(crate) attempt_duration: Arc<dyn Histogram>,
}

impl MetricsInterceptorInstruments {
    pub(crate) fn new() -> Result<Self, ObservabilityError> {
        let meter = get_telemetry_provider()?
            .meter_provider()
            .get_meter("MetricsInterceptor", None);

        Ok(Self{
            operation_duration: meter
                .create_histogram("smithy.client.call.duration")
                .set_units("s")
                .set_description("Overall call duration (including retries and time to send or receive request and response body)")
                .build(),
            attempt_duration: meter
                .create_histogram("smithy.client.call.attempt.duration")
                .set_units("s")
                .set_description("The time it takes to connect to the service, send the request, and get back HTTP status code and headers (including time queued waiting to be sent)")
                .build(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct MetricsInterceptor {
    instruments: MetricsInterceptorInstruments,
    // Holding a TimeSource here isn't ideal, but RuntimeComponents aren't available in
    // the read_before_execution hook and that is when we need to start the timer for
    // the operation.
    time_source: SharedTimeSource,
}

impl MetricsInterceptor {
    pub(crate) fn new(time_source: SharedTimeSource) -> Result<Self, ObservabilityError> {
        Ok(MetricsInterceptor {
            instruments: MetricsInterceptorInstruments::new()?,
            time_source,
        })
    }

    pub(crate) fn get_attrs_from_cfg(
        &self,
        cfg: &aws_smithy_types::config_bag::ConfigBag,
    ) -> Option<Attributes> {
        let operation_metadata = cfg.load::<Metadata>();

        if let Some(md) = operation_metadata {
            let mut attributes = Attributes::new();
            attributes.set("rpc.service", AttributeValue::String(md.service().into()));
            attributes.set("rpc.method", AttributeValue::String(md.name().into()));

            Some(attributes)
        } else {
            None
        }
    }
}

impl Intercept for MetricsInterceptor {
    fn name(&self) -> &'static str {
        "MetricsInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let mut layer = Layer::new("MetricsInterceptor");

        layer.store_put(MeasurementsContainer {
            call_start: self.time_source.now(),
            attempts: 0,
            attempt_start: SystemTime::UNIX_EPOCH,
        });
        cfg.push_layer(layer);
        Ok(())
    }

    fn read_after_execution(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::FinalizerInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .load::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");

        let attributes = self.get_attrs_from_cfg(cfg);

        if let Some(attrs) = attributes {
            let call_end = self.time_source.now();
            let call_duration = call_end.duration_since(measurements.call_start);
            if let Ok(elapsed) = call_duration {
                self.instruments.operation_duration.record(
                    elapsed.as_secs_f64(),
                    Some(&attrs),
                    None,
                );
            }
        }

        Ok(())
    }

    fn read_before_attempt(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .get_mut::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");

        measurements.attempts += 1;
        measurements.attempt_start = self.time_source.now();

        Ok(())
    }

    fn read_after_attempt(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::FinalizerInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .load::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");

        let attempt_end = self.time_source.now();
        let attempt_duration = attempt_end.duration_since(measurements.attempt_start);
        let attributes = self.get_attrs_from_cfg(cfg);

        if let (Ok(elapsed), Some(mut attrs)) = (attempt_duration, attributes) {
            attrs.set("attempt", AttributeValue::I64(measurements.attempts.into()));

            self.instruments
                .attempt_duration
                .record(elapsed.as_secs_f64(), Some(&attrs), None);
        }
        Ok(())
    }
}

/// Runtime plugin that adds an interceptor for collecting metrics
#[derive(Debug, Default)]
pub struct MetricsRuntimePlugin {
    time_source: SharedTimeSource,
}

impl MetricsRuntimePlugin {
    /// Creates a runtime plugin which installs an interceptor for collecting metrics
    pub fn new(time_source: SharedTimeSource) -> Self {
        Self { time_source }
    }
}

impl RuntimePlugin for MetricsRuntimePlugin {
    fn runtime_components(
        &self,
        _current_components: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        let interceptor = MetricsInterceptor::new(self.time_source.clone());
        if let Ok(interceptor) = interceptor {
            Cow::Owned(RuntimeComponentsBuilder::new("Metrics").with_interceptor(interceptor))
        } else {
            Cow::Owned(RuntimeComponentsBuilder::new("Metrics"))
        }
    }
}

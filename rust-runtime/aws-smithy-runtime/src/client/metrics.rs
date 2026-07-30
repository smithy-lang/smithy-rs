/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_observability::{
    global::get_telemetry_provider,
    instruments::{Histogram, MonotonicCounter},
    AttributeValue, Attributes, ObservabilityError,
};
use aws_smithy_runtime_api::client::{
    interceptors::{dyn_dispatch_hint, Intercept, SharedInterceptor},
    orchestrator::Metadata,
    runtime_components::RuntimeComponentsBuilder,
    runtime_plugin::RuntimePlugin,
};
use aws_smithy_types::config_bag::{FrozenLayer, Layer, Storable, StoreReplace};
use std::{borrow::Cow, sync::Arc, time::SystemTime};

/// Struct to hold metric data in the ConfigBag
#[derive(Debug, Clone)]
pub(crate) struct MeasurementsContainer {
    call_start: SystemTime,
    attempts: u32,
    attempt_start: SystemTime,
    serialization_start: Option<SystemTime>,
    deserialization_start: Option<SystemTime>,
    signing_start: Option<SystemTime>,
}

impl Storable for MeasurementsContainer {
    type Storer = StoreReplace<Self>;
}

/// Instruments for recording a single operation.
///
/// On retries: `operation_duration`, `serialization_duration`, and `call_errors` are
/// recorded once per call. All other metrics are recorded per-attempt.
#[derive(Debug, Clone)]
pub(crate) struct OperationTelemetry {
    pub(crate) operation_duration: Arc<dyn Histogram>,
    pub(crate) attempt_duration: Arc<dyn Histogram>,
    pub(crate) call_attempts: Arc<dyn MonotonicCounter>,
    pub(crate) call_errors: Arc<dyn MonotonicCounter>,
    pub(crate) serialization_duration: Arc<dyn Histogram>,
    pub(crate) deserialization_duration: Arc<dyn Histogram>,
    pub(crate) signing_duration: Arc<dyn Histogram>,
    pub(crate) resolve_identity_duration: Arc<dyn Histogram>,
    pub(crate) resolve_endpoint_duration: Arc<dyn Histogram>,
}

impl OperationTelemetry {
    pub(crate) fn new(scope: &'static str) -> Result<Self, ObservabilityError> {
        let meter = get_telemetry_provider()?
            .meter_provider()
            .get_meter(scope, None);

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
            call_attempts: meter
                .create_monotonic_counter("smithy.client.call.attempts")
                .set_units("{attempt}")
                .set_description("The number of attempts for an operation call")
                .build(),
            call_errors: meter
                .create_monotonic_counter("smithy.client.call.errors")
                .set_units("{error}")
                .set_description("The number of calls that result in an error")
                .build(),
            serialization_duration: meter
                .create_histogram("smithy.client.call.serialization_duration")
                .set_units("s")
                .set_description("The time it takes to serialize a request body")
                .build(),
            deserialization_duration: meter
                .create_histogram("smithy.client.call.deserialization_duration")
                .set_units("s")
                .set_description("The time it takes to deserialize a response body")
                .build(),
            signing_duration: meter
                .create_histogram("smithy.client.call.auth.signing_duration")
                .set_units("s")
                .set_description("The time it takes to sign a request")
                .build(),
            resolve_identity_duration: meter
                .create_histogram("smithy.client.call.auth.resolve_identity_duration")
                .set_units("s")
                .set_description("The time it takes to acquire an identity (credentials, bearer token, etc.) from an identity provider")
                .build(),
            resolve_endpoint_duration: meter
                .create_histogram("smithy.client.call.resolve_endpoint_duration")
                .set_units("s")
                .set_description("The time it takes to resolve an endpoint for the request")
                .build(),
        })
    }
}

impl Storable for OperationTelemetry {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug)]
pub(crate) struct MetricsInterceptor {
    time_source: SharedTimeSource,
}

impl MetricsInterceptor {
    pub(crate) fn new(time_source: SharedTimeSource) -> Result<Self, ObservabilityError> {
        Ok(MetricsInterceptor { time_source })
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

    pub(crate) fn get_measurements_and_instruments<'a>(
        &self,
        cfg: &'a aws_smithy_types::config_bag::ConfigBag,
    ) -> (&'a MeasurementsContainer, &'a OperationTelemetry) {
        let measurements = cfg
            .load::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");

        let instruments = cfg
            .load::<OperationTelemetry>()
            .expect("set in RuntimePlugin");

        (measurements, instruments)
    }
}

#[dyn_dispatch_hint]
impl Intercept for MetricsInterceptor {
    fn name(&self) -> &'static str {
        "MetricsInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        cfg.interceptor_state().store_put(MeasurementsContainer {
            call_start: self.time_source.now(),
            attempts: 0,
            attempt_start: SystemTime::UNIX_EPOCH,
            serialization_start: None,
            deserialization_start: None,
            signing_start: None,
        });

        Ok(())
    }

    fn read_before_serialization(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .get_mut::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");
        measurements.serialization_start = Some(self.time_source.now());
        Ok(())
    }

    fn read_after_serialization(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let (measurements, instruments) = self.get_measurements_and_instruments(cfg);
        let attributes = self.get_attrs_from_cfg(cfg);

        if let (Some(start), Some(attrs)) = (measurements.serialization_start, attributes) {
            let now = self.time_source.now();
            if let Ok(elapsed) = now.duration_since(start) {
                instruments.serialization_duration.record(
                    elapsed.as_secs_f64(),
                    Some(&attrs),
                    None,
                );
            }
        }
        Ok(())
    }

    fn read_before_signing(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .get_mut::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");
        measurements.signing_start = Some(self.time_source.now());
        Ok(())
    }

    fn read_after_signing(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let (measurements, instruments) = self.get_measurements_and_instruments(cfg);
        let attributes = self.get_attrs_from_cfg(cfg);

        if let (Some(start), Some(attrs)) = (measurements.signing_start, attributes) {
            let now = self.time_source.now();
            if let Ok(elapsed) = now.duration_since(start) {
                instruments
                    .signing_duration
                    .record(elapsed.as_secs_f64(), Some(&attrs), None);
            }
        }

        Ok(())
    }

    fn read_after_execution(
        &self,
        context: &aws_smithy_runtime_api::client::interceptors::context::FinalizerInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let (measurements, instruments) = self.get_measurements_and_instruments(cfg);

        let attributes = self.get_attrs_from_cfg(cfg);

        if let Some(attrs) = attributes {
            let call_end = self.time_source.now();
            let call_duration = call_end.duration_since(measurements.call_start);
            if let Ok(elapsed) = call_duration {
                instruments
                    .operation_duration
                    .record(elapsed.as_secs_f64(), Some(&attrs), None);
            }

            // Increment error counter if the call failed
            if let Some(Err(_)) = context.output_or_error() {
                instruments.call_errors.add(1, Some(&attrs), None);
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
        let (measurements, instruments) = self.get_measurements_and_instruments(cfg);

        let attempt_end = self.time_source.now();
        let attempt_duration = attempt_end.duration_since(measurements.attempt_start);
        let attributes = self.get_attrs_from_cfg(cfg);

        if let (Ok(elapsed), Some(mut attrs)) = (attempt_duration, attributes) {
            attrs.set("attempt", AttributeValue::I64(measurements.attempts.into()));

            instruments
                .attempt_duration
                .record(elapsed.as_secs_f64(), Some(&attrs), None);

            // Increment attempts counter
            instruments.call_attempts.add(1, Some(&attrs), None);
        }
        Ok(())
    }

    fn read_before_deserialization(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let measurements = cfg
            .get_mut::<MeasurementsContainer>()
            .expect("set in `read_before_execution`");
        measurements.deserialization_start = Some(self.time_source.now());
        Ok(())
    }

    fn read_after_deserialization(
        &self,
        _context: &aws_smithy_runtime_api::client::interceptors::context::AfterDeserializationInterceptorContextRef<'_>,
        _runtime_components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        cfg: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let (measurements, instruments) = self.get_measurements_and_instruments(cfg);
        let attributes = self.get_attrs_from_cfg(cfg);

        if let (Some(start), Some(attrs)) = (measurements.deserialization_start, attributes) {
            let now = self.time_source.now();
            if let Ok(elapsed) = now.duration_since(start) {
                instruments.deserialization_duration.record(
                    elapsed.as_secs_f64(),
                    Some(&attrs),
                    None,
                );
            }
        }
        Ok(())
    }
}

/// Runtime plugin that adds an interceptor for collecting metrics
#[derive(Debug, Default)]
pub struct MetricsRuntimePlugin {
    scope: &'static str,
    time_source: SharedTimeSource,
    metadata: Option<Metadata>,
}

impl MetricsRuntimePlugin {
    /// Create a [MetricsRuntimePluginBuilder]
    pub fn builder() -> MetricsRuntimePluginBuilder {
        MetricsRuntimePluginBuilder::default()
    }
}

impl RuntimePlugin for MetricsRuntimePlugin {
    fn runtime_components(
        &self,
        _current_components: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        let interceptor = MetricsInterceptor::new(self.time_source.clone());
        if let Ok(interceptor) = interceptor {
            Cow::Owned(
                RuntimeComponentsBuilder::new("Metrics")
                    .with_interceptor(SharedInterceptor::permanent(interceptor)),
            )
        } else {
            Cow::Owned(RuntimeComponentsBuilder::new("Metrics"))
        }
    }

    fn config(&self) -> Option<FrozenLayer> {
        let instruments = OperationTelemetry::new(self.scope);

        if let Ok(instruments) = instruments {
            let mut cfg = Layer::new("Metrics");
            cfg.store_put(instruments);

            if let Some(metadata) = &self.metadata {
                cfg.store_put(metadata.clone());
            }

            Some(cfg.freeze())
        } else {
            None
        }
    }
}

/// Builder for [MetricsRuntimePlugin]
#[derive(Debug, Default)]
pub struct MetricsRuntimePluginBuilder {
    scope: Option<&'static str>,
    time_source: Option<SharedTimeSource>,
    metadata: Option<Metadata>,
}

impl MetricsRuntimePluginBuilder {
    /// Set the scope for the metrics
    pub fn with_scope(mut self, scope: &'static str) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Set the [TimeSource] for the metrics
    pub fn with_time_source(mut self, time_source: impl TimeSource + 'static) -> Self {
        self.time_source = Some(SharedTimeSource::new(time_source));
        self
    }

    /// Set the [Metadata] for the metrics.
    ///
    /// Note: the Metadata is optional, most operations set it themselves, but this is useful
    /// for operations that do not, like some of the credential providers.
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Build a [MetricsRuntimePlugin]
    pub fn build(
        self,
    ) -> Result<MetricsRuntimePlugin, aws_smithy_runtime_api::box_error::BoxError> {
        if let Some(scope) = self.scope {
            Ok(MetricsRuntimePlugin {
                scope,
                time_source: self.time_source.unwrap_or_default(),
                metadata: self.metadata,
            })
        } else {
            Err("Scope is required for MetricsRuntimePlugin.".into())
        }
    }
}

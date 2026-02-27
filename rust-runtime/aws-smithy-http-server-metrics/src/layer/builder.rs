/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::Mutex;

use metrique::DefaultSink;
use metrique::OnParentDrop;
use metrique::Slot;

use crate::default::DefaultMetricsExtension;
use crate::default::DefaultMetricsServiceState;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;
use crate::layer::DefaultMetrics;
use crate::layer::MetricsLayer;
use crate::traits::InitMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::DefaultInit;
use crate::types::DefaultRs;
use crate::types::HttpRequest;

// Macro to generate disable methods for configuration
macro_rules! impl_disable_methods {
    () => {
        /// Disable all default request metrics
        pub fn disable_default_request_metrics(mut self) -> Self {
            self.default_req_metrics_config.disable_all = true;
            self
        }

        /// Disable all default response metrics
        pub fn disable_default_response_metrics(mut self) -> Self {
            self.default_res_metrics_config.disable_all = true;
            self
        }

        /// Disable the default `outstanding_requests` metric
        pub fn disable_default_outstanding_requests_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_outstanding_requests = true;
            self
        }

        /// Disable the default `request_id` metric
        pub fn disable_default_request_id_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_request_id = true;
            self
        }

        /// Disable the default `operation_name` metric
        pub fn disable_default_operation_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_operation = true;
            self
        }

        /// Disable the default `service_name` metric
        pub fn disable_default_service_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_service = true;
            self
        }

        /// Disable the default `service_version` metric
        pub fn disable_default_service_version_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_service_version = true;
            self
        }

        /// Disable the default `http_status_code` metric
        pub fn disable_default_http_status_code_metric(mut self) -> Self {
            self.default_res_metrics_config.disable_http_status_code = true;
            self
        }

        /// Disable the default `success` metric
        pub fn disable_default_success_metric(mut self) -> Self {
            self.default_res_metrics_config.disable_success = true;
            self
        }

        /// Disable the default `client_error` metric
        pub fn disable_default_client_error_metric(mut self) -> Self {
            self.default_res_metrics_config.disable_client_error = true;
            self
        }

        /// Disable the default `server_error` metric
        pub fn disable_default_server_error_metric(mut self) -> Self {
            self.default_res_metrics_config.disable_server_error = true;
            self
        }

        /// Disable the default `operation_time` metric
        pub fn disable_default_operation_time_metric(mut self) -> Self {
            self.default_res_metrics_config.disable_operation_time = true;
            self
        }
    };
}

pub struct NeedsInitialization;
pub struct WithRq;
pub struct WithRqAndRs;

#[non_exhaustive]
pub struct MetricsLayerBuilder<
    State,
    Entry = DefaultMetrics,
    Sink = DefaultSink,
    Init = DefaultInit<Entry, Sink>,
    Res = DefaultRs<Entry>,
> where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    pub init_metrics: Option<Init>,
    pub response_metrics: Option<Res>,
    pub default_req_metrics_config: DefaultRequestMetricsConfig,
    pub default_res_metrics_config: DefaultResponseMetricsConfig,
    pub(crate) _state: PhantomData<State>,
    pub(crate) _close_entry: PhantomData<Entry>,
    pub(crate) _entry_sink: PhantomData<Sink>,
}

impl<Entry, Sink> MetricsLayerBuilder<NeedsInitialization, Entry, Sink>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
{
    /// Sets the function to initialize metrics for each request
    ///
    /// # Example
    /// ```rust,ignore
    /// let metrics_layer = MetricsLayer::builder()
    ///     .init_metrics(|req| PokemonMetrics::default().append_on_drop(ServiceMetrics::sink()));
    /// ```
    pub fn init_metrics(
        self,
        init_metrics: impl InitMetrics<Entry, Sink>,
    ) -> MetricsLayerBuilder<WithRq, Entry, Sink, impl InitMetrics<Entry, Sink>> {
        MetricsLayerBuilder {
            init_metrics: Some(init_metrics),
            response_metrics: self.response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<Entry, Sink, Init> MetricsLayerBuilder<WithRq, Entry, Sink, Init>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
{
    impl_disable_methods!();

    /// Sets a function to extract custom metrics from responses.
    ///
    /// # Function Parameters
    ///
    /// - `response: &mut Response<ResBody>` - The HTTP response
    /// - `metrics: &mut Entry` - The metrics entry to populate
    ///
    /// Called after the operation handler completes.
    ///
    /// See [`ResponseMetrics`] for the full trait signature.
    pub fn response_metrics(
        self,
        f: impl ResponseMetrics<Entry>,
    ) -> MetricsLayerBuilder<WithRqAndRs, Entry, Sink, Init, impl ResponseMetrics<Entry>> {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            response_metrics: Some(f),
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<Entry, Sink, Init, Res> MetricsLayerBuilder<WithRqAndRs, Entry, Sink, Init, Res>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    impl_disable_methods!();
}

// Below are the extension trait and impls that will be generated by the proc macro when a metrics struct is
// annotated with #[smithy_metrics]

pub trait DefaultMetricsBuildExt<Sink, Init, Res>
where
    Sink: ThreadSafeEntrySink<DefaultMetrics>,
    Init: InitMetrics<DefaultMetrics, Sink>,
    Res: ResponseMetrics<DefaultMetrics>,
{
    /// Build the [`MetricsLayer`] that can be added to your service.
    fn build(self) -> MetricsLayer<DefaultMetrics, Sink, Init, Res>;
}

macro_rules! impl_build_for_state {
    ($state:ty) => {
        impl<Sink, Init, Res> DefaultMetricsBuildExt<Sink, Init, Res>
            for MetricsLayerBuilder<$state, DefaultMetrics, Sink, Init, Res>
        where
            Sink: ThreadSafeEntrySink<DefaultMetrics>,
            Init: InitMetrics<DefaultMetrics, Sink>,
            Res: ResponseMetrics<DefaultMetrics>,
        {
            fn build(self) -> MetricsLayer<DefaultMetrics, Sink, Init, Res> {
                let default_metrics_extension_fn =
                    |req: &mut HttpRequest,
                     metrics: &mut DefaultMetrics,
                     req_config: DefaultRequestMetricsConfig,
                     res_config: DefaultResponseMetricsConfig,
                     service_state: DefaultMetricsServiceState| {
                        metrics.default_request_metrics =
                            Some(Slot::new(DefaultRequestMetrics::default()));
                        metrics.default_response_metrics =
                            Some(Slot::new(DefaultResponseMetrics::default()));

                        let default_req_metrics_slotguard = metrics
                            .default_request_metrics
                            .as_mut()
                            .and_then(|slot| slot.open(OnParentDrop::Discard))
                            .expect(
                                "unreachable: the option is set to a created slot in this scope",
                            );
                        let default_res_metrics_slotguard = metrics
                            .default_response_metrics
                            .as_mut()
                            .and_then(|slot| slot.open(OnParentDrop::Discard))
                            .expect(
                                "unreachable: the option is set to a created slot in this scope",
                            );

                        let ext = DefaultMetricsExtension {
                            request_ext: DefaultRequestMetricsExtension {
                                metrics: Arc::new(Mutex::new(default_req_metrics_slotguard)),
                                config: req_config,
                            },
                            response_ext: DefaultResponseMetricsExtension {
                                metrics: Arc::new(Mutex::new(default_res_metrics_slotguard)),
                                config: res_config,
                            },
                            service_state,
                        };

                        req.extensions_mut().insert(ext);
                    };

                MetricsLayer {
                    init_metrics: self.init_metrics.expect("init_metrics must be provided"),
                    response_metrics: self.response_metrics,
                    default_metrics_extension_fn,
                    default_req_metrics_config: self.default_req_metrics_config,
                    default_res_metrics_config: self.default_res_metrics_config,
                    _entry_sink: PhantomData,
                }
            }
        }
    };
}

impl_build_for_state!(WithRq);
impl_build_for_state!(WithRqAndRs);

#[cfg(test)]
mod tests {
    use metrique::AppendAndCloseOnDrop;
    use metrique::ServiceMetrics;
    use metrique_writer::GlobalEntrySink;

    use super::*;
    use crate::layer::MetricsLayer;
    use crate::types::HttpResponse;

    // Compile-time guarantees that methods exist on the correct states
    macro_rules! assert_methods_callable {
        ($state:ty => [$($method:ident($($args:tt)*)),*]) => {
            $(const _: fn(MetricsLayerBuilder<$state>) = |b| { b.$method($($args)*); };)*
        };
    }

    // Test that methods can be called on correct states - these will fail to compile if methods don't exist
    assert_methods_callable!(NeedsInitialization => [init_metrics(dummy_init)]);
    assert_methods_callable!(WithRq => [response_metrics(dummy_response_fn), build()]);
    assert_methods_callable!(WithRqAndRs => [build()]);

    // State transition tests
    macro_rules! assert_state {
        ($fn_name:ident, $state:ty) => {
            fn $fn_name<Entry, Sink, Init, Res>(
                _: &MetricsLayerBuilder<$state, Entry, Sink, Init, Res>,
            ) where
                Entry: ThreadSafeCloseEntry,
                Sink: ThreadSafeEntrySink<Entry>,
                Init: InitMetrics<Entry, Sink>,
                Res: ResponseMetrics<Entry>,
            {
            }
        };
    }
    assert_state!(assert_needs_initialization, NeedsInitialization);
    assert_state!(assert_with_rq, WithRq);
    assert_state!(assert_with_rq_and_rs, WithRqAndRs);

    fn dummy_init(_req: &mut HttpRequest) -> AppendAndCloseOnDrop<DefaultMetrics, DefaultSink> {
        DefaultMetrics::default().append_on_drop(ServiceMetrics::sink())
    }

    fn dummy_response_fn(_res: &mut HttpResponse, _metrics: &mut DefaultMetrics) {}

    #[test]
    fn test_needs_initialization_state() {
        let builder = MetricsLayer::<DefaultMetrics, DefaultSink>::builder();
        assert_needs_initialization(&builder);
    }

    #[test]
    fn test_transition_to_with_rq_via_init_metrics() {
        let builder = MetricsLayer::builder().init_metrics(dummy_init);
        assert_with_rq(&builder);
    }

    #[test]
    fn test_with_rq_to_with_rq_and_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .response_metrics(dummy_response_fn);
        assert_with_rq_and_rs(&builder);
    }
}

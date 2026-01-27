/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::Mutex;

use http::Request;
use http::Response;
use metrique::DefaultSink;
use metrique::OnParentDrop;
use metrique::Slot;

use crate::default::DefaultMetricsExtension;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;
use crate::layer::DefaultMetrics;
use crate::layer::MetricsLayer;
use crate::layer::ReqBody;
use crate::layer::ResBody;
use crate::traits::InitMetrics;
use crate::traits::RequestMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::DefaultInit;
use crate::types::DefaultRq;
use crate::types::DefaultRs;

// Macro to generate disable methods for configuration
macro_rules! impl_disable_methods {
    () => {
        pub fn disable_default_request_metrics(mut self) -> Self {
            self.default_req_metrics_config.disable_all = true;
            self
        }

        pub fn disable_default_response_metrics(mut self) -> Self {
            self.default_res_metrics_config.disable_all = true;
            self
        }

        pub fn disable_default_request_id_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_request_id = true;
            self
        }

        pub fn disable_default_operation_name_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_operation_name = true;
            self
        }

        pub fn disable_default_service_name_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_service_name = true;
            self
        }

        pub fn disable_default_service_version_metric(mut self) -> Self {
            self.default_req_metrics_config.disable_service_version = true;
            self
        }

        pub fn disable_default_http_status_code(mut self) -> Self {
            self.default_res_metrics_config.disable_http_status_code = true;
            self
        }
    };
}

pub struct NeedsInitialization;
pub struct WithDefaults;
pub struct WithRq;
pub struct WithRs;
pub struct WithRqAndRs;

#[non_exhaustive]
pub struct MetricsLayerBuilder<
    State,
    E = DefaultMetrics,
    S = DefaultSink,
    I = DefaultInit<E, S>,
    Rq = DefaultRq<E>,
    Rs = DefaultRs<E>,
> where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    pub init_metrics: Option<I>,
    pub request_metrics: Option<Rq>,
    pub response_metrics: Option<Rs>,
    pub default_req_metrics_config: DefaultRequestMetricsConfig,
    pub default_res_metrics_config: DefaultResponseMetricsConfig,
    pub(crate) _state: PhantomData<State>,
    pub(crate) _close_entry: PhantomData<E>,
    pub(crate) _entry_sink: PhantomData<S>,
}

impl<E, S> MetricsLayerBuilder<NeedsInitialization, E, S>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
{
    pub fn init_metrics(
        self,
        init_metrics: impl InitMetrics<E, S>,
    ) -> MetricsLayerBuilder<WithDefaults, E, S, impl InitMetrics<E, S>> {
        MetricsLayerBuilder {
            init_metrics: Some(init_metrics),
            request_metrics: self.request_metrics,
            response_metrics: self.response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<E, S, I> MetricsLayerBuilder<WithDefaults, E, S, I>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
{
    pub fn request_metrics(
        self,
        f: impl RequestMetrics<E>,
    ) -> MetricsLayerBuilder<WithRq, E, S, I, impl RequestMetrics<E>, DefaultRs<E>> {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            request_metrics: Some(f),
            response_metrics: None,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }

    pub fn response_metrics(
        self,
        f: impl Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static,
    ) -> MetricsLayerBuilder<WithRs, E, S, I, DefaultRq<E>, impl ResponseMetrics<E>> {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            request_metrics: None,
            response_metrics: Some(f),
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }

    impl_disable_methods!();
}

impl<E, S, I, Rq> MetricsLayerBuilder<WithRq, E, S, I, Rq>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
{
    pub fn response_metrics(
        self,
        f: impl ResponseMetrics<E>,
    ) -> MetricsLayerBuilder<WithRqAndRs, E, S, I, impl RequestMetrics<E>, impl ResponseMetrics<E>>
    {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            request_metrics: self.request_metrics,
            response_metrics: Some(f),
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }

    impl_disable_methods!();
}

impl<E, S, I, Rs> MetricsLayerBuilder<WithRs, E, S, I, DefaultRq<E>, Rs>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rs: ResponseMetrics<E>,
{
    pub fn request_metrics(
        self,
        f: impl RequestMetrics<E>,
    ) -> MetricsLayerBuilder<WithRqAndRs, E, S, I, impl RequestMetrics<E>, impl ResponseMetrics<E>>
    {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            request_metrics: Some(f),
            response_metrics: self.response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }

    impl_disable_methods!();
}

impl<E, S, I, Rq, Rs> MetricsLayerBuilder<WithRqAndRs, E, S, I, Rq, Rs>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    impl_disable_methods!();
}

// Below are the extension trait and impls that will be generated by the proc macro when a metrics struct is
// annotated with #[smithy_metrics]

pub trait DefaultMetricsBuildExt<S, I, Rq, Rs>
where
    S: ThreadSafeEntrySink<DefaultMetrics>,
    I: InitMetrics<DefaultMetrics, S>,
    Rq: RequestMetrics<DefaultMetrics>,
    Rs: ResponseMetrics<DefaultMetrics>,
{
    fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs>;
}

macro_rules! impl_build_for_state {
    ($state:ty) => {
        impl<S, I, Rq, Rs> DefaultMetricsBuildExt<S, I, Rq, Rs>
            for MetricsLayerBuilder<$state, DefaultMetrics, S, I, Rq, Rs>
        where
            S: ThreadSafeEntrySink<DefaultMetrics>,
            I: InitMetrics<DefaultMetrics, S>,
            Rq: RequestMetrics<DefaultMetrics>,
            Rs: ResponseMetrics<DefaultMetrics>,
        {
            fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs> {
                let default_metrics_extension_fn =
                    |req: &mut Request<ReqBody>,
                     metrics: &mut DefaultMetrics,
                     req_config: DefaultRequestMetricsConfig,
                     res_config: DefaultResponseMetricsConfig| {
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
                        };

                        req.extensions_mut().insert(ext);
                    };

                MetricsLayer {
                    init_metrics: self.init_metrics.expect("init_metrics must be provided"),
                    request_metrics: self.request_metrics,
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

impl_build_for_state!(WithDefaults);
impl_build_for_state!(WithRq);
impl_build_for_state!(WithRs);
impl_build_for_state!(WithRqAndRs);

#[cfg(test)]
mod tests {
    use metrique::AppendAndCloseOnDrop;
    use metrique::ServiceMetrics;
    use metrique_writer::GlobalEntrySink;

    use super::*;
    use crate::layer::MetricsLayer;

    // Compile-time guarantees that methods exist on the correct states
    macro_rules! assert_methods_callable {
        ($state:ty => [$($method:ident($($args:tt)*)),*]) => {
            $(const _: fn(MetricsLayerBuilder<$state>) = |b| { b.$method($($args)*); };)*
        };
    }

    // Test that methods can be called on correct states - these will fail to compile if methods don't exist
    assert_methods_callable!(NeedsInitialization => [init_metrics(dummy_init)]);
    assert_methods_callable!(WithDefaults => [
        request_metrics(dummy_request_fn),
        response_metrics(dummy_response_fn),
        build()
    ]);
    assert_methods_callable!(WithRq => [response_metrics(dummy_response_fn), build()]);
    assert_methods_callable!(WithRs => [request_metrics(dummy_request_fn), build()]);
    assert_methods_callable!(WithRqAndRs => [build()]);

    // State transition tests
    macro_rules! assert_state {
        ($fn_name:ident, $state:ty) => {
            fn $fn_name<E, S, I, Rq, Rs>(_: &MetricsLayerBuilder<$state, E, S, I, Rq, Rs>)
            where
                E: ThreadSafeCloseEntry,
                S: ThreadSafeEntrySink<E>,
                I: InitMetrics<E, S>,
                Rq: RequestMetrics<E>,
                Rs: ResponseMetrics<E>,
            {
            }
        };
    }
    assert_state!(assert_needs_initialization, NeedsInitialization);
    assert_state!(assert_with_defaults, WithDefaults);
    assert_state!(assert_with_rq, WithRq);
    assert_state!(assert_with_rs, WithRs);
    assert_state!(assert_with_rq_and_rs, WithRqAndRs);

    fn dummy_init() -> AppendAndCloseOnDrop<DefaultMetrics, DefaultSink> {
        DefaultMetrics::default().append_on_drop(ServiceMetrics::sink())
    }

    fn dummy_request_fn(_req: &mut Request<ReqBody>, _metrics: &mut DefaultMetrics) {}
    fn dummy_response_fn(_res: &mut Response<ResBody>, _metrics: &mut DefaultMetrics) {}

    #[test]
    fn test_needs_initialization_state() {
        let builder = MetricsLayer::<DefaultMetrics, DefaultSink>::builder();
        assert_needs_initialization(&builder);
    }

    #[test]
    fn test_transition_to_with_defaults_via_init_metrics() {
        let builder = MetricsLayer::builder().init_metrics(dummy_init);
        assert_with_defaults(&builder);
    }

    #[test]
    fn test_with_defaults_to_with_rq() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .request_metrics(dummy_request_fn);
        assert_with_rq(&builder);
    }

    #[test]
    fn test_with_defaults_to_with_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .response_metrics(dummy_response_fn);
        assert_with_rs(&builder);
    }

    #[test]
    fn test_with_rq_to_with_rq_and_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .request_metrics(dummy_request_fn)
            .response_metrics(dummy_response_fn);
        assert_with_rq_and_rs(&builder);
    }

    #[test]
    fn test_with_rs_to_with_rq_and_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .response_metrics(dummy_response_fn)
            .request_metrics(dummy_request_fn);
        assert_with_rq_and_rs(&builder);
    }
}

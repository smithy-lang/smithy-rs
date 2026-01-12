use std::marker::PhantomData;

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::DefaultSink;
use metrique::OnParentDrop;
use metrique::RootEntry;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique::writer::EntrySink;
use metrique_core::CloseEntry;
use metrique_writer::AttachGlobalEntrySink;
use metrique_writer::GlobalEntrySink;

use crate::DefaultInit;
use crate::DefaultRq;
use crate::DefaultRs;
use crate::default::DefaultMetricsEntry;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;
use crate::layer::DefaultMetrics;
use crate::layer::DefaultMetricsLayerError;
use crate::layer::MetricsLayer;
use crate::layer::ReqBody;
use crate::layer::ResBody;

pub struct NeedsInitialization;
pub struct WithDefaults;
pub struct WithRq;
pub struct WithRs;
pub struct WithRqAndRs;

pub struct MetricsLayerBuilder<
    State,
    E = DefaultMetrics,
    S = DefaultSink,
    I = DefaultInit<E, S>,
    Rq = DefaultRq<E, S>,
    Rs = DefaultRs<E, S>,
> where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
    pub(crate) init_metrics: Option<I>,
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,
    pub(crate) _state: PhantomData<State>,
}

impl MetricsLayerBuilder<NeedsInitialization> {
    /// Return a [`MetricsLayerBuilder`] with default metrics initialization using metrique's
    /// application-wide global entry sink [`metrique::ServiceMetrics`].
    ///
    /// To use this, attach an [`EntrySink`] to metrique's application-wide global entry sink
    /// [`metrique::ServiceMetrics`].
    ///
    /// # Errors
    ///
    /// Returns [`DefaultMetricsLayerError::NoSinkAttached`] if an [`metrique::EntrySink`]
    /// has not been attached to metrique's application-wide global entry sink.
    /// [`metrique::ServiceMetrics`]
    pub fn try_init_with_defaults(
        self,
    ) -> Result<MetricsLayerBuilder<WithDefaults>, DefaultMetricsLayerError> {
        if ServiceMetrics::try_sink().is_none() {
            return Err(DefaultMetricsLayerError::NoSinkAttached);
        };

        Ok(MetricsLayerBuilder {
            init_metrics: Some(|| DefaultMetrics::default().append_on_drop(ServiceMetrics::sink())),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        })
    }
}

impl<E, S> MetricsLayerBuilder<NeedsInitialization, E, S>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub fn init_metrics(
        self,
        init_metrics: impl Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    ) -> MetricsLayerBuilder<
        WithDefaults,
        E,
        S,
        impl Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    > {
        MetricsLayerBuilder {
            init_metrics: Some(init_metrics),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        }
    }
}

impl<E, S, I> MetricsLayerBuilder<WithDefaults, E, S, I>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
{
    pub fn set_request_metrics(
        self,
        f: impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> MetricsLayerBuilder<
        WithRq,
        E,
        S,
        I,
        impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
        DefaultRs<E, S>,
    > {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            set_request_metrics: Some(f),
            set_response_metrics: None,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        }
    }

    pub fn set_response_metrics(
        self,
        f: impl Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> MetricsLayerBuilder<
        WithRs,
        E,
        S,
        I,
        DefaultRq<E, S>,
        impl Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    > {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            set_request_metrics: None,
            set_response_metrics: Some(f),
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        }
    }

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
}

impl<E, S, I, Rq> MetricsLayerBuilder<WithRq, E, S, I, Rq>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
    pub fn set_response_metrics(
        self,
        f: impl Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> MetricsLayerBuilder<
        WithRqAndRs,
        E,
        S,
        I,
        impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
        impl Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    > {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: Some(f),
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        }
    }

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
}

impl<E, S, I, Rs> MetricsLayerBuilder<WithRs, E, S, I, DefaultRq<E, S>, Rs>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
    pub fn set_request_metrics(
        self,
        f: impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> MetricsLayerBuilder<
        WithRqAndRs,
        E,
        S,
        I,
        impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
        impl Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    > {
        MetricsLayerBuilder {
            init_metrics: self.init_metrics,
            set_request_metrics: Some(f),
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
            _state: PhantomData,
        }
    }

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
}

/// These are the impls that will be generated by the proc macro when the metrics struct is annotated with #[smithy_metrics]
impl<S, I, Rq, Rs> MetricsLayerBuilder<WithDefaults, DefaultMetrics, S, I, Rq, Rs>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<DefaultMetrics, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs> {
        let default_req_metrics_extension_fn =
            |req: &mut Request<ReqBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultRequestMetricsConfig| {
                metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));
                let default_req_metrics_slotguard = metrics
                    .request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultRequestMetricsExtension {
                    metrics: default_req_metrics_slotguard,
                    config,
                };

                req.extensions_mut().insert(ext);
            };

        let default_res_metrics_extension_fn =
            |res: &mut Response<ResBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultResponseMetricsConfig| {
                metrics.response_metrics = Some(Slot::new(DefaultResponseMetrics::default()));
                let default_res_metrics_slotguard = metrics
                    .response_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultResponseMetricsExtension {
                    metrics: default_res_metrics_slotguard,
                    config,
                };

                res.extensions_mut().insert(ext);
            };

        MetricsLayer {
            init_metrics: self.init_metrics.expect("init_metrics must be provided"),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_extension_fn,
            default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
        }
    }
}

impl<S, I, Rq, Rs> MetricsLayerBuilder<WithRq, DefaultMetrics, S, I, Rq, Rs>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<DefaultMetrics, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs> {
        let default_req_metrics_extension_fn =
            |req: &mut Request<ReqBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultRequestMetricsConfig| {
                metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));
                let default_req_metrics_slotguard = metrics
                    .request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultRequestMetricsExtension {
                    metrics: default_req_metrics_slotguard,
                    config,
                };

                req.extensions_mut().insert(ext);
            };

        let default_res_metrics_extension_fn =
            |res: &mut Response<ResBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultResponseMetricsConfig| {
                metrics.response_metrics = Some(Slot::new(DefaultResponseMetrics::default()));
                let default_res_metrics_slotguard = metrics
                    .response_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultResponseMetricsExtension {
                    metrics: default_res_metrics_slotguard,
                    config,
                };

                res.extensions_mut().insert(ext);
            };

        MetricsLayer {
            init_metrics: self.init_metrics.expect("init_metrics must be provided"),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_extension_fn,
            default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
        }
    }
}

impl<S, I, Rq, Rs> MetricsLayerBuilder<WithRs, DefaultMetrics, S, I, Rq, Rs>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<DefaultMetrics, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs> {
        let default_req_metrics_extension_fn =
            |req: &mut Request<ReqBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultRequestMetricsConfig| {
                metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));
                let default_req_metrics_slotguard = metrics
                    .request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultRequestMetricsExtension {
                    metrics: default_req_metrics_slotguard,
                    config,
                };

                req.extensions_mut().insert(ext);
            };

        let default_res_metrics_extension_fn =
            |res: &mut Response<ResBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultResponseMetricsConfig| {
                metrics.response_metrics = Some(Slot::new(DefaultResponseMetrics::default()));
                let default_res_metrics_slotguard = metrics
                    .response_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultResponseMetricsExtension {
                    metrics: default_res_metrics_slotguard,
                    config,
                };

                res.extensions_mut().insert(ext);
            };

        MetricsLayer {
            init_metrics: self.init_metrics.expect("init_metrics must be provided"),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_extension_fn,
            default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
        }
    }
}

impl<S, I, Rq, Rs> MetricsLayerBuilder<WithRqAndRs, DefaultMetrics, S, I, Rq, Rs>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<DefaultMetrics, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>)
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub fn build(self) -> MetricsLayer<DefaultMetrics, S, I, Rq, Rs> {
        let default_req_metrics_extension_fn =
            |req: &mut Request<ReqBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultRequestMetricsConfig| {
                metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));
                let default_req_metrics_slotguard = metrics
                    .request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultRequestMetricsExtension {
                    metrics: default_req_metrics_slotguard,
                    config,
                };

                req.extensions_mut().insert(ext);
            };

        let default_res_metrics_extension_fn =
            |res: &mut Response<ResBody>,
             metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>,
             config: DefaultResponseMetricsConfig| {
                metrics.response_metrics = Some(Slot::new(DefaultResponseMetrics::default()));
                let default_res_metrics_slotguard = metrics
                    .response_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let ext = DefaultResponseMetricsExtension {
                    metrics: default_res_metrics_slotguard,
                    config,
                };

                res.extensions_mut().insert(ext);
            };

        MetricsLayer {
            init_metrics: self.init_metrics.expect("init_metrics must be provided"),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
            default_req_metrics_extension_fn,
            default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config,
            default_res_metrics_config: self.default_res_metrics_config,
        }
    }
}
#[cfg(test)]
mod tests {
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
        set_request_metrics(dummy_request_fn),
        set_response_metrics(dummy_response_fn),
        build()
    ]);
    assert_methods_callable!(WithRq => [set_response_metrics(dummy_response_fn), build()]);
    assert_methods_callable!(WithRs => [set_request_metrics(dummy_request_fn), build()]);
    assert_methods_callable!(WithRqAndRs => [build()]);

    // State transition tests
    macro_rules! assert_state {
        ($fn_name:ident, $state:ty) => {
            fn $fn_name<E, S, I, Rq, Rs>(_: &MetricsLayerBuilder<$state, E, S, I, Rq, Rs>)
            where
                E: CloseEntry + Send + Sync + 'static,
                S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
                I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
                Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)
                    + Clone
                    + Send
                    + Sync
                    + 'static,
                Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)
                    + Clone
                    + Send
                    + Sync
                    + 'static,
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

    fn dummy_request_fn(
        _req: &mut Request<ReqBody>,
        _metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, DefaultSink>,
    ) {
    }
    fn dummy_response_fn(
        _res: &mut Response<ResBody>,
        _metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, DefaultSink>,
    ) {
    }

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
            .set_request_metrics(dummy_request_fn);
        assert_with_rq(&builder);
    }

    #[test]
    fn test_with_defaults_to_with_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .set_response_metrics(dummy_response_fn);
        assert_with_rs(&builder);
    }

    #[test]
    fn test_with_rq_to_with_rq_and_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .set_request_metrics(dummy_request_fn)
            .set_response_metrics(dummy_response_fn);
        assert_with_rq_and_rs(&builder);
    }

    #[test]
    fn test_with_rs_to_with_rq_and_rs() {
        let builder = MetricsLayer::builder()
            .init_metrics(dummy_init)
            .set_response_metrics(dummy_response_fn)
            .set_request_metrics(dummy_request_fn);
        assert_with_rq_and_rs(&builder);
    }

    #[test]
    fn test_try_init_with_defaults_no_sink() {
        let result = MetricsLayer::builder().try_init_with_defaults();
        assert!(matches!(
            result,
            Err(DefaultMetricsLayerError::NoSinkAttached)
        ));
    }
}

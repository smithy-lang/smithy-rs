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
pub struct Ready;

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
    ) -> Result<MetricsLayerBuilder<Ready>, DefaultMetricsLayerError> {
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

impl<E, S, I> MetricsLayerBuilder<NeedsInitialization, E, S, I>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
{
    pub fn init_metrics(
        self,
        init_metrics: I,
    ) -> MetricsLayerBuilder<Ready, E, S, I, DefaultRq<E, S>, DefaultRs<E, S>> {
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

impl<E, S, I, Rq, Rs> MetricsLayerBuilder<Ready, E, S, I, Rq, Rs>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
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

    pub fn set_request_metrics(mut self, f: Rq) -> Self {
        self.set_request_metrics = Some(f);
        self
    }

    pub fn set_response_metrics(mut self, f: Rs) -> Self {
        self.set_response_metrics = Some(f);
        self
    }
}

/// This is the impl that will be generated by the proc macro when the metrics struct is annotated with #[smithy_metrics]
impl<S, I, Rq, Rs> MetricsLayerBuilder<Ready, DefaultMetrics, S, I, Rq, Rs>
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

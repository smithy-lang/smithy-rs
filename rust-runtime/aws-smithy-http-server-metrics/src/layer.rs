#![allow(missing_docs)]

use std::marker::PhantomData;

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::DefaultSink;
use metrique::RootEntry;
use metrique_core::CloseEntry;
use metrique_writer::EntrySink;
use thiserror::Error;
use tower::Layer;

use crate::DefaultInit;
use crate::DefaultRq;
use crate::DefaultRs;
use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::service::MetricsLayerService;

pub mod builder;

#[derive(Error, Debug)]
pub enum DefaultMetricsLayerError {
    #[error("No sink attached to [`metrique::ServiceMetrics`]")]
    NoSinkAttached,
}

#[derive(Debug)]
pub struct MetricsLayer<
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
    pub(crate) init_metrics: I,
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
    pub(crate) default_req_metrics_extension_fn:
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultRequestMetricsConfig),
    pub(crate) default_res_metrics_extension_fn:
        fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultResponseMetricsConfig),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,
}
impl MetricsLayer {
    /// Return a [`MetricsLayer`] with default metrics initialization using metrique's
    /// application-wide global entry sink [`metrique::ServiceMetrics`].
    ///
    /// See [`MetricsLayerBuilder::try_init_with_defaults`].
    pub fn try_new() -> Result<MetricsLayer, DefaultMetricsLayerError> {
        Ok(Self::builder().try_init_with_defaults()?.build())
    }
}

impl<E, S> MetricsLayer<E, S>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, E, S> {
        MetricsLayerBuilder {
            init_metrics: None,
            set_request_metrics: None,
            set_response_metrics: None,
            default_req_metrics_config: DefaultRequestMetricsConfig::default(),
            default_res_metrics_config: DefaultResponseMetricsConfig::default(),
            _state: PhantomData,
        }
    }
}

impl<Ser, E, S, I, Rq, Rs> Layer<Ser> for MetricsLayer<E, S, I, Rq, Rs>
where
    Ser: Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
    type Service = MetricsLayerService<Ser, E, S, I, Rq, Rs>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService {
            inner,
            init_metrics: self.init_metrics.clone(),
            set_request_metrics: self.set_request_metrics.clone(),
            set_response_metrics: self.set_response_metrics.clone(),
            default_req_metrics_extension_fn: self.default_req_metrics_extension_fn.clone(),
            default_res_metrics_extension_fn: self.default_res_metrics_extension_fn.clone(),
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),
        }
    }
}

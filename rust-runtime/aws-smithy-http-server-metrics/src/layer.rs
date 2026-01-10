#![allow(missing_docs)]

use std::marker::PhantomData;

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::DefaultSink;
use metrique::RootEntry;
use metrique::ServiceMetrics;
use metrique::writer::EntrySink;
use metrique_core::CloseEntry;
use metrique_writer::GlobalEntrySink;
use tower::Layer;

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::service::MetricsLayerService;

pub mod builder;

pub struct MetricsLayer<
    E = DefaultMetrics,
    S = DefaultSink,
    I = fn() -> AppendAndCloseOnDrop<E, S>,
    Rq = fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>),
    Rs = fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>),
> where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
{
    pub(crate) init_metrics: I,
    pub(crate) default_req_metrics_extension_fn:
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>),
    pub(crate) default_res_metrics_extension_fn:
        fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>),
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
}
impl MetricsLayer {
    pub fn new() -> Self {
        Self::builder()
            .init_metrics(|| DefaultMetrics::default().append_on_drop(ServiceMetrics::sink()))
            .build()
    }
}

impl<E, S, I, Rq, Rs> MetricsLayer<E, S, I, Rq, Rs>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, E, S, I, Rq, Rs> {
        MetricsLayerBuilder {
            init_metrics: None,
            set_request_metrics: None,
            set_response_metrics: None,
            with_default_request_metrics: true,
            with_default_response_metrics: true,
            _state: PhantomData,
        }
    }
}

impl<Ser, E, S, I, Rq, Rs> Layer<Ser> for MetricsLayer<E, S, I, Rq, Rs>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    type Service = MetricsLayerService<Ser, I, Rq, Rs, E, S>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService {
            inner,
            init_metrics: self.init_metrics.clone(),
            default_req_metrics_extension_fn: self.default_req_metrics_extension_fn,
            default_res_metrics_extension_fn: self.default_res_metrics_extension_fn,
            set_request_metrics: self.set_request_metrics.clone(),
            set_response_metrics: self.set_response_metrics.clone(),
        }
    }
}

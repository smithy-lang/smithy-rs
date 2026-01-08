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

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::layer::inner::MetricsLayer as MetricsLayerInner;

pub mod builder;
pub mod inner;

pub struct MetricsLayer<
    E = DefaultMetrics,
    S = DefaultSink,
    I = fn() -> AppendAndCloseOnDrop<E, S>,
    Rq = fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>),
    Rs = fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>),
> where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    _close_entry: PhantomData<E>,
    _entry_sink: PhantomData<S>,
    _init_fn: PhantomData<I>,
    _req_fn: PhantomData<Rq>,
    _res_fn: PhantomData<Rs>,
}
impl MetricsLayer {
    pub fn new() -> MetricsLayerInner<
        fn() -> AppendAndCloseOnDrop<DefaultMetrics, DefaultSink>,
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, DefaultSink>),
        fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, DefaultSink>),
        DefaultMetrics,
        DefaultSink,
    > {
        Self::builder()
            .init_metrics(|| DefaultMetrics::default().append_on_drop(ServiceMetrics::sink()))
            .build()
    }
}

impl<E, S, I, Rq, Rs> MetricsLayer<E, S, I, Rq, Rs>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, I, Rq, Rs, E, S> {
        MetricsLayerBuilder {
            init_metrics: None,
            set_request_metrics: None,
            set_response_metrics: None,
            with_default_request_metrics: true,
            with_default_response_metrics: true,
            with_request_id_metric: true,
            with_start_metric: true,
            with_operation_name_metric: true,
            with_service_name_metric: true,
            with_service_version_metric: true,
            with_http_status_code: true,
            _state: PhantomData,
        }
    }
}

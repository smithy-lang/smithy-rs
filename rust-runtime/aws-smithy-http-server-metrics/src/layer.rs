#![allow(missing_docs)]

use std::marker::PhantomData;

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::CloseValue;
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

pub trait BuildMetricsLayer {
    type InitMetrics: Fn() -> AppendAndCloseOnDrop<Self::E, Self::S> + Clone;
    type SetReqMetrics: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<Self::E, Self::S>)
        + Clone;
    type SetResMetrics: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<Self::E, Self::S>) + Clone;
    type E: CloseEntry + Send + Sync + 'static;
    type S: EntrySink<RootEntry<<<Self as BuildMetricsLayer>::E as CloseValue>::Closed>>
        + Send
        + Sync
        + 'static;

    fn builder() -> MetricsLayerBuilder<
        NeedsInitialization,
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    >;
}

pub struct MetricsLayer<E = DefaultMetrics, S = DefaultSink>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    _close_entry: PhantomData<E>,
    _entry_sink: PhantomData<S>,
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

impl<E, S> BuildMetricsLayer for MetricsLayer<E, S>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    type InitMetrics = fn() -> AppendAndCloseOnDrop<E, S>;
    type SetReqMetrics = fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>);
    type SetResMetrics = fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>);
    type E = E;
    type S = S;

    fn builder() -> MetricsLayerBuilder<
        NeedsInitialization,
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    > {
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

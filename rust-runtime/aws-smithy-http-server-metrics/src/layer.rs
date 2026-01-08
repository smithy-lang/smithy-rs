#![allow(missing_docs)]

use std::marker::PhantomData;

use aws_smithy_http_server::error::Error;
use http::Request;
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Bytes;
use metrique::AppendAndCloseOnDrop;
use metrique::CloseValue;
use metrique::DefaultSink;
use metrique::OnParentDrop;
use metrique::RootEntry;
use metrique::Slot;
use metrique::writer::EntrySink;
use metrique_core::CloseEntry;
use metrique_macro::metrics;

use inner::MetricsLayer as MetricsLayerInner;

#[doc(hidden)]
pub mod inner;

type ReqBody = hyper::body::Body;
type ResBody = UnsyncBoxBody<Bytes, Error>;

#[metrics]
#[derive(Default)]
pub struct DefaultMetrics {
    #[metrics(flatten)]
    request_metrics: Option<Slot<DefaultRequestMetrics>>,
    #[metrics(flatten)]
    response_metrics: Option<DefaultResponseMetrics>,
}

#[metrics]
#[derive(Default)]
struct DefaultRequestMetrics {
    request_id: Option<String>,
}

#[metrics]
#[derive(Default)]
struct DefaultResponseMetrics {
    http_status_code: Option<u16>,
}

pub trait InitMetricsLayer {
    type InitMetrics: Fn() -> AppendAndCloseOnDrop<Self::E, Self::S> + Clone;
    type SetReqMetrics: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<Self::E, Self::S>)
        + Clone;
    type SetResMetrics: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<Self::E, Self::S>) + Clone;
    type E: CloseEntry + Send + Sync + 'static;
    type S: EntrySink<RootEntry<<<Self as InitMetricsLayer>::E as CloseValue>::Closed>>
        + Send
        + Sync
        + 'static;

    fn new(
        init_metrics: Self::InitMetrics,
    ) -> MetricsLayerInner<
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    >;

    fn builder(
        init_metrics: Self::InitMetrics,
    ) -> MetricsLayerBuilder<
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    >;
}

pub struct MetricsLayer<S, E = DefaultMetrics>
where
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    _close_entry: PhantomData<E>,
    _entry_sink: PhantomData<S>,
}
impl<S> InitMetricsLayer for MetricsLayer<S, DefaultMetrics>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
{
    type InitMetrics = fn() -> AppendAndCloseOnDrop<DefaultMetrics, S>;
    type SetReqMetrics = fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>);
    type SetResMetrics = fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>);
    type E = DefaultMetrics;
    type S = S;

    fn new(
        init_metrics: Self::InitMetrics,
    ) -> MetricsLayerInner<
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    > {
        Self::builder(init_metrics).build()
    }

    fn builder(
        init_metrics: Self::InitMetrics,
    ) -> MetricsLayerBuilder<
        Self::InitMetrics,
        Self::SetReqMetrics,
        Self::SetResMetrics,
        Self::E,
        Self::S,
    > {
        MetricsLayerBuilder {
            init_metrics,
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
        }
    }
}

pub struct MetricsLayerBuilder<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    init_metrics: I,
    set_request_metrics: Option<Rq>,
    set_response_metrics: Option<Rs>,

    with_default_request_metrics: bool,
    with_default_response_metrics: bool,
    with_request_id_metric: bool,
    with_start_metric: bool,
    with_operation_name_metric: bool,
    with_service_name_metric: bool,
    with_service_version_metric: bool,
    with_http_status_code: bool,
}
impl<S>
    MetricsLayerBuilder<
        fn() -> AppendAndCloseOnDrop<DefaultMetrics, S>,
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>),
        fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>),
        DefaultMetrics,
        S,
    >
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Send + Sync + 'static,
{
    pub fn build(
        self,
    ) -> MetricsLayerInner<
        fn() -> AppendAndCloseOnDrop<DefaultMetrics, S>,
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>),
        fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, S>),
        DefaultMetrics,
        S,
    > {
        let set_default_request_metrics = |req: &mut Request<ReqBody>,
                                           metrics: &mut AppendAndCloseOnDrop<
            DefaultMetrics,
            S,
        >| {
            metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));

            let mut default_req_metrics_slotguard = metrics
                    .request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

            default_req_metrics_slotguard.request_id = Some("test_request_id".to_string());

            req.extensions_mut().insert(default_req_metrics_slotguard);
        };

        let set_default_response_metrics =
            |res: &Response<ResBody>, metrics: &mut AppendAndCloseOnDrop<DefaultMetrics, S>| {
                let default_res_metrics = DefaultResponseMetrics {
                    http_status_code: Some(res.status().as_u16()),
                };

                metrics.response_metrics = Some(default_res_metrics);
            };

        MetricsLayerInner {
            init_metrics: self.init_metrics,
            set_default_request_metrics: self
                .with_default_request_metrics
                .then_some(set_default_request_metrics),
            set_default_response_metrics: self
                .with_default_response_metrics
                .then_some(set_default_response_metrics),
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
        }
    }
}

impl<I, Rq, Rs, E, S> MetricsLayerBuilder<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
{
    pub fn without_default_request_metrics(mut self) -> Self {
        self.with_default_request_metrics = false;
        self
    }

    pub fn without_default_response_metrics(mut self) -> Self {
        self.with_default_response_metrics = false;
        self
    }

    pub fn without_request_id_metric(mut self) -> Self {
        self.with_request_id_metric = false;
        self
    }

    pub fn without_start_metric(mut self) -> Self {
        self.with_start_metric = false;
        self
    }

    pub fn without_operation_name_metric(mut self) -> Self {
        self.with_operation_name_metric = false;
        self
    }

    pub fn without_service_name_metric(mut self) -> Self {
        self.with_service_name_metric = false;
        self
    }

    pub fn without_service_version_metric(mut self) -> Self {
        self.with_service_version_metric = false;
        self
    }

    pub fn without_http_status_code(mut self) -> Self {
        self.with_http_status_code = false;
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

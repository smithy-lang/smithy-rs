#![allow(missing_docs)]

use std::sync::Arc;
use std::sync::Mutex;

use aws_smithy_http_server::error::Error;
use http::Request;
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Body as ReqBody;
use hyper::body::Bytes;
use metrique::AppendAndCloseOnDrop;
use metrique::OnParentDrop;
use metrique::RootEntry;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique::writer::EntrySink;
use metrique_core::CloseEntry;
use metrique_macro::metrics;
use metrique_writer::BoxEntrySink;
use metrique_writer::GlobalEntrySink;
use tower::Layer;

use crate::service::MetricsLayerService;

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
    test: Option<String>,
}

pub struct MetricsLayer<I, Rq, Rs, E, S>
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
}

type ResBody = UnsyncBoxBody<Bytes, Error>;

impl
    MetricsLayer<
        fn() -> DefaultMetricsGuard,
        fn(&mut Request<ReqBody>, &mut DefaultMetricsGuard),
        fn(&Response<ResBody>, &mut DefaultMetricsGuard),
        DefaultMetrics,
        BoxEntrySink,
    >
{
    pub fn new() -> MetricsLayer<
        fn() -> AppendAndCloseOnDrop<DefaultMetrics, BoxEntrySink>,
        impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, BoxEntrySink>) + Clone,
        fn(
            &Response<UnsyncBoxBody<Bytes, Error>>,
            &mut AppendAndCloseOnDrop<DefaultMetrics, BoxEntrySink>,
        ),
        DefaultMetrics,
        BoxEntrySink,
    > {
        MetricsLayerBuilder::default().build()
    }
}
impl<I, Rq, Rs, E, S> MetricsLayer<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
}
impl<Ser, I, Rq, Rs, E, S> Layer<Ser> for MetricsLayer<I, Rq, Rs, E, S>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    type Service = MetricsLayerService<Ser, I, Rq, Rs, E, S>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService::builder(inner, self.init_metrics.clone())
            .set_request_metrics(self.set_request_metrics.clone())
            .set_response_metrics(self.set_response_metrics.clone())
            .build()
    }
}

pub struct MetricsLayerBuilder<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
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
impl<I, Rq, Rs, E, S> MetricsLayerBuilder<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
    Rq: for<'a, 'b> Fn(&'a mut Request<ReqBody>, &'b mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: for<'a, 'b> Fn(&'a Response<ResBody>, &'b mut AppendAndCloseOnDrop<E, S>) + Clone,
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

impl<Rq>
    MetricsLayerBuilder<
        fn() -> DefaultMetricsGuard,
        Rq,
        fn(&Response<ResBody>, &mut DefaultMetricsGuard),
        DefaultMetrics,
        BoxEntrySink,
    >
where
    Rq: Fn(&mut Request<ReqBody>, &mut DefaultMetricsGuard) + Clone,
{
    pub fn build(
        self,
    ) -> MetricsLayer<
        fn() -> DefaultMetricsGuard,
        impl Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<DefaultMetrics, BoxEntrySink>) + Clone,
        fn(&Response<ResBody>, &mut DefaultMetricsGuard),
        DefaultMetrics,
        BoxEntrySink,
    > {
        let set_request_metrics =
            move |req: &mut Request<ReqBody>, metrics: &mut DefaultMetricsGuard| {
                if let Some(set_request_metrics) = self.set_request_metrics.clone() {
                    (set_request_metrics)(req, metrics);
                }

                if self.with_default_request_metrics {
                    metrics.request_metrics = Some(Slot::new(DefaultRequestMetrics::default()));

                    let flush_guard = metrics.flush_guard();
                    let mut default_req_metrics_slotguard = metrics
                        .request_metrics
                        .as_mut()
                        .expect("unreachable: the option is set to some in this scope")
                        .open(OnParentDrop::Wait(flush_guard))
                        .expect("unreachable: the slot was created in this scope and is not opened before this point");

                    self.with_request_id_metric.then(|| {
                        default_req_metrics_slotguard.request_id =
                            Some("test_request_id".to_string());
                    });

                    req.extensions_mut().insert(default_req_metrics_slotguard);
                }
            };

        MetricsLayer {
            init_metrics: self.init_metrics,
            set_request_metrics: Some(set_request_metrics),
            set_response_metrics: self.set_response_metrics,
        }
    }
}

impl Default
    for MetricsLayerBuilder<
        fn() -> DefaultMetricsGuard,
        fn(&mut Request<ReqBody>, &mut DefaultMetricsGuard),
        fn(&Response<ResBody>, &mut DefaultMetricsGuard),
        DefaultMetrics,
        BoxEntrySink,
    >
{
    fn default() -> Self {
        Self {
            init_metrics: || DefaultMetrics::default().append_on_drop(ServiceMetrics::sink()),
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

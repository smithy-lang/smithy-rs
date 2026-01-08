use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::RootEntry;
use metrique_core::CloseEntry;
use metrique_writer::EntrySink;
use tower::Layer;

use crate::layer::ReqBody;
use crate::layer::ResBody;
use crate::service::MetricsLayerService;

pub struct MetricsLayer<I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub(crate) init_metrics: I,
    pub(crate) set_default_request_metrics:
        Option<fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)>,
    pub(crate) set_default_response_metrics:
        Option<fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)>,
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
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
            .set_default_request_metrics(self.set_default_request_metrics)
            .set_default_response_metrics(self.set_default_response_metrics)
            .set_request_metrics(self.set_request_metrics.clone())
            .set_response_metrics(self.set_response_metrics.clone())
            .build()
    }
}

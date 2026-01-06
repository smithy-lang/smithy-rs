use std::task::Context;
use std::task::Poll;

use aws_smithy_http_server::error::Error;
use http::Request;
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Body as ReqBody;
use hyper::body::Bytes;
use metrique::AppendAndCloseOnDrop;
use metrique::RootEntry;
use metrique_core::CloseEntry;
use metrique_writer::EntrySink;
use tower::Service;

type ResBody = UnsyncBoxBody<Bytes, Error>;

pub struct MetricsLayerService<Ser, I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    inner: Ser,
    init_metrics: I,
    set_request_metrics: Option<Rq>,
    set_response_metrics: Option<Rs>,
}
impl<Ser, I, Rq, Rs, E, S> Clone for MetricsLayerService<Ser, I, Rq, Rs, E, S>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            init_metrics: self.init_metrics.clone(),
            set_request_metrics: self.set_request_metrics.clone(),
            set_response_metrics: self.set_response_metrics.clone(),
        }
    }
}
impl<Ser, I, Rq, Rs, E, S> Service<Request<ReqBody>> for MetricsLayerService<Ser, I, Rq, Rs, E, S>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    Ser::Future: Send + 'static,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + 'static,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    type Response = Ser::Response;
    type Error = Ser::Error;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let mut metrics = (self.init_metrics)();

        if let Some(set_request_metrics) = &self.set_request_metrics {
            (set_request_metrics)(&mut req, &mut metrics);
        }

        let future = self.inner.call(req);
        let set_response_metrics = self.set_response_metrics.clone();

        futures::FutureExt::boxed(async move {
            let response = match future.await {
                Ok(resp) => resp,
                // if there was an error inside ssdk, short circuit
                Err(e) => return Err(e),
            };

            if let Some(set_response_metrics) = &set_response_metrics {
                (set_response_metrics)(&response, &mut metrics);
            }

            Ok(response)
        })
    }
}
impl<Ser, I, Rq, Rs, E, S> MetricsLayerService<Ser, I, Rq, Rs, E, S>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub(crate) fn builder(
        inner: Ser,
        init_metrics: I,
    ) -> MetricsLayerServiceBuilder<Ser, I, Rq, Rs, E, S> {
        MetricsLayerServiceBuilder {
            inner,
            init_metrics,
            set_request_metrics: Default::default(),
            set_response_metrics: Default::default(),
        }
    }
}

pub(crate) struct MetricsLayerServiceBuilder<Ser, I, Rq, Rs, E, S>
where
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    inner: Ser,
    init_metrics: I,
    set_request_metrics: Option<Rq>,
    set_response_metrics: Option<Rs>,
}
impl<Ser, I, Rq, Rs, E, S> MetricsLayerServiceBuilder<Ser, I, Rq, Rs, E, S>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub(crate) fn set_request_metrics(mut self, f: Option<Rq>) -> Self {
        self.set_request_metrics = f;
        self
    }

    pub(crate) fn set_response_metrics(mut self, f: Option<Rs>) -> Self {
        self.set_response_metrics = f;
        self
    }

    pub(crate) fn build(self) -> MetricsLayerService<Ser, I, Rq, Rs, E, S> {
        MetricsLayerService {
            inner: self.inner,
            init_metrics: self.init_metrics,
            set_request_metrics: self.set_request_metrics,
            set_response_metrics: self.set_response_metrics,
        }
    }
}

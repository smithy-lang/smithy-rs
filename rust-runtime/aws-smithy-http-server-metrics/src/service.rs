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
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    pub(crate) inner: Ser,
    pub(crate) init_metrics: I,
    pub(crate) default_req_metrics_extension_fn:
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>),
    pub(crate) default_res_metrics_extension_fn:
        fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>),
    pub(crate) set_request_metrics: Option<Rq>,
    pub(crate) set_response_metrics: Option<Rs>,
}
impl<Ser, I, Rq, Rs, E, S> Clone for MetricsLayerService<Ser, I, Rq, Rs, E, S>
where
    Ser: Clone,
    I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone,
    Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone,
    E: CloseEntry + Send + Sync + 'static,
    S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            init_metrics: self.init_metrics.clone(),
            default_req_metrics_extension_fn: self.default_req_metrics_extension_fn,
            default_res_metrics_extension_fn: self.default_res_metrics_extension_fn,
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
    Rs: Fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + 'static,
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

        (self.default_req_metrics_extension_fn)(&mut req, &mut metrics);

        if let Some(set_request_metrics) = &self.set_request_metrics {
            (set_request_metrics)(&mut req, &mut metrics);
        }

        let future = self.inner.call(req);
        let default_res_metrics_extension_fn = self.default_res_metrics_extension_fn.clone();
        let set_response_metrics = self.set_response_metrics.clone();

        futures::FutureExt::boxed(async move {
            let mut res = match future.await {
                Ok(res) => res,
                Err(e) => return Err(e),
            };

            (default_res_metrics_extension_fn)(&mut res, &mut metrics);

            if let Some(set_response_metrics) = &set_response_metrics {
                (set_response_metrics)(&mut res, &mut metrics);
            }

            Ok(res)
        })
    }
}

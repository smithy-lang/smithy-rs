use std::ops::Deref;
use std::task::Context;
use std::task::Poll;

use aws_smithy_http_server::operation::OperationShape;
use aws_smithy_http_server::plugin::HttpMarker;
use aws_smithy_http_server::plugin::Plugin;
use aws_smithy_http_server::service::ServiceShape;
use futures::FutureExt;
use http::Request;
use http::Response;
use metrique::OnParentDrop;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique::SlotGuard;
use metrique_writer::GlobalEntrySink;
use tower::Service;

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultResponseMetrics;

pub struct MetricsPlugin {
    pub(crate) with_default_request_metrics: bool,
    pub(crate) with_operation_name: bool,
    pub(crate) with_service_name: bool,
    pub(crate) with_service_version: bool,
}
impl Default for MetricsPlugin {
    fn default() -> Self {
        Self {
            with_default_request_metrics: true,
            with_operation_name: true,
            with_service_name: true,
            with_service_version: true,
        }
    }
}

impl HttpMarker for MetricsPlugin {}

impl<Ser, Op, T> Plugin<Ser, Op, T> for MetricsPlugin
where
    Op: OperationShape,
    Ser: ServiceShape,
{
    type Output = MetricsPluginService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        if !self.with_default_request_metrics {
            return MetricsPluginService {
                inner,
                operation_name: None,
                service_name: None,
                service_version: None,
            };
        }

        let service_name = self.with_service_name.then(|| Ser::ID.name());
        let service_version = self.with_service_version.then(|| Ser::VERSION).flatten();
        let operation_name = self.with_operation_name.then(|| Op::ID.name());

        MetricsPluginService {
            inner,
            operation_name,
            service_name,
            service_version,
        }
    }
}

#[derive(Debug)]
pub struct MetricsPluginService<T> {
    inner: T,
    operation_name: Option<&'static str>,
    service_name: Option<&'static str>,
    service_version: Option<&'static str>,
}

impl<T> Clone for MetricsPluginService<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            operation_name: self.operation_name,
            service_name: self.service_name,
            service_version: self.service_version,
        }
    }
}
impl<Ser> MetricsPluginService<Ser>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Ser::Future: Send + 'static,
{
    fn get_default_request_metrics(&self, req: &Request<ReqBody>) -> DefaultRequestMetrics {
        DefaultRequestMetrics {
            service_name: self.operation_name.map(|n| n.to_string()),
            service_version: self.service_name.map(|n| n.to_string()),
            operation_name: self.service_version.map(|n| n.to_string()),
            request_id: Some("req_id_placeholder".to_string()),
        }
    }

    fn get_default_response_metrics(res: &Response<ResBody>) -> DefaultResponseMetrics {
        DefaultResponseMetrics {
            http_status_code: Some(res.status().as_u16()),
        }
    }
}

impl<Ser> Service<Request<ReqBody>> for MetricsPluginService<Ser>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Ser::Future: Send + 'static,
{
    type Response = Ser::Response;
    type Error = Ser::Error;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let default_request_metrics = self.get_default_request_metrics(&req);

        let maybe_req_metrics = req
            .extensions_mut()
            .get_mut::<SlotGuard<DefaultRequestMetrics>>();

        match maybe_req_metrics {
            Some(req_metrics) => {
                **req_metrics = default_request_metrics;

                req.extensions_mut()
                    .remove::<SlotGuard<DefaultRequestMetrics>>();

                let future = self.inner.call(req);
                return futures::FutureExt::boxed(async move {
                    let mut res = match future.await {
                        Ok(res) => res,
                        Err(e) => return Err(e),
                    };

                    let default_response_metrics = Self::get_default_response_metrics(&res);

                    let maybe_res_metrics = res
                        .extensions_mut()
                        .get_mut::<SlotGuard<DefaultResponseMetrics>>();

                    if let Some(res_metrics) = maybe_res_metrics {
                        **res_metrics = default_response_metrics;
                    }

                    res.extensions_mut()
                        .remove::<SlotGuard<DefaultResponseMetrics>>();

                    Ok(res)
                });
            }
            None => {
                let mut metrics = DefaultMetrics::default().append_on_drop(ServiceMetrics::sink());

                metrics.request_metrics = Some(Slot::new(self.get_default_request_metrics(&req)));
                metrics.request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let future = self.inner.call(req);
                return futures::FutureExt::boxed(async move {
                    let res = match future.await {
                        Ok(res) => res,
                        Err(e) => return Err(e),
                    };

                    metrics.response_metrics =
                        Some(Slot::new(Self::get_default_response_metrics(&res)));
                    metrics.response_metrics
                        .as_mut()
                        .expect("unreachable: the option is set to some in this scope")
                        .open(OnParentDrop::Discard)
                        .expect("unreachable: the slot was created in this scope and is not opened before this point");

                    Ok(res)
                });
            }
        }
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

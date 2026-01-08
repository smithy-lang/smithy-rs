use std::task::Context;
use std::task::Poll;

use aws_smithy_http_server::operation::OperationShape;
use aws_smithy_http_server::plugin::HttpMarker;
use aws_smithy_http_server::plugin::Plugin;
use aws_smithy_http_server::service::ServiceShape;
use futures::FutureExt;
use http::Request;
use http::Response;
use metrique::SlotGuard;
use tower::Service;

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultRequestMetrics;

pub struct MetricsPlugin {
    pub(crate) with_default_request_metrics: bool,
    pub(crate) with_operation_name_metric: bool,
    pub(crate) with_service_name_metric: bool,
    pub(crate) with_service_version_metric: bool,
}
impl Default for MetricsPlugin {
    fn default() -> Self {
        Self {
            with_default_request_metrics: true,
            with_operation_name_metric: true,
            with_service_name_metric: true,
            with_service_version_metric: true,
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

        let service_name = self.with_service_name_metric.then(|| Ser::ID.name());
        let service_version = self
            .with_service_version_metric
            .then(|| Ser::VERSION)
            .flatten();
        let operation_name = self.with_operation_name_metric.then(|| Op::ID.name());

        MetricsPluginService {
            inner,
            operation_name: operation_name,
            service_name: service_name,
            service_version: service_version,
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

    fn call(&mut self, mut request: Request<ReqBody>) -> Self::Future {
        let request_metrics = request
            .extensions_mut()
            .get_mut::<SlotGuard<DefaultRequestMetrics>>();

        if let Some(request_metrics) = request_metrics {
            request_metrics.operation_name = self.operation_name.map(|n| n.to_string());
            request_metrics.service_name = self.service_name.map(|n| n.to_string());
            request_metrics.service_version = self.service_version.map(|n| n.to_string());
        }

        self.inner.call(request).boxed()
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

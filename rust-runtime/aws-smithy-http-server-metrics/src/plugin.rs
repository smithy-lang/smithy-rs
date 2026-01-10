use std::task::Context;
use std::task::Poll;

use aws_smithy_http_server::operation::OperationShape;
use aws_smithy_http_server::plugin::HttpMarker;
use aws_smithy_http_server::plugin::Plugin;
use aws_smithy_http_server::service::ServiceShape;
use http::Request;
use http::Response;
use metrique::OnParentDrop;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique_writer::GlobalEntrySink;
use tower::Service;

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;

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
            service_name: self.service_name.map(|n| n.to_string()),
            service_version: self.service_version.map(|n| n.to_string()),
            operation_name: self.operation_name.map(|n| n.to_string()),
            request_id: Some("req_id_placeholder".to_string()),
        }
    }

    fn get_default_response_metrics(res: &Response<ResBody>) -> DefaultResponseMetrics {
        DefaultResponseMetrics {
            http_status_code: Some(res.status().as_u16()),
        }
    }

    fn apply_default_request_metrics_config(
        metrics: DefaultRequestMetrics,
        config: &DefaultRequestMetricsConfig,
    ) -> DefaultRequestMetrics {
        if config.disable_all {
            return DefaultRequestMetrics::default();
        }

        DefaultRequestMetrics {
            service_name: metrics
                .service_name
                .filter(|_| !config.disable_service_name),
            service_version: metrics
                .service_version
                .filter(|_| !config.disable_service_version),
            operation_name: metrics
                .operation_name
                .filter(|_| !config.disable_operation_name),
            request_id: metrics.request_id.filter(|_| !config.disable_request_id),
        }
    }

    fn apply_default_response_metrics_config(
        metrics: DefaultResponseMetrics,
        config: &DefaultResponseMetricsConfig,
    ) -> DefaultResponseMetrics {
        if config.disable_all {
            return DefaultResponseMetrics::default();
        }

        DefaultResponseMetrics {
            http_status_code: metrics
                .http_status_code
                .filter(|_| !config.disable_http_status_code),
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

        let maybe_default_req_metrics_ext = req
            .extensions_mut()
            .get_mut::<DefaultRequestMetricsExtension>();

        match maybe_default_req_metrics_ext {
            Some(default_req_metrics_ext) => {
                *default_req_metrics_ext.metrics = Self::apply_default_request_metrics_config(
                    default_request_metrics,
                    &default_req_metrics_ext.config,
                );

                req.extensions_mut()
                    .remove::<DefaultRequestMetricsExtension>();

                let future = self.inner.call(req);
                return futures::FutureExt::boxed(async move {
                    let mut res = match future.await {
                        Ok(res) => res,
                        Err(e) => return Err(e),
                    };

                    let default_response_metrics = Self::get_default_response_metrics(&res);

                    let maybe_default_res_metrics_ext =
                        res.extensions_mut()
                            .get_mut::<DefaultResponseMetricsExtension>();

                    if let Some(default_res_metrics_ext) = maybe_default_res_metrics_ext {
                        *default_res_metrics_ext.metrics =
                            Self::apply_default_response_metrics_config(
                                default_response_metrics,
                                &default_res_metrics_ext.config,
                            );
                    }

                    res.extensions_mut()
                        .remove::<DefaultResponseMetricsExtension>();

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

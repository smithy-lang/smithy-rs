use std::future::Future;
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
use metrique_writer::AttachGlobalEntrySink;
use tower::Service;

use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;
use crate::types::ReqBody;
use crate::types::ResBody;

#[derive(Default)]
pub struct DefaultMetricsPlugin;

impl HttpMarker for DefaultMetricsPlugin {}

impl<Ser, Op, T> Plugin<Ser, Op, T> for DefaultMetricsPlugin
where
    Op: OperationShape,
    Ser: ServiceShape,
{
    type Output = DefaultMetricsPluginService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        DefaultMetricsPluginService {
            inner,
            service_name: Ser::ID.name(),
            service_version: Ser::VERSION,
            operation_name: Op::ID.name(),
        }
    }
}

#[derive(Debug)]
pub struct DefaultMetricsPluginService<Ser> {
    inner: Ser,
    service_name: &'static str,
    service_version: Option<&'static str>,
    operation_name: &'static str,
}

impl<Ser> Clone for DefaultMetricsPluginService<Ser>
where
    Ser: Clone,
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
impl<Ser> DefaultMetricsPluginService<Ser>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Ser::Future: Send + 'static,
{
    fn get_default_request_metrics(&self, _req: &Request<ReqBody>) -> DefaultRequestMetrics {
        DefaultRequestMetrics {
            service_name: Some(self.service_name.to_string()),
            service_version: self.service_version.map(|n| n.to_string()),
            operation_name: Some(self.operation_name.to_string()),
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

impl<Ser> Service<Request<ReqBody>> for DefaultMetricsPluginService<Ser>
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
                futures::FutureExt::boxed(async move {
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
                })
            }
            None => {
                // When no outer layer exists, provide the default metrics through metrique's
                // application-wide global metric sink, if an underlying sink has been attached
                let Some(sink) = ServiceMetrics::try_sink() else {
                    return self.inner.call(req).boxed();
                };

                let mut metrics = DefaultMetrics::default().append_on_drop(sink);

                metrics.default_request_metrics =
                    Some(Slot::new(self.get_default_request_metrics(&req)));
                metrics.default_request_metrics
                    .as_mut()
                    .expect("unreachable: the option is set to some in this scope")
                    .open(OnParentDrop::Discard)
                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                let future = self.inner.call(req);
                futures::FutureExt::boxed(async move {
                    let res = match future.await {
                        Ok(res) => res,
                        Err(e) => return Err(e),
                    };

                    metrics.default_response_metrics =
                        Some(Slot::new(Self::get_default_response_metrics(&res)));
                    metrics.default_response_metrics
                        .as_mut()
                        .expect("unreachable: the option is set to some in this scope")
                        .open(OnParentDrop::Discard)
                        .expect("unreachable: the slot was created in this scope and is not opened before this point");

                    Ok(res)
                })
            }
        }
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

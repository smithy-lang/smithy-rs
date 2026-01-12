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
use metrique::RootEntry;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique_writer::AttachGlobalEntrySink;
use metrique_writer::BoxEntrySink;
use metrique_writer::EntrySink;
use tower::Service;

use crate::ReqBody;
use crate::ResBody;
use crate::default::DefaultMetrics;
use crate::default::DefaultMetricsEntry;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultRequestMetricsExtension;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;

pub struct MetricsPlugin<S = BoxEntrySink>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
{
    sink: Option<S>,
}
impl<S> MetricsPlugin<S>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
{
    pub fn new_with_sink(sink: S) -> Self {
        Self { sink: Some(sink) }
    }
}
impl MetricsPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Default for MetricsPlugin {
    fn default() -> Self {
        Self { sink: None }
    }
}

impl<S> HttpMarker for MetricsPlugin<S> where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static
{
}

impl<Ser, Op, T, S> Plugin<Ser, Op, T> for MetricsPlugin<S>
where
    Op: OperationShape,
    Ser: ServiceShape,
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
{
    type Output = MetricsPluginService<T, S>;

    fn apply(&self, inner: T) -> Self::Output {
        MetricsPluginService {
            inner,
            service_name: Ser::ID.name(),
            service_version: Ser::VERSION,
            operation_name: Op::ID.name(),
            sink: self.sink.clone(),
        }
    }
}

#[derive(Debug)]
pub struct MetricsPluginService<Ser, S>
where
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
{
    inner: Ser,
    service_name: &'static str,
    service_version: Option<&'static str>,
    operation_name: &'static str,
    sink: Option<S>,
}

impl<Ser, S> Clone for MetricsPluginService<Ser, S>
where
    Ser: Clone,
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            operation_name: self.operation_name,
            service_name: self.service_name,
            service_version: self.service_version,
            sink: self.sink.clone(),
        }
    }
}
impl<Ser, S> MetricsPluginService<Ser, S>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Ser::Future: Send + 'static,
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
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

    fn handle_metrics_with_sink<T>(
        &mut self,
        sink: T,
        req: Request<ReqBody>,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<Response<ResBody>, Ser::Error>> + Send + 'static>,
    >
    where
        T: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
    {
        let mut metrics = DefaultMetrics::default().append_on_drop(sink);
        metrics.request_metrics = Some(Slot::new(self.get_default_request_metrics(&req)));
        metrics.request_metrics
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

            metrics.response_metrics = Some(Slot::new(Self::get_default_response_metrics(&res)));
            metrics.response_metrics
                .as_mut()
                .expect("unreachable: the option is set to some in this scope")
                .open(OnParentDrop::Discard)
                .expect("unreachable: the slot was created in this scope and is not opened before this point");

            Ok(res)
        })
    }
}

impl<Ser, S> Service<Request<ReqBody>> for MetricsPluginService<Ser, S>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Ser::Future: Send + 'static,
    S: EntrySink<RootEntry<DefaultMetricsEntry>> + Clone + Send + Sync + 'static,
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
                if let Some(sink) = self.sink.clone() {
                    return self.handle_metrics_with_sink(sink, req);
                } else if let Some(sink) = ServiceMetrics::try_sink() {
                    return self.handle_metrics_with_sink(sink, req);
                } else {
                    return self.inner.call(req).boxed();
                }
            }
        }
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use aws_smithy_http_server::operation::OperationShape;
use aws_smithy_http_server::plugin::HttpMarker;
use aws_smithy_http_server::plugin::Plugin;
use aws_smithy_http_server::request::request_id::ServerRequestId;
use aws_smithy_http_server::service::ServiceShape;
use http::Request;
use http::Response;
use metrique::timers::OwnedTimerGuard;
use metrique::timers::Stopwatch;
use metrique::OnParentDrop;
use metrique::ServiceMetrics;
use metrique::Slot;
use metrique_writer::AttachGlobalEntrySink;
use pin_project_lite::pin_project;
use tower::Service;
use tracing;

use crate::default::DefaultMetrics;
use crate::default::DefaultMetricsExtension;
use crate::default::DefaultRequestMetrics;
use crate::default::DefaultResponseMetrics;
use crate::default::DefaultResponseMetricsConfig;
use crate::default::DefaultResponseMetricsExtension;
use crate::types::ReqBody;
use crate::types::ResBody;

pin_project! {
    /// Future returned by [`DefaultMetricsPluginService`].
    ///
    /// This type is not intended to be used directly. See [`DefaultMetricsPlugin`].
    #[project = DefaultMetricsFutureProj]
    pub enum DefaultMetricsFuture<F> {
        /// Metrics extension from outer layer
        WithExtension {
            #[pin]
            inner: F,
            response_ext: DefaultResponseMetricsExtension,
            operation_timer_guard: Option<OwnedTimerGuard>
        },
        /// No outer metrics layer, but sink available
        WithMetrics {
            #[pin]
            inner: F,
            metrics: metrique::AppendAndCloseOnDrop<DefaultMetrics, metrique_writer::BoxEntrySink>,
            operation_timer_guard: Option<OwnedTimerGuard>
        },
        /// No sink available
        Passthrough {
            #[pin]
            inner: F,
        },
    }
}

impl<F, Err> Future for DefaultMetricsFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, Err>>,
{
    type Output = Result<Response<ResBody>, Err>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            DefaultMetricsFutureProj::WithExtension {
                inner,
                response_ext,
                operation_timer_guard,
            } => match inner.poll(cx) {
                Poll::Ready(Ok(res)) => {
                    let operation_time = operation_timer_guard.take().map(|guard| guard.stop());

                    let default_response_metrics =
                        get_default_response_metrics(&res, operation_time);
                    let configured_default_response_metrics = configure_default_response_metrics(
                        default_response_metrics,
                        &response_ext.config,
                    );

                    response_ext.metrics.lock().map_or_else(
                    |e| {
                        tracing::error!(
                            "Failed to acquire lock on DefaultResponseMetrics with error {e}. Metrics may be incomplete."
                        )
                    },
                    |mut metrics| **metrics = configured_default_response_metrics,
                );

                    Poll::Ready(Ok(res))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            },
            DefaultMetricsFutureProj::WithMetrics {
                inner,
                metrics,
                operation_timer_guard,
            } => match inner.poll(cx) {
                Poll::Ready(Ok(res)) => {
                    let operation_time = operation_timer_guard.take().map(|guard| guard.stop());

                    metrics.default_response_metrics = Some(Slot::new(
                        get_default_response_metrics(&res, operation_time),
                    ));
                    metrics
                        .default_response_metrics
                        .as_mut()
                        .and_then(|slot| slot.open(OnParentDrop::Discard))
                        .expect("unreachable: the option is set to a created slot in this scope");

                    Poll::Ready(Ok(res))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            },
            DefaultMetricsFutureProj::Passthrough { inner } => inner.poll(cx),
        }
    }
}

/// Plugin that automatically collects default metrics for all operations.
///
/// This plugin adds standard metrics to every operation without requiring manual configuration.
/// For a complete list of collected metrics, see the [crate-level documentation](crate).
///
/// # Usage
///
/// Apply to your service using the plugin system:
///
/// ```rust,ignore
/// use aws_smithy_http_server_metrics::DefaultMetricsPlugin;
///
/// let http_plugins = HttpPlugins::new()
///     .push(DefaultMetricsPlugin)
///     .instrument();
/// ```
///
/// # Integration with MetricsLayer
///
/// This plugin works with [`MetricsLayer`](crate::layer::MetricsLayer).
/// When both are used together, default metrics from this plugin are automatically
/// folded with custom metrics from the layer.
///
/// # Metrics Sink
///
/// Metrics are emitted to the global sink configured via [`ServiceMetrics::attach`]
/// or [`attach_to_stream`](metrique_writer::AttachGlobalEntrySinkExt::attach_to_stream).
/// If no sink is attached, metrics collection is skipped.
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

/// Service wrapper that collects default metrics for operations.
///
/// Created by applying [`DefaultMetricsPlugin`] to a service.
///
/// This type is not intended to be used directly. See [`DefaultMetricsPlugin`].
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
    /// Gets the default request metrics that can be retrieved from the request object directly
    ///
    /// Assigns None to those that need information from the outer metrics layer to be set
    fn get_default_request_metrics(&self, req: &Request<ReqBody>) -> DefaultRequestMetrics {
        DefaultRequestMetrics {
            service_name: Some(self.service_name.to_string()),
            service_version: self.service_version.map(|n| n.to_string()),
            operation_name: Some(self.operation_name.to_string()),
            request_id: req
                .extensions()
                .get::<ServerRequestId>()
                .map(|id| id.to_string()),
            outstanding_requests: None,
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
    type Future = DefaultMetricsFuture<Ser::Future>;

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let mut stopwatch = Stopwatch::new();
        let operation_timer_guard = stopwatch.start_owned();

        let default_request_metrics = self.get_default_request_metrics(&req);

        let maybe_default_metrics_ext = req.extensions_mut().get_mut::<DefaultMetricsExtension>();

        match maybe_default_metrics_ext {
            Some(ext) => {
                let extended_default_request_metrics =
                    extend_default_request_metrics(default_request_metrics, ext);

                ext.request_ext.metrics.lock().map_or_else(
                    |e| {
                        tracing::error!(
                            "Failed to acquire lock on DefaultRequestMetrics with error {e}. Metrics may be incomplete."
                        )
                    },
                    |mut metrics| **metrics = extended_default_request_metrics,
                );

                let response_ext = ext.response_ext.clone();

                req.extensions_mut().remove::<DefaultMetricsExtension>();

                DefaultMetricsFuture::WithExtension {
                    inner: self.inner.call(req),
                    response_ext,
                    operation_timer_guard: Some(operation_timer_guard),
                }
            }
            None => {
                // When no outer layer exists, provide the default metrics through metrique's
                // application-wide global metric sink, if an underlying sink has been attached
                let Some(sink) = ServiceMetrics::try_sink() else {
                    tracing::info!(
                        "Default metrics collection skipped. No metrics sink configured. \
                        Use ServiceMetrics::attach() or ServiceMetrics::attach_to_stream() \
                        to enable metrics using metrique's globally provided sink."
                    );
                    return DefaultMetricsFuture::Passthrough {
                        inner: self.inner.call(req),
                    };
                };

                let mut metrics = DefaultMetrics::default().append_on_drop(sink);

                metrics.default_request_metrics = Some(Slot::new(default_request_metrics));

                metrics
                    .default_request_metrics
                    .as_mut()
                    .and_then(|slot| slot.open(OnParentDrop::Discard))
                    .expect("unreachable: the option is set to a created slot in this scope");

                DefaultMetricsFuture::WithMetrics {
                    inner: self.inner.call(req),
                    metrics,
                    operation_timer_guard: Some(operation_timer_guard),
                }
            }
        }
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

fn get_default_response_metrics(
    res: &Response<ResBody>,
    operation_time: Option<Duration>,
) -> DefaultResponseMetrics {
    let status = res.status();
    let status_code = status.as_u16();

    let error = if (400..500).contains(&status_code) {
        Some(1)
    } else {
        Some(0)
    };

    let fault = if status_code >= 500 { Some(1) } else { Some(0) };

    DefaultResponseMetrics {
        http_status_code: Some(status_code),
        error,
        fault,
        operation_time,
    }
}

fn extend_default_request_metrics(
    metrics: DefaultRequestMetrics,
    ext: &DefaultMetricsExtension,
) -> DefaultRequestMetrics {
    let config = &ext.request_ext.config;

    if config.disable_all {
        return DefaultRequestMetrics::default();
    }

    let outstanding_requests = (!config.disable_outstanding_requests).then_some(
        ext.service_state
            .outstanding_requests_counter
            .load(Ordering::Relaxed),
    );

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
        outstanding_requests,
    }
}

fn configure_default_response_metrics(
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
        error: metrics.error.filter(|_| !config.disable_error),
        fault: metrics.fault.filter(|_| !config.disable_fault),
        operation_time: metrics
            .operation_time
            .filter(|_| !config.disable_operation_time),
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use pin_project_lite::pin_project;
use tower::Service;

use crate::default::service_counter::ServiceCounterGuard;
use crate::default::DefaultMetricsServiceCounters;
use crate::default::DefaultMetricsServiceState;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::traits::InitMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::HttpRequest;
use crate::types::HttpResponse;

pin_project! {
    /// Future returned by [`MetricsLayerService`].
    ///
    /// This type is not intended to be used directly. See [`MetricsLayer`](crate::layer::MetricsLayer).
    pub struct MetricsLayerServiceFuture<F, Entry, Sink, Res>
    where
        F: Future,
        Entry: ThreadSafeCloseEntry,
        Sink: ThreadSafeEntrySink<Entry>,
        Res: ResponseMetrics<Entry>,
    {
        #[pin]
        inner: F,
        metrics: metrique::AppendAndCloseOnDrop<Entry, Sink>,
        response_metrics: Option<Res>,
        default_service_state: DefaultMetricsServiceCounters,
        outstanding_requests_counter_guard: ServiceCounterGuard
    }
}

impl<F, Entry, Sink, Res, Err> Future for MetricsLayerServiceFuture<F, Entry, Sink, Res>
where
    F: Future<Output = Result<HttpResponse, Err>>,
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Res: ResponseMetrics<Entry>,
{
    type Output = Result<HttpResponse, Err>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // for safely accessing the pinned inner future
        let this = self.project();

        match this.inner.poll(cx) {
            Poll::Ready(Ok(mut res)) => {
                if let Some(response_metrics) = this.response_metrics {
                    (response_metrics)(&mut res, this.metrics);
                }

                Poll::Ready(Ok(res))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Tower service that collects metrics for HTTP requests and responses.
///
/// This type is not intended to be used directly. See [`MetricsLayer`](crate::layer::MetricsLayer).
pub struct MetricsLayerService<Ser, Entry, Sink, Init, Res>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    pub(crate) inner: Ser,
    pub(crate) init_metrics: Init,
    pub(crate) response_metrics: Option<Res>,
    pub(crate) default_metrics_extension_fn: fn(
        &mut HttpRequest,
        &mut Entry,
        DefaultRequestMetricsConfig,
        DefaultResponseMetricsConfig,
        DefaultMetricsServiceState,
    ),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,
    pub(crate) default_service_counters: DefaultMetricsServiceCounters,

    pub(crate) _entry_sink: PhantomData<Sink>,
}
impl<Ser, Entry, Sink, Init, Res> Clone for MetricsLayerService<Ser, Entry, Sink, Init, Res>
where
    Ser: Clone,
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            init_metrics: self.init_metrics.clone(),
            response_metrics: self.response_metrics.clone(),
            default_metrics_extension_fn: self.default_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),
            default_service_counters: self.default_service_counters.clone(),

            _entry_sink: PhantomData,
        }
    }
}
impl<Ser, Entry, Sink, Init, Res> Service<HttpRequest>
    for MetricsLayerService<Ser, Entry, Sink, Init, Res>
where
    Ser: Service<HttpRequest, Response = HttpResponse> + Clone,
    Ser::Future: Send + 'static,
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    type Response = Ser::Response;
    type Error = Ser::Error;
    type Future = MetricsLayerServiceFuture<Ser::Future, Entry, Sink, Res>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: HttpRequest) -> Self::Future {
        let mut metrics = (self.init_metrics)(&mut req);

        // We increment the outstanding requests and get the count at this layer
        // (typically outer middleware) so we can get this as close to the time
        // this request entered the system as possible
        let (outstanding_requests_counter_guard, outstanding_requests) = self
            .default_service_counters
            .outstanding_requests_counter
            .increment();

        (self.default_metrics_extension_fn)(
            &mut req,
            &mut metrics,
            self.default_req_metrics_config.clone(),
            self.default_res_metrics_config.clone(),
            DefaultMetricsServiceState {
                outstanding_requests,
            },
        );

        MetricsLayerServiceFuture {
            inner: self.inner.call(req),
            metrics,
            response_metrics: self.response_metrics.clone(),
            default_service_state: self.default_service_counters.clone(),
            outstanding_requests_counter_guard,
        }
    }
}

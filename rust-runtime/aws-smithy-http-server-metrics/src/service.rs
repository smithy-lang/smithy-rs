/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::task::Context;
use std::task::Poll;

use metrique::AppendAndCloseOnDrop;
use pin_project_lite::pin_project;
use tower::Service;

use crate::default::DefaultMetrics;
use crate::default::DefaultMetricsConfig;
use crate::default::HasSmithyMetrics;
use crate::pool::SmithyMetricsPool;
use crate::traits::InitMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::aws_smithy_http_server::metrics::RequestExt;
use crate::types::HttpRequest;
use crate::types::HttpResponse;

/// A request's metrics entry, shared by the response future, which records response metrics into
/// it, and the request's finish callback, which records the default metrics and emits it.
///
/// The entry is emitted when it is taken out and dropped by the finish callback, or otherwise when
/// the last reference is dropped.
type SharedEntry<Entry, Sink> = Arc<Mutex<Option<AppendAndCloseOnDrop<Entry, Sink>>>>;

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
        metrics: SharedEntry<Entry, Sink>,
        response_metrics: Option<Res>,
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
                    let mut entry = this.metrics.lock().unwrap_or_else(PoisonError::into_inner);
                    // The entry is gone only if the request has already finished.
                    if let Some(entry) = entry.as_mut() {
                        (response_metrics)(&mut res, entry);
                    }
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
    pub(crate) init_extensions: fn(&mut HttpRequest, &mut Entry),
    pub(crate) default_metrics_config: DefaultMetricsConfig,
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
            init_extensions: self.init_extensions,
            default_metrics_config: self.default_metrics_config.clone(),
            _entry_sink: PhantomData,
        }
    }
}

impl<Ser, Entry, Sink, Init, Res> Service<HttpRequest>
    for MetricsLayerService<Ser, Entry, Sink, Init, Res>
where
    Ser: Service<HttpRequest, Response = HttpResponse> + Clone,
    Ser::Future: Send + 'static,
    Entry: ThreadSafeCloseEntry + HasSmithyMetrics,
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

        // Obtain a producer handle for this request's metrics pool and store it
        // in the request extensions as a `SmithyMetricsPool`. Independently-owned
        // middleware and libraries that hold the request can pull it out and
        // contribute child metrics that flatten into this request's entry.
        let pool_handle = metrics.smithy_metrics().metrics_pool.handle();
        req.extensions_mut()
            .insert(SmithyMetricsPool::new(pool_handle));

        (self.init_extensions)(&mut req, &mut metrics);

        let metrics: SharedEntry<Entry, Sink> = Arc::new(Mutex::new(Some(metrics)));
        let config = self.default_metrics_config.clone();
        req.on_finish({
            let metrics = metrics.clone();
            move |request_metrics| {
                let entry = metrics
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let (Some(mut entry), Some(request_metrics)) = (entry, request_metrics) {
                    entry.smithy_metrics_mut().default_metrics =
                        DefaultMetrics::new(&request_metrics, &config);
                    // Emits the entry.
                    drop(entry);
                }
            }
        });

        MetricsLayerServiceFuture {
            inner: self.inner.call(req),
            metrics,
            response_metrics: self.response_metrics.clone(),
        }
    }
}

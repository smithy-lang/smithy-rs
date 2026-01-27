/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use http::Request;
use http::Response;
use pin_project_lite::pin_project;
use tower::Service;

use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::traits::InitMetrics;
use crate::traits::RequestMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::ReqBody;
use crate::types::ResBody;

pin_project! {
    /// Named future to avoid heap allocation
    ///
    /// `inner` has `#[pin]` to prevent moving in memory once polled.
    pub struct MetricsLayerServiceFuture<F, E, S, Rs>
    where
        F: Future,
        E: ThreadSafeCloseEntry,
        S: ThreadSafeEntrySink<E>,
        Rs: ResponseMetrics<E>,
    {
        #[pin]
        inner: F,
        metrics: metrique::AppendAndCloseOnDrop<E, S>,
        response_metrics: Option<Rs>,
    }
}

impl<F, E, S, Rs, Err> Future for MetricsLayerServiceFuture<F, E, S, Rs>
where
    F: Future<Output = Result<Response<ResBody>, Err>>,
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    Rs: ResponseMetrics<E>,
{
    type Output = Result<Response<ResBody>, Err>;

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

pub struct MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    pub(crate) inner: Ser,
    pub(crate) init_metrics: I,
    pub(crate) request_metrics: Option<Rq>,
    pub(crate) response_metrics: Option<Rs>,
    pub(crate) default_metrics_extension_fn: fn(
        &mut Request<ReqBody>,
        &mut E,
        DefaultRequestMetricsConfig,
        DefaultResponseMetricsConfig,
    ),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,

    pub(crate) _entry_sink: PhantomData<S>,
}
impl<Ser, E, S, I, Rq, Rs> Clone for MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    Ser: Clone,
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            init_metrics: self.init_metrics.clone(),
            request_metrics: self.request_metrics.clone(),
            response_metrics: self.response_metrics.clone(),
            default_metrics_extension_fn: self.default_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),

            _entry_sink: PhantomData,
        }
    }
}
impl<Ser, E, S, I, Rq, Rs> Service<Request<ReqBody>> for MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    Ser::Future: Send + 'static,
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    type Response = Ser::Response;
    type Error = Ser::Error;
    type Future = MetricsLayerServiceFuture<Ser::Future, E, S, Rs>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let mut metrics = (self.init_metrics)();

        (self.default_metrics_extension_fn)(
            &mut req,
            &mut metrics,
            self.default_req_metrics_config.clone(),
            self.default_res_metrics_config.clone(),
        );

        if let Some(request_metrics) = &self.request_metrics {
            (request_metrics)(&mut req, &mut metrics);
        }

        // Return named future type instead of boxing - zero heap allocation
        MetricsLayerServiceFuture {
            inner: self.inner.call(req),
            metrics,
            response_metrics: self.response_metrics.clone(),
        }
    }
}

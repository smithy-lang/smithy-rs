/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::task::Context;
use std::task::Poll;

use aws_smithy_http_server::error::Error;
use http::Request;
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Body as ReqBody;
use hyper::body::Bytes;
use metrique::AppendAndCloseOnDrop;
use tower::Service;

use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::traits::InitMetrics;
use crate::traits::MetriqueCloseEntry;
use crate::traits::MetriqueEntrySink;
use crate::traits::RequestMetrics;
use crate::traits::ResponseMetrics;

type ResBody = UnsyncBoxBody<Bytes, Error>;

pub struct MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E, S>,
    Rs: ResponseMetrics<E, S>,
{
    pub(crate) inner: Ser,
    pub(crate) init_metrics: I,
    pub(crate) request_metrics: Option<Rq>,
    pub(crate) response_metrics: Option<Rs>,
    pub(crate) default_req_metrics_extension_fn:
        fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultRequestMetricsConfig),
    pub(crate) default_res_metrics_extension_fn:
        fn(&mut Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>, DefaultResponseMetricsConfig),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,
}
impl<Ser, E, S, I, Rq, Rs> Clone for MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    Ser: Clone,
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E, S>,
    Rs: ResponseMetrics<E, S>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            init_metrics: self.init_metrics.clone(),
            request_metrics: self.request_metrics.clone(),
            response_metrics: self.response_metrics.clone(),
            default_req_metrics_extension_fn: self.default_req_metrics_extension_fn,
            default_res_metrics_extension_fn: self.default_res_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),
        }
    }
}
impl<Ser, E, S, I, Rq, Rs> Service<Request<ReqBody>> for MetricsLayerService<Ser, E, S, I, Rq, Rs>
where
    Ser: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    Ser::Future: Send + 'static,
    E: MetriqueCloseEntry,
    S: MetriqueEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E, S>,
    Rs: ResponseMetrics<E, S>,
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

        (self.default_req_metrics_extension_fn)(
            &mut req,
            &mut metrics,
            self.default_req_metrics_config.clone(),
        );

        if let Some(request_metrics) = &self.request_metrics {
            (request_metrics)(&mut req, &mut metrics);
        }

        let future = self.inner.call(req);
        let default_res_metrics_extension_fn = self.default_res_metrics_extension_fn;
        let default_res_metrics_config = self.default_res_metrics_config.clone();
        let response_metrics = self.response_metrics.clone();

        futures::FutureExt::boxed(async move {
            let mut res = match future.await {
                Ok(res) => res,
                Err(e) => return Err(e),
            };

            (default_res_metrics_extension_fn)(&mut res, &mut metrics, default_res_metrics_config);

            if let Some(response_metrics) = &response_metrics {
                (response_metrics)(&mut res, &mut metrics);
            }

            Ok(res)
        })
    }
}

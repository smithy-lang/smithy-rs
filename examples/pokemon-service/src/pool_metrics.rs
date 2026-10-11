/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Example of independently-owned middleware contributing to the per-request
//! metrics entry through [`SmithyMetricsPool`].
//!
//! This is an HTTP-level [`tower`] layer: it holds the raw request, so it can
//! read [`SmithyMetricsPool`] from the request extensions where [`MetricsLayer`]
//! stored it. It cannot name the service's metrics entry type, so it appends a
//! prefixed child metrics struct to the pool. The child flattens into the single
//! per-request entry when the request finishes.

use std::task::{Context, Poll};

use aws_smithy_http_server_metrics::SmithyMetricsPool;
use metrique::timers::Timer;
use metrique::unit::Millisecond;
use metrique::unit_of_work::metrics;
use tower::{Layer, Service};

/// Middleware timing contributed to the per-request metrics entry through the
/// pool. Prefixing with `middleware` keeps these field names from colliding with
/// the service's own metrics; here it emits as `middleware_time`.
#[metrics]
#[derive(Default)]
struct MiddlewareMetrics {
    #[metrics(unit = Millisecond)]
    time: Timer,
}

/// Layer that wraps a service with [`PoolContributingService`].
#[derive(Clone, Default)]
pub struct PoolContributingLayer;

impl<S> Layer<S> for PoolContributingLayer {
    type Service = PoolContributingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PoolContributingService { inner }
    }
}

/// Reads [`SmithyMetricsPool`] from the request extensions and contributes a
/// timing metric for the time spent in the inner service.
#[derive(Clone)]
pub struct PoolContributingService<S> {
    inner: S,
}

impl<S, B> Service<http::Request<B>> for PoolContributingService<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        // Independently-owned middleware cannot name the service's metrics entry
        // type, so it pulls the pool handle out of the request extensions and
        // appends a prefixed child metric that flattens into the request's entry.
        if let Some(pool) = req.extensions().get::<SmithyMetricsPool>() {
            pool.with_prefix(["middleware"]).append(MiddlewareMetrics {
                time: Timer::start_now(),
            });
        }
        self.inner.call(req)
    }
}

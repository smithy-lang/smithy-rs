/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use http::Request;
use metrique::DefaultSink;
use thiserror::Error;
use tower::Layer;

use crate::default::DefaultMetrics;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::layer::builder::DefaultMetricsBuildExt;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::service::MetricsLayerService;
use crate::traits::InitMetrics;
use crate::traits::RequestMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::DefaultInit;
use crate::types::DefaultRq;
use crate::types::DefaultRs;
use crate::types::ReqBody;

pub mod builder;

/// Errors that can occur when creating a metrics layer
#[derive(Error, Debug)]
pub enum DefaultMetricsLayerError {
    /// No sink was attached to the service metrics
    #[error("No sink attached to [`metrique::ServiceMetrics`]")]
    NoSinkAttached,
}

/// A Tower layer that collects metrics for HTTP requests and responses, allowing you to define
/// and initialize metrics that can be set throughout a request/response lifecycle.
///
/// Default metrics from [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin)
/// will be folded into metrics from this layer if it is in the service's HTTP plugins.
///
/// # Usage
///
/// Typically, this should be layered as the outermost middleware layer, so that the rest of
/// the request path has access to metrics request extensions added by this, and this has access
/// to any metrics response extensions added by the response path.
///
/// Create a metrics layer using the builder pattern:
///
/// ```rust,ignore
/// use aws_smithy_http_server_metrics::MetricsLayer;
///
/// let metrics_layer = MetricsLayer::builder()
///     .init_metrics(|| MyMetrics::default().append_on_drop(sink))
///     .request_metrics(|request, metrics| {
///         // Populate custom metrics from request, e.g.
///         metrics.user_agent = request.headers()
///             .get("user-agent")
///             .and_then(|v| v.to_str().ok())
///             .map(Into::into);
///     })
///     .response_metrics(|response, metrics| {
///         // Populate custom metrics from response
///     })
///     .build();
/// ```
///
/// Then add it to your service as outer middleware, typically layered after any other middleware
/// to ensure that it is layered at the outermost level:
///
/// ```rust,ignore
/// let app = app.layer(metrics_layer);
/// ```
///
/// If you just want a custom sink with the default metrics, provided
/// [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin) exists in the HTTP plugins,
/// use [`new_with_sink()`](MetricsLayer::new_with_sink):
///
/// ```rust,ignore
/// let metrics_layer = MetricsLayer::new_with_sink(my_sink);
/// let app = app.layer(metrics_layer);
/// ```
#[derive(Debug)]
pub struct MetricsLayer<
    E = DefaultMetrics,
    S = DefaultSink,
    I = DefaultInit<E, S>,
    Rq = DefaultRq<E>,
    Rs = DefaultRs<E>,
> where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
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

impl<E, S, I, Rq, Rs> MetricsLayer<E, S, I, Rq, Rs>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    #[doc(hidden)]
    pub fn __macro_new(
        init_metrics: I,
        request_metrics: Option<Rq>,
        response_metrics: Option<Rs>,
        default_metrics_extension_fn: fn(
            &mut Request<ReqBody>,
            &mut E,
            DefaultRequestMetricsConfig,
            DefaultResponseMetricsConfig,
        ),
        default_req_metrics_config: DefaultRequestMetricsConfig,
        default_res_metrics_config: DefaultResponseMetricsConfig,
    ) -> Self {
        Self {
            init_metrics,
            request_metrics,
            response_metrics,
            default_metrics_extension_fn,
            default_req_metrics_config,
            default_res_metrics_config,

            _entry_sink: PhantomData,
        }
    }
}

impl<S> MetricsLayer<DefaultMetrics, S>
where
    S: ThreadSafeEntrySink<DefaultMetrics> + Clone,
{
    /// Creates a new metrics layer with the provided sink and default metrics. For default
    /// metrics to be set automatically, [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin)
    /// must be added to the service's HTTP plugins.
    ///
    /// This is a convenience method for when you want to have a custom sink with the default
    /// metrics configuration.
    ///
    /// See [`metrique::EntrySink`].
    ///
    /// For additional control, use [`MetricsLayer::builder()`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = MetricsLayer::new_with_sink(my_sink);
    /// ```
    pub fn new_with_sink(
        sink: S,
    ) -> MetricsLayer<DefaultMetrics, S, impl InitMetrics<DefaultMetrics, S>> {
        Self::builder()
            .init_metrics(move || DefaultMetrics::default().append_on_drop(sink.clone()))
            .build()
    }
}

impl<E, S> MetricsLayer<E, S>
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
{
    /// Creates a new [`MetricsLayerBuilder`] for configuring and building a [`MetricsLayer`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = MetricsLayer::builder()
    ///     .init_metrics(|| MyMetrics::default().append_on_drop(sink))
    ///     .build();
    /// ```
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, E, S> {
        MetricsLayerBuilder {
            init_metrics: None,
            request_metrics: None,
            response_metrics: None,
            default_req_metrics_config: DefaultRequestMetricsConfig::default(),
            default_res_metrics_config: DefaultResponseMetricsConfig::default(),
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<Ser, E, S, I, Rq, Rs> Layer<Ser> for MetricsLayer<E, S, I, Rq, Rs>
where
    Ser: Clone,
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    I: InitMetrics<E, S>,
    Rq: RequestMetrics<E>,
    Rs: ResponseMetrics<E>,
{
    type Service = MetricsLayerService<Ser, E, S, I, Rq, Rs>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService {
            inner,
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

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use http::Request;
use metrique::DefaultSink;
use metrique::ServiceMetrics;
use metrique_writer::GlobalEntrySink;
use thiserror::Error;
use tower::Layer;

use crate::default::DefaultMetrics;
use crate::default::DefaultMetricsServiceState;
use crate::default::DefaultRequestMetricsConfig;
use crate::default::DefaultResponseMetricsConfig;
use crate::layer::builder::DefaultMetricsBuildExt;
use crate::layer::builder::MetricsLayerBuilder;
use crate::layer::builder::NeedsInitialization;
use crate::service::MetricsLayerService;
use crate::traits::InitMetrics;
use crate::traits::ResponseMetrics;
use crate::traits::ThreadSafeCloseEntry;
use crate::traits::ThreadSafeEntrySink;
use crate::types::DefaultInit;
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
    Entry = DefaultMetrics,
    Sink = DefaultSink,
    Init = DefaultInit<Entry, Sink>,
    Res = DefaultRs<Entry>,
> where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    pub(crate) init_metrics: Init,
    pub(crate) response_metrics: Option<Res>,
    pub(crate) default_metrics_extension_fn: fn(
        &mut Request<ReqBody>,
        &mut Entry,
        DefaultRequestMetricsConfig,
        DefaultResponseMetricsConfig,
        DefaultMetricsServiceState,
    ),
    pub(crate) default_req_metrics_config: DefaultRequestMetricsConfig,
    pub(crate) default_res_metrics_config: DefaultResponseMetricsConfig,

    pub(crate) _entry_sink: PhantomData<Sink>,
}

impl MetricsLayer {
    pub fn new(
    ) -> MetricsLayer<DefaultMetrics, DefaultSink, impl InitMetrics<DefaultMetrics, DefaultSink>>
    {
        Self::builder()
            .init_metrics(|_req| DefaultMetrics::default().append_on_drop(ServiceMetrics::sink()))
            .build()
    }
}

impl<Entry, Sink, Init, Res> MetricsLayer<Entry, Sink, Init, Res>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    #[doc(hidden)]
    pub fn __macro_new(
        init_metrics: Init,
        response_metrics: Option<Res>,
        default_metrics_extension_fn: fn(
            &mut Request<ReqBody>,
            &mut Entry,
            DefaultRequestMetricsConfig,
            DefaultResponseMetricsConfig,
            DefaultMetricsServiceState,
        ),
        default_req_metrics_config: DefaultRequestMetricsConfig,
        default_res_metrics_config: DefaultResponseMetricsConfig,
    ) -> Self {
        Self {
            init_metrics,
            response_metrics,
            default_metrics_extension_fn,
            default_req_metrics_config,
            default_res_metrics_config,

            _entry_sink: PhantomData,
        }
    }
}

impl<Sink> MetricsLayer<DefaultMetrics, Sink>
where
    Sink: ThreadSafeEntrySink<DefaultMetrics> + Clone,
{
    /// Creates a new metrics layer with the provided sink and default metrics. For default
    /// metrics to be set automatically, [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin)
    /// must be added to the service's HTTP plugins.
    ///
    /// This is a convenience method for when you want to have a custom sink with the default
    /// metrics configuration.
    ///
    /// For additional control, use [`MetricsLayer::builder()`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = MetricsLayer::new_with_sink(my_sink);
    /// ```
    pub fn new_with_sink(
        sink: Sink,
    ) -> MetricsLayer<DefaultMetrics, Sink, impl InitMetrics<DefaultMetrics, Sink>> {
        Self::builder()
            .init_metrics(move |_req| DefaultMetrics::default().append_on_drop(sink.clone()))
            .build()
    }
}

impl<Entry, Sink> MetricsLayer<Entry, Sink>
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
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
    pub fn builder() -> MetricsLayerBuilder<NeedsInitialization, Entry, Sink> {
        MetricsLayerBuilder {
            init_metrics: None,
            response_metrics: None,
            default_req_metrics_config: DefaultRequestMetricsConfig::default(),
            default_res_metrics_config: DefaultResponseMetricsConfig::default(),
            _state: PhantomData,
            _close_entry: PhantomData,
            _entry_sink: PhantomData,
        }
    }
}

impl<Ser, Entry, Sink, Init, Res> Layer<Ser> for MetricsLayer<Entry, Sink, Init, Res>
where
    Ser: Clone,
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    Init: InitMetrics<Entry, Sink>,
    Res: ResponseMetrics<Entry>,
{
    type Service = MetricsLayerService<Ser, Entry, Sink, Init, Res>;

    fn layer(&self, inner: Ser) -> Self::Service {
        MetricsLayerService {
            inner,
            init_metrics: self.init_metrics.clone(),
            response_metrics: self.response_metrics.clone(),
            default_metrics_extension_fn: self.default_metrics_extension_fn,
            default_req_metrics_config: self.default_req_metrics_config.clone(),
            default_res_metrics_config: self.default_res_metrics_config.clone(),
            default_service_state: DefaultMetricsServiceState::default(),

            _entry_sink: PhantomData,
        }
    }
}

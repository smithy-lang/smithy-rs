/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use metrique::DefaultSink;
use metrique::ServiceMetrics;
use metrique_writer::GlobalEntrySink;
use thiserror::Error;
use tower::Layer;

use crate::default::DefaultMetricsConfig;
use crate::default::SmithyMetrics;
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
use crate::types::HttpRequest;

pub mod builder;

/// Errors that can occur when creating a metrics layer
#[derive(Error, Debug)]
pub enum DefaultMetricsLayerError {
    /// No sink was attached to the service metrics
    #[error("No sink attached to [`metrique::ServiceMetrics`]")]
    NoSinkAttached,
}

/// A Tower layer that publishes metrics for HTTP requests and responses, allowing you to define and
/// initialize metrics that can be set throughout a request/response lifecycle.
///
/// When standard server metrics have been enabled by calling the generated service config builder's
/// `enable_standard_metrics()` method, they will be folded into metrics from this layer.
///
/// # Usage
///
/// Typically, this should be layered as the outermost middleware layer, so that the rest of the
/// request path has access to metrics request extensions added by this, and this has access to any
/// metrics response extensions added by the response path.
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
/// Then add it to your service as outer middleware, typically layered after any other middleware to
/// ensure that it is layered at the outermost level:
///
/// ```rust,ignore
/// let app = metrics_layer.layer(app);
/// ```
///
/// If you just want a custom sink with standard metrics, provided that the generated service config
/// builder's `enable_standard_metrics()` method has been called, use
/// [`new_with_sink()`](MetricsLayer::new_with_sink):
///
/// ```rust,ignore
/// let metrics_layer = MetricsLayer::new_with_sink(my_sink);
/// let app = metrics_layer.layer(app);
/// ```
///
/// # Interoperability with other middleware layers
///
/// If another middleware layer between `MetricsLayer` and the Smithy service has the effect of
/// invoking the Smithy service multiple times per request, `MetricsLayer` will only publish metrics
/// relating to the *first* such invocation. To ensure that metrics are published for all
/// invocations, ensure that `MetricsLayer` is *inside* the layer that calls its inner service
/// multiple times.
#[derive(Debug)]
pub struct MetricsLayer<
    Entry = SmithyMetrics,
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
    /// Adds request extensions for the entry, such as handles to its operation metrics.
    pub(crate) init_extensions: fn(&mut HttpRequest, &mut Entry),
    pub(crate) default_metrics_config: DefaultMetricsConfig,

    pub(crate) _entry_sink: PhantomData<Sink>,
}

impl MetricsLayer {
    /// Creates a `MetricsLayer` using the global [`ServiceMetrics`] sink and the default metrics
    /// type ([`SmithyMetrics`]).
    ///
    /// For standard server metrics to be included automatically, the `enable_standard_metrics()`
    /// method must be called on the generated service config builder.
    ///
    /// This is a convenience method for when you want to use a default sink with the default
    /// metrics configuration. For additional control, use [`MetricsLayer::builder()`].
    pub fn new(
    ) -> MetricsLayer<SmithyMetrics, DefaultSink, impl InitMetrics<SmithyMetrics, DefaultSink>>
    {
        Self::builder()
            .init_metrics(|_req| SmithyMetrics::default().append_on_drop(ServiceMetrics::sink()))
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
        init_extensions: fn(&mut HttpRequest, &mut Entry),
        default_metrics_config: DefaultMetricsConfig,
    ) -> Self {
        Self {
            init_metrics,
            response_metrics,
            init_extensions,
            default_metrics_config,

            _entry_sink: PhantomData,
        }
    }
}

impl<Sink> MetricsLayer<SmithyMetrics, Sink>
where
    Sink: ThreadSafeEntrySink<SmithyMetrics> + Clone,
{
    /// Creates a new metrics layer with the provided sink and default metrics type
    /// ([`SmithyMetrics`]).
    ///
    /// For standard server metrics to be included automatically, the `enable_standard_metrics()`
    /// method must be called on the generated service config builder.
    ///
    /// This is a convenience method for when you want to have a custom sink with the default
    /// metrics configuration. For additional control, use [`MetricsLayer::builder()`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = MetricsLayer::new_with_sink(my_sink);
    /// ```
    pub fn new_with_sink(
        sink: Sink,
    ) -> MetricsLayer<SmithyMetrics, Sink, impl InitMetrics<SmithyMetrics, Sink>> {
        Self::builder()
            .init_metrics(move |_req| SmithyMetrics::default().append_on_drop(sink.clone()))
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
            default_metrics_config: DefaultMetricsConfig::default(),
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
            init_extensions: self.init_extensions,
            default_metrics_config: self.default_metrics_config.clone(),

            _entry_sink: PhantomData,
        }
    }
}

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics publication for Smithy Rust servers.
//!
//! This crate provides [`MetricsLayer`], which publishes a [metrique] metrics entry for each
//! request. Each entry can contain:
//! 1. **Default metrics**: the standard server metrics collected by the server, if the generated
//!    service config builder's `enable_standard_metrics()` method has been called. See [Default
//!    metrics](#default-metrics) below.
//! 2. **Custom metrics**: metrics you define, recorded by the layers, plugins, operation handlers,
//!    and other middleware.
//!
//! Each entry is published once its request has finished: its response has been fully sent, sending
//! the response failed, or handling the request was abandoned.
//!
//! # Publishing default metrics
//!
//! Enable standard metrics in the service config, and add a [`MetricsLayer`] around the service:
//!
//! ```rust, ignore
//! use aws_smithy_http_server_metrics::MetricsLayer;
//! use tower::Layer;
//!
//! let config = PokemonServiceConfig::builder()
//!     .enable_standard_metrics()
//!     .build();
//! let app = PokemonService::builder(config)
//!     /* operation handlers */
//!     .build()?;
//! let app = MetricsLayer::new().layer(app);
//! ```
//!
//! [`MetricsLayer::new`] publishes to metrique's global sink for application-wide metrics,
//! [`ServiceMetrics`](metrique::ServiceMetrics), which must be configured, e.g.:
//!
//! ```rust
//! use metrique::ServiceMetrics;
//! use metrique_writer::AttachGlobalEntrySinkExt;
//! use metrique_writer::FormatExt;
//! use metrique_writer_format_emf::Emf;
//! use tracing_appender::rolling::RollingFileAppender;
//! use tracing_appender::rolling::Rotation;
//!
//! // Attach a global sink to ServiceMetrics (e.g. CloudWatch EMF)
//! let service_log_dir = "logs";
//! let service_log_name = "pokemon_service_metrics";
//!
//! let emf = Emf::builder(
//!         "Ns".to_string(),
//!         vec![
//!             vec![],
//!             vec![
//!                 "service".to_string(),
//!                 "service_version".to_string(),
//!                 "operation".to_string(),
//!             ],
//!         ],
//!     )
//!     .build()
//!     .output_to_makewriter(RollingFileAppender::new(
//!         Rotation::MINUTELY,
//!         service_log_dir,
//!         service_log_name,
//!     ));
//! ServiceMetrics::attach_to_stream(emf);
//! ```
//!
//! To publish to another sink, use [`MetricsLayer::new_with_sink`].
//!
//! # Publishing custom metrics
//!
//! Use [`MetricsLayer::builder`] to define your own metrics, which are published alongside the
//! default metrics.
//!
//! ```rust, ignore
//! use aws_smithy_http_server_metrics::MetricsLayer;
//! use metrique::ServiceMetrics;
//! use metrique::writer::GlobalEntrySink;
//! use metrique::unit_of_work::metrics;
//! use metrique_macro::metrics;
//!
//! let metrics_layer = MetricsLayer::builder()
//!     .init_metrics(|request| {
//!         let mut metrics = MyMetrics::default().append_on_drop(ServiceMetrics::sink());
//!         metrics.request_metrics.user_agent = request.headers()
//!             .get("user-agent")
//!             .and_then(|v| v.to_str().ok())
//!             .map(Into::into);
//!         metrics
//!     })
//!     .response_metrics(|response, metrics| {
//!         metrics.response_metrics.content_length = response
//!             .headers()
//!             .get("content-length")
//!             .and_then(|v| v.to_str().ok())
//!             .and_then(|v| v.parse().ok());
//!     })
//!     .build();
//!
//! let app = metrics_layer.layer(app);
//! ```
//!
//! Use the `#[smithy_metrics]` macro to integrate with a metrique metrics struct.
//!
//! Use the `#[smithy_metrics(operation)` macro on a field for metrics access of that field in
//! operation handlers.
//!
//! ```rust
//! use aws_smithy_http_server_metrics::operation::Metrics;
//! use aws_smithy_http_server_metrics::smithy_metrics;
//! use metrique::unit_of_work::metrics;
//!
//! #[smithy_metrics]
//! #[metrics]
//! struct MyMetrics {
//!     #[metrics(flatten)]
//!     request_metrics: MyRequestMetrics,
//!     #[metrics(flatten)]
//!     response_metrics: MyResponseMetrics,
//!     #[metrics(flatten)]
//!     operation_metrics: MyOperationMetrics,
//! }
//!
//! #[metrics]
//! struct MyRequestMetrics {
//!     my_request_metric: Option<String>,
//! }
//!
//! #[metrics]
//! struct MyResponseMetrics {
//!     my_response_metric: Option<String>,
//! }
//!
//! #[metrics]
//! struct MyOperationMetrics {
//!     my_operation_metric: Option<String>,
//! }
//!
//! // Does not include the input parameter and output return type for example's sake
//! fn operation_handler(mut metrics: Metrics<MyOperationMetrics>) {
//!     metrics.my_operation_metric = Some("example".to_string());
//! }
//! ```
//!
//! For a complete example, see the [pokemon-service
//! example](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service) for an
//! example that uses CloudWatch EMF (Embedded Metric Format) as an output format.
//!
//! # Default metrics
//!
//! When standard metrics are enabled, each entry contains the following default metrics, unless
//! they have been disabled in the [`MetricsLayerBuilder`] configuration.
//!
//! | Metric | Description |
//! |--------|-------------|
//! | `service` | Name of the service |
//! | `service_version` | Version of the service, if the model specifies one |
//! | `operation` | Name of the operation the request was routed to; absent if it was not routed to an operation |
//! | `request_id` | ID assigned to the request by `ServerRequestIdProviderLayer`; absent if none was assigned |
//! | `outstanding_requests` | Number of requests to the service in progress when this request started, including this one |
//! | `http_status_code` | HTTP status code of the response |
//! | `success` | Whether the request succeeded |
//! | `client_error` | Whether the request failed because of the client |
//! | `server_error` | Whether the request failed because of the server |
//! | `operation_time` | Time spent executing the operation's handler and any model plugins; zero if they did not execute |
//!
//! `http_status_code`, `success`, `client_error` and `server_error` are absent if the service
//! produced no response. Requests are classified as successes, client errors or server errors as
//! described by [`Outcome`](aws_smithy_http_server::metrics::Outcome).
//!
//! # Contributing metrics dynamically
//!
//! Most metrics are recorded by code that can name the request's metrics entry type: the metrics
//! layer and operation handlers through their own `#[smithy_metrics(operation)]` field (see
//! [`OperationMetrics`]). Prefer that whenever possible.
//!
//! Independently-owned middleware and libraries *cannot* name the entry type. For them, the metrics
//! layer stores a [`SmithyMetricsPool`] in the request extensions. Pull it out and append a
//! prefixed child metrics struct; it flattens into the single per-request entry when the request
//! finishes:
//!
//! ```rust,ignore
//! use aws_smithy_http_server_metrics::SmithyMetricsPool;
//! use metrique::unit_of_work::metrics;
//!
//! #[metrics]
//! struct MiddlewareMetrics {
//!     middleware_calls: u64,
//! }
//!
//! // `req` is the incoming request held by the middleware.
//! if let Some(pool) = req.extensions().get::<SmithyMetricsPool>() {
//!     pool.with_prefix(["middleware"])
//!         .append(MiddlewareMetrics { middleware_calls: 1 });
//! }
//! ```
//!
//! Give each contributor a prefix distinct from the parent's fields and from every other
//! contributor (via [`with_prefix`](metrique_util::MetricsPoolHandle::with_prefix) or
//! [`with_exact_prefix`](metrique_util::MetricsPoolHandle::with_exact_prefix)). For how the pool
//! resolves field-name collisions among its children, see the
//! [`MetricsPool`](metrique_util::MetricsPool) crate-level documentation.
//!
//! # Platform support
//!
//! MIPS and PowerPC are currently not supported due to lack of
//! [`AtomicU64`](std::sync::atomic::AtomicU64) support. Once
//! <https://github.com/smithy-lang/smithy-rs/pull/4487> and
//! <https://github.com/awslabs/metrique/issues/183> are complete, we can use the latest metrique
//! version and support these.

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![cfg(not(any(target_arch = "mips", target_arch = "powerpc")))]

pub mod default;
pub mod layer;
pub mod operation;
pub mod pool;
pub mod service;
pub mod traits;
pub mod types;

pub use layer::builder::MetricsLayerBuilder;
pub use layer::MetricsLayer;
pub use operation::Metrics as OperationMetrics;
pub use pool::SmithyMetricsPool;

pub use aws_smithy_http_server_metrics_macro::smithy_metrics;

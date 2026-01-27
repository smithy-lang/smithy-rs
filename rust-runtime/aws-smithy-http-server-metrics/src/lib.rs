/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics collection for Smithy Rust servers.
//!
//! This crate provides two ways to collect metrics:
//! 1. **Default metrics** via [`DefaultMetricsPlugin`] - automatic collection of standard metrics
//! 2. **Custom metrics** via [`MetricsLayer`] - define your own metrics extracted from requests/responses
//!
//! # DefaultMetricsPlugin
//!
//! [`DefaultMetricsPlugin`] automatically collects standard metrics for every request.
//! See the Collected Metrics section below:
//! 
//! ```rust,ignore
//! use aws_smithy_http_server_metrics::plugin::DefaultMetricsPlugin;
//!
//! let http_plugins = HttpPlugins::new()
//!     .push(DefaultMetricsPlugin)
//!     .instrument();
//! ```
//!
//! **Important**: [`DefaultMetricsPlugin`] requires a metrics sink to be configured,
//! otherwise metrics collection will be skipped and an info log will be emitted.
//! 
//! To use a custom sink, you can add a [`MetricsLayer`]. See the MetricsLayer section below.
//!
//! If no [`MetricsLayer`] exists, [`DefaultMetricsPlugin`] will use metrique's global sink for
//! application-wide metrics, [`ServiceMetrics`](metrique::ServiceMetrics). It will therefore
//! need to be configured, e.g.:
//!
//! ```rust,ignore
//! use metrique::ServiceMetrics;
//! use metrique_writer::AttachGlobalEntrySinkExt;
//! use tracing_appender::rolling::RollingFileAppender;
//! use tracing_appender::rolling::Rotation;
//!
//! // Attach a global sink to ServiceMetrics (e.g., CloudWatch EMF)
//! let service_log_dir = "logs";
//! let service_log_name = "pokemon_service_metrics";
//!
//! let emf = Emf::builder("Ns".to_string(), vec![vec![]])
//!     .build()
//!     .output_to_makewriter(RollingFileAppender::new(
//!         Rotation::MINUTELY,
//!         service_log_dir,
//!         service_log_name,
//!     ));
//! ServiceMetrics::attach_to_stream(emf_sink);
//! ```
//!
//! ## Collected Metrics
//!
//! ### Request Metrics
//!
//! | Metric | Type | Description |
//! |--------|------|-------------|
//! | `service_name` | Property | Name of the service |
//! | `service_version` | Property | Version of the service |
//! | `operation_name` | Property | Name of the operation being invoked |
//! | `request_id` | Property | Unique identifier for the request |
//!
//! ### Response Metrics
//!
//! | Metric | Type | Description |
//! |--------|------|-------------|
//! | `http_status_code` | Property | HTTP status code of the response |
//! | `error` | Counter | Client error indicator (4xx status code) |
//! | `fault` | Counter | Server fault indicator (5xx status code) |
//! | `operation_time` | Duration | Time from pre-deserialization to post-serialization |
//!
//! # MetricsLayer
//!
//! Use [`MetricsLayer`] to customize metrics collection and/or define your own metrics:
//! 
//! Default metrics from [`DefaultMetricsPlugin`](crate::plugin::DefaultMetricsPlugin)
//! will be folded into metrics from this layer if it is in the service's HTTP plugins.
//! 
//! A [`MetricsLayer`] can be used to customize the sink with the default metrics using
//! [`MetricsLayer::new_with_sink`].
//! 
//! For full control, use [`MetricsLayer::builder`].
//!
//! ```rust,ignore
//! use aws_smithy_http_server_metrics::MetricsLayer;
//!
//! let metrics_layer = MetricsLayer::builder()
//!     .init_metrics(|| MyMetrics::default().append_on_drop(sink))
//!     .request_metrics(|request, metrics| {
//!         metrics.request_metrics.user_agent = request.headers()
//!             .get("user-agent")
//!             .and_then(|v| v.to_str().ok())
//!             .map(Into::into);
//!     })
//!     .response_metrics(|response, metrics| {
//!         metrics.response_metrics.content_length = response
//!             .headers()
//!             .get("content-length")
//!             .and_then(|v| v.to_str().ok())
//!             .and_then(|v| v.parse().ok());
//!     .build();
//!
//! let app = app.layer(metrics_layer);
//! ```
//!
//! Use the `#[smithy_metrics]` macro to integrate with a metrique metrics struct:
//!
//! ```rust,ignore
//! #[smithy_metrics]
//! #[metrics]
//! #[derive(Default)]
//! pub struct MyMetrics {
//!     #[metrics(flatten)]
//!     pub request_metrics: MyRequestMetrics,
//!     #[metrics(flatten)]
//!     pub response_metrics: MyResponseMetrics,
//! }
//! ```
//! 
//! For a complete example, see the [pokemon-service example](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service).
//!
//! # Output Format Examples
//!
//! ## CloudWatch EMF (Embedded Metric Format)
//!
//! Metrics are emitted as structured JSON logs that CloudWatch automatically converts to metrics:
//!
//! ```json
//! {
//!   "_aws": {
//!     "Timestamp": 1737847711257,
//!     "CloudWatchMetrics": [{
//!       "Namespace": "MyService",
//!       "Dimensions": [[]],
//!       "Metrics": [{"Name": "error"}, {"Name": "fault"}]
//!     }]
//!   },
//!   "error": 0,
//!   "fault": 0,
//!   "service_name": "MyService",
//!   "operation_name": "MyOperation",
//!   "http_status_code": 200
//! }
//! ```

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */

pub mod default;
pub mod extension;
pub mod layer;
pub mod plugin;
pub mod service;
pub mod traits;
pub mod types;

pub use layer::builder::MetricsLayerBuilder;
pub use layer::MetricsLayer;
pub use plugin::DefaultMetricsPlugin;

pub use aws_smithy_http_server_metrics_macro::smithy_metrics;

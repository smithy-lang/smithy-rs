/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Metrics collection for Smithy Rust servers.
//!
//! This crate provides two ways to collect metrics:
//! 1. **Default metrics** via [`DefaultMetricsPlugin`] - collection of standard metrics at the operation level
//! 2. **Custom metrics** via [`MetricsLayer`] - collection of custom metrics and standard metrics at the outer request level
//!
//! # DefaultMetricsPlugin
//!
//! [`DefaultMetricsPlugin`] automatically collects standard metrics for every request at operation time.
//! See the Collected Metrics section below:
//!
//! ```rust, ignore
//! use aws_smithy_http_server_metrics::plugin::DefaultMetricsPlugin;
//! use aws_smithy_http_server::plugin::HttpPlugins;
//!
//! let http_plugins = HttpPlugins::new()
//!     .push(DefaultMetricsPlugin);
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
//! # MetricsLayer
//!
//! Use [`MetricsLayer`] to customize metrics collection and/or define your own metrics:
//!
//! Default metrics from [`DefaultMetricsPlugin`]
//! will be folded into metrics from this layer if it is in the service's HTTP plugins.
//!
//! A [`MetricsLayer`] can be used to customize the sink with the default metrics using
//! [`MetricsLayer::new_with_sink`].
//!
//! For full control, use [`MetricsLayer::builder`].
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
//! let app = app.layer(metrics_layer);
//! ```
//!
//! Use the `#[smithy_metrics]` macro to integrate with a metrique metrics struct.
//!
//! Use the `#[smithy_metrics(operation)` macro on a field for metrics access of that
//! field in operation handlers.
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
//! For a complete example, see the [pokemon-service example](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service)
//! for an example that uses CloudWatch EMF (Embedded Metric Format) as an output format.
//!
//! # Collected Metrics
//!
//! ## DefaultMetricsPlugin
//!
//! The [`DefaultMetricsPlugin`] provides the following metrics by default.
//!
//! For fully successful requests/responses, these are guaranteed to be present if
//! they have not been explicitly disabled in the builder configuration.
//! Response metrics may not be present if the future is dropped before the backswing.
//!
//! ### Request Metrics
//!
//! | Metric | Description |
//! |--------|-------------|
//! | `service` | Name of the service |
//! | `service_version` | Version of the service |
//! | `operation` | Name of the operation being invoked |
//! | `request_id` | Unique identifier for the request |
//! | `outstanding_requests` | Number of concurrent requests counting any operation being processing in that moment |
//!
//! ### Response Metrics
//!
//! | Metric | Description |
//! |--------|-------------|
//! | `http_status_code` | HTTP status code of the response |
//! | `client_error` | Client error indicator (4xx status code) |
//! | `server_error` | Server error indicator (5xx status code) |
//! | `operation_time` | Timestamp that denotes operation time from pre-deserialization to post-serialization |
//!
//! # Platform support
//!
//! MIPS and PowerPC are currently not supported due to lack of
//! [`AtomicU64`](std::sync::atomic::AtomicU64) support. Once
//! <https://github.com/smithy-lang/smithy-rs/pull/4487> and
//! <https://github.com/awslabs/metrique/issues/183> are complete,
//! we can use the latest metrique version and support these.

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![cfg(not(any(target_arch = "mips", target_arch = "powerpc")))]

pub mod default;
pub mod layer;
pub mod operation;
pub mod plugin;
pub mod service;
pub mod traits;
pub mod types;

pub use layer::builder::MetricsLayerBuilder;
pub use layer::MetricsLayer;
pub use operation::Metrics as OperationMetrics;
pub use plugin::DefaultMetricsPlugin;

pub use aws_smithy_http_server_metrics_macro::smithy_metrics;
